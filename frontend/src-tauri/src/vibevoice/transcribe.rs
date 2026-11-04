//! Orchestrates one VibeVoice transcription: health check, request, progress polling,
//! cancellation and conversion to storable segments.

use super::client::{StatusResponse, VibeVoiceClient, START_HINT};
use super::settings::{self, VibeVoiceSettings};
use super::store::{build_segments, SpeakerSegment};
use crate::state::AppState;
use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Progress range (percent) that the VibeVoice step occupies in the import/retranscription bar.
pub const PROGRESS_START: u32 = 25;
pub const PROGRESS_END: u32 = 80;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub struct VibeVoiceOutput {
    pub segments: Vec<SpeakerSegment>,
    pub duration_seconds: f64,
    pub chunks: u32,
    pub warnings: Vec<String>,
    pub speaker_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VibeVoiceWarningEvent {
    pub meeting_id: Option<String>,
    pub warnings: Vec<String>,
}

async fn load_settings<R: Runtime>(app: &AppHandle<R>) -> Result<VibeVoiceSettings> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    settings::load(state.db_manager.pool()).await
}

/// Load settings and verify the sidecar is reachable and idle.
/// Errors carry a user-facing message explaining how to start the server.
pub async fn ensure_server_ready<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(VibeVoiceClient, VibeVoiceSettings)> {
    let settings = load_settings(app).await?;
    let client = VibeVoiceClient::new(&settings.server_url)?;
    let health = client.health().await?;
    if health.status != "ok" {
        return Err(anyhow!(
            "VibeVoice server is not healthy (status '{}'). {}",
            health.status,
            START_HINT
        ));
    }
    if health.busy {
        return Err(anyhow!(
            "VibeVoice server is busy with another transcription. Try again when it finishes."
        ));
    }
    info!(
        "VibeVoice server ready (version {:?}, model loaded: {})",
        health.version, health.model_loaded
    );
    Ok((client, settings))
}

/// Map server-side status to an overall percentage and a message.
pub fn progress_from_status(st: &StatusResponse, elapsed: Duration) -> (u32, String) {
    let span = (PROGRESS_END - PROGRESS_START) as f32;
    let pct = match st.stage.as_str() {
        "converting" => PROGRESS_START,
        "loading" => PROGRESS_START + 2,
        "transcribing" if st.total > 0 => {
            PROGRESS_START + 5 + ((st.done as f32 / st.total as f32) * (span - 5.0)) as u32
        }
        "done" => PROGRESS_END,
        _ => PROGRESS_START,
    }
    .min(PROGRESS_END);
    let secs = elapsed.as_secs();
    let base = if st.message.is_empty() {
        "VibeVoice is transcribing...".to_string()
    } else {
        st.message.clone()
    };
    let hint = if st.stage == "loading" {
        " (first run downloads/loads ~6 GB)"
    } else {
        ""
    };
    (
        pct,
        format!(
            "{}{} — {}m {:02}s elapsed",
            base,
            hint,
            secs / 60,
            secs % 60
        ),
    )
}

/// Transcribe one audio file with the VibeVoice sidecar.
///
/// `progress(percent, message)` is called periodically; `cancelled` is polled and, if set,
/// the request is abandoned and `Err(cancel_message)` is returned. The sidecar finishes its
/// current pass in the background (it cannot be interrupted mid-inference).
pub async fn transcribe_file<R, F>(
    app: &AppHandle<R>,
    meeting_id: Option<&str>,
    audio_path: &Path,
    client_and_settings: Option<(VibeVoiceClient, VibeVoiceSettings)>,
    progress: F,
    cancelled: &AtomicBool,
    cancel_message: &str,
) -> Result<VibeVoiceOutput>
where
    R: Runtime,
    F: Fn(u32, &str),
{
    let (client, settings) = match client_and_settings {
        Some(cs) => cs,
        None => ensure_server_ready(app).await?,
    };

    let audio_path_str = audio_path
        .canonicalize()
        .unwrap_or_else(|_| audio_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    debug!("VibeVoice input file: {}", audio_path_str);
    info!(
        "Sending recording to VibeVoice ({} hotwords)",
        settings.hotwords.len()
    );
    progress(PROGRESS_START, "Sending audio to VibeVoice server...");

    let started = Instant::now();
    let request = client.transcribe(&audio_path_str, &settings.hotwords);
    tokio::pin!(request);
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.tick().await; // first tick fires immediately

    let response = loop {
        tokio::select! {
            res = &mut request => break res?,
            _ = ticker.tick() => {
                if cancelled.load(Ordering::SeqCst) {
                    warn!("VibeVoice request abandoned by user; the server will finish its current pass");
                    return Err(anyhow!("{}", cancel_message));
                }
                match client.status().await {
                    Ok(st) if st.active => {
                        let (pct, msg) = progress_from_status(&st, started.elapsed());
                        progress(pct, &msg);
                    }
                    Ok(_) => {}
                    Err(e) => debug!("VibeVoice status poll failed: {}", e),
                }
            }
        }
    };

    let segments = build_segments(&response);
    let speaker_count = segments
        .iter()
        .map(|s| s.speaker.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    info!(
        "VibeVoice finished: {:.0}s audio, {} segments, {} speakers, {} chunk(s) in {:.1}s",
        response.duration,
        segments.len(),
        speaker_count,
        response.chunks,
        started.elapsed().as_secs_f64()
    );

    if !response.warnings.is_empty() {
        for w in &response.warnings {
            warn!("VibeVoice: {}", w);
        }
        let _ = app.emit(
            "vibevoice-warning",
            VibeVoiceWarningEvent {
                meeting_id: meeting_id.map(str::to_string),
                warnings: response.warnings.clone(),
            },
        );
    }
    if segments.is_empty() {
        return Err(anyhow!(
            "VibeVoice returned no speech segments for this recording"
        ));
    }

    progress(PROGRESS_END, "VibeVoice transcription complete");
    Ok(VibeVoiceOutput {
        segments,
        duration_seconds: response.duration,
        chunks: response.chunks,
        warnings: response.warnings,
        speaker_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(stage: &str, done: u32, total: u32) -> StatusResponse {
        StatusResponse {
            active: true,
            stage: stage.into(),
            done,
            total,
            message: "m".into(),
        }
    }

    #[test]
    fn progress_mapping_is_monotonic_and_bounded() {
        let e = Duration::from_secs(75);
        assert_eq!(
            progress_from_status(&st("converting", 0, 0), e).0,
            PROGRESS_START
        );
        let a = progress_from_status(&st("transcribing", 0, 3), e).0;
        let b = progress_from_status(&st("transcribing", 1, 3), e).0;
        let c = progress_from_status(&st("transcribing", 3, 3), e).0;
        assert!(PROGRESS_START < a && a < b && b < c && c <= PROGRESS_END);
        assert_eq!(progress_from_status(&st("done", 3, 3), e).0, PROGRESS_END);
        let (_, msg) = progress_from_status(&st("loading", 0, 1), e);
        assert!(msg.contains("1m 15s elapsed"));
        assert!(msg.contains("~6 GB"));
    }
}
