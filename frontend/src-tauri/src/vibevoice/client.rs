//! HTTP client for the local VibeVoice sidecar plus response parsing.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Health checks must be fast; the transcription itself can take a long time
/// (roughly real-time on first load for a long meeting), so it gets a long timeout.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub const START_HINT: &str =
    "Start it from the Meetily repository with:  cd vibevoice-server && ./start.sh";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_loaded: bool,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub mlx_audio_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StatusResponse {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerSegment {
    /// Speaker id as a string ("0", "1", or "2-0" for chunked recordings).
    #[serde(deserialize_with = "de_speaker")]
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscribeResponse {
    pub segments: Vec<ServerSegment>,
    #[serde(default)]
    pub language: Option<String>,
    pub duration: f64,
    #[serde(default = "one")]
    pub chunks: u32,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn one() -> u32 {
    1
}

/// Accept speaker ids as strings or numbers (0 -> "0").
fn de_speaker<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => s.trim().to_string(),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(|i| i.to_string())
            .unwrap_or_else(|| n.to_string()),
        _ => "unknown".to_string(),
    })
}

/// Parse and sanity-check a `/transcribe` response body.
///
/// Drops empty-text segments, clamps negative/inverted times, and sorts by start.
pub fn parse_transcribe_response(body: &str) -> Result<TranscribeResponse> {
    let mut resp: TranscribeResponse =
        serde_json::from_str(body).context("VibeVoice server returned an unexpected response")?;
    if !resp.duration.is_finite() || resp.duration < 0.0 {
        resp.duration = 0.0;
    }
    resp.segments
        .retain(|s| !s.text.trim().is_empty() && s.start.is_finite() && s.end.is_finite());
    for s in &mut resp.segments {
        s.text = s.text.trim().to_string();
        s.start = s.start.max(0.0);
        if s.end < s.start {
            s.end = s.start;
        }
    }
    resp.segments.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then_with(|| a.end.total_cmp(&b.end))
    });
    Ok(resp)
}

/// Extract FastAPI's `{"detail": "..."}` error message, falling back to the raw body.
pub fn error_detail(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

pub struct VibeVoiceClient {
    base_url: String,
    http: reqwest::Client,
}

impl VibeVoiceClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy() // localhost only; never route through a system proxy
            .connect_timeout(Duration::from_secs(5))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn not_running(&self, e: &reqwest::Error) -> anyhow::Error {
        if e.is_connect() || e.is_timeout() {
            anyhow!(
                "VibeVoice server is not running at {}. {}",
                self.base_url,
                START_HINT
            )
        } else {
            anyhow!(
                "Could not reach VibeVoice server at {}: {}",
                self.base_url,
                e
            )
        }
    }

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map_err(|e| self.not_running(&e))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "VibeVoice health check failed ({}): {}",
                status,
                error_detail(&body)
            ));
        }
        let health: HealthResponse = serde_json::from_str(&body)
            .context("Server at this URL is not the Meetily VibeVoice sidecar")?;
        Ok(health)
    }

    pub async fn status(&self) -> Result<StatusResponse> {
        let resp = self
            .http
            .get(format!("{}/status", self.base_url))
            .timeout(STATUS_TIMEOUT)
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    pub async fn transcribe(
        &self,
        audio_path: &str,
        hotwords: &[String],
    ) -> Result<TranscribeResponse> {
        let resp = self
            .http
            .post(format!("{}/transcribe", self.base_url))
            .timeout(TRANSCRIBE_TIMEOUT)
            .json(&serde_json::json!({ "audio_path": audio_path, "hotwords": hotwords }))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    anyhow!(
                        "VibeVoice transcription timed out after {} hours",
                        TRANSCRIBE_TIMEOUT.as_secs() / 3600
                    )
                } else {
                    self.not_running(&e)
                }
            })?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .context("failed to read VibeVoice response")?;
        if !status.is_success() {
            let detail = error_detail(&body);
            return Err(match status.as_u16() {
                409 => anyhow!("VibeVoice server is busy with another transcription. Try again when it finishes."),
                _ => anyhow!("VibeVoice transcription failed ({}): {}", status, detail),
            });
        }
        parse_transcribe_response(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_response() {
        let body = r#"{"segments":[
            {"speaker":"1","start":3.5,"end":7.0,"text":"  second  "},
            {"speaker":0,"start":0.0,"end":3.4,"text":"first"},
            {"speaker":"2-0","start":8.0,"end":7.0,"text":"inverted"},
            {"speaker":null,"start":-1.0,"end":1.0,"text":"neg"},
            {"speaker":"0","start":9.0,"end":10.0,"text":"   "}
        ],"language":null,"duration":12.5,"chunks":2,"warnings":["split"],"processing_seconds":3.1}"#;
        let r = parse_transcribe_response(body).unwrap();
        assert_eq!(r.duration, 12.5);
        assert_eq!(r.chunks, 2);
        assert_eq!(r.warnings, vec!["split".to_string()]);
        let got: Vec<(&str, f64, f64, &str)> = r
            .segments
            .iter()
            .map(|s| (s.speaker.as_str(), s.start, s.end, s.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("unknown", 0.0, 1.0, "neg"),
                ("0", 0.0, 3.4, "first"),
                ("1", 3.5, 7.0, "second"),
                ("2-0", 8.0, 8.0, "inverted"),
            ]
        );
    }

    #[test]
    fn minimal_response_defaults() {
        let r = parse_transcribe_response(r#"{"segments":[],"duration":1}"#).unwrap();
        assert_eq!(r.chunks, 1);
        assert!(r.warnings.is_empty());
        assert!(r.language.is_none());
    }

    #[test]
    fn rejects_malformed_response() {
        assert!(parse_transcribe_response("not json").is_err());
        assert!(parse_transcribe_response(r#"{"segments":[{"text":"x"}],"duration":1}"#).is_err());
    }

    #[test]
    fn error_detail_extracts_fastapi_message() {
        assert_eq!(
            error_detail(r#"{"detail":"Could not read audio"}"#),
            "Could not read audio"
        );
        assert_eq!(error_detail("plain"), "plain");
    }

    #[test]
    fn health_parses_sidecar_payload() {
        let h: HealthResponse = serde_json::from_str(
            r#"{"status":"ok","model":"m","model_loaded":true,"busy":false,"version":"0.1.0","mlx_audio_version":null}"#,
        )
        .unwrap();
        assert!(h.model_loaded);
        assert_eq!(h.version.as_deref(), Some("0.1.0"));
    }
}
