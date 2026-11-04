//! Tauri commands for VibeVoice settings, health and per-meeting speaker names.

use super::client::{VibeVoiceClient, START_HINT};
use super::settings::{self, VibeVoiceSettings};
use super::speakers::{self, MeetingSpeaker};
use crate::state::AppState;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VibeVoiceHealthStatus {
    pub reachable: bool,
    pub server_url: String,
    pub model: Option<String>,
    pub model_loaded: bool,
    pub busy: bool,
    pub version: Option<String>,
    pub message: String,
}

#[tauri::command]
pub async fn vibevoice_get_settings(
    state: tauri::State<'_, AppState>,
) -> Result<VibeVoiceSettings, String> {
    settings::load(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn vibevoice_save_settings(
    settings: VibeVoiceSettings,
    state: tauri::State<'_, AppState>,
) -> Result<VibeVoiceSettings, String> {
    settings::save(state.db_manager.pool(), &settings)
        .await
        .map_err(|e| e.to_string())
}

/// Check the sidecar. `server_url` lets the settings screen test an unsaved URL.
#[tauri::command]
pub async fn vibevoice_health_check(
    server_url: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<VibeVoiceHealthStatus, String> {
    let url = match server_url {
        Some(u) => settings::normalize_server_url(&u).map_err(|e| e.to_string())?,
        None => {
            settings::load(state.db_manager.pool())
                .await
                .map_err(|e| e.to_string())?
                .server_url
        }
    };
    let client = VibeVoiceClient::new(&url).map_err(|e| e.to_string())?;
    Ok(match client.health().await {
        Ok(h) => VibeVoiceHealthStatus {
            reachable: true,
            server_url: url,
            message: if h.busy {
                "Connected — currently transcribing".into()
            } else if h.model_loaded {
                "Connected — model loaded".into()
            } else {
                "Connected — model loads on first use".into()
            },
            model: h.model,
            model_loaded: h.model_loaded,
            busy: h.busy,
            version: h.version,
        },
        Err(e) => VibeVoiceHealthStatus {
            reachable: false,
            server_url: url,
            model: None,
            model_loaded: false,
            busy: false,
            version: None,
            message: format!("{}", e).replace(START_HINT, "").trim().to_string(),
        },
    })
}

#[tauri::command]
pub async fn vibevoice_get_meeting_speakers(
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingSpeaker>, String> {
    speakers::list_speakers(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

/// Rename a speaker for one meeting. `display_name` empty/null resets to "Speaker N".
#[tauri::command]
pub async fn vibevoice_rename_speaker(
    meeting_id: String,
    speaker_id: String,
    display_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingSpeaker>, String> {
    let pool = state.db_manager.pool();
    speakers::rename_speaker(pool, &meeting_id, &speaker_id, display_name.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    log::info!("Renamed a speaker in meeting {}", meeting_id);
    speakers::list_speakers(pool, &meeting_id)
        .await
        .map_err(|e| e.to_string())
}
