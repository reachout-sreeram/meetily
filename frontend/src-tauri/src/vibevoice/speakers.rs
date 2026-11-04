//! Per-meeting speaker names: list speakers, rename them, resolve display names.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

pub const MAX_DISPLAY_NAME_LEN: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingSpeaker {
    pub speaker_id: String,
    /// Name to show: the user's rename, or the default "Speaker N".
    pub display_name: String,
    /// True if the user renamed this speaker.
    pub is_custom: bool,
    pub segment_count: i64,
    /// Total speaking time in seconds.
    pub total_seconds: f64,
}

/// Default label for a raw speaker id: "0" -> "Speaker 0", "2-1" -> "Speaker 2-1".
pub fn default_label(speaker_id: &str) -> String {
    format!("Speaker {}", speaker_id)
}

/// Resolve the display name for a speaker id given the meeting's rename map.
pub fn display_name(speaker_id: &str, names: &HashMap<String, String>) -> String {
    names
        .get(speaker_id)
        .cloned()
        .unwrap_or_else(|| default_label(speaker_id))
}

/// Trim and validate a user-entered name. `None`/empty means "reset to default".
pub fn normalize_display_name(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let name: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(anyhow!(
            "Speaker name is too long (max {} characters)",
            MAX_DISPLAY_NAME_LEN
        ));
    }
    Ok(Some(name))
}

/// Load the rename map for one meeting.
pub async fn load_names(pool: &SqlitePool, meeting_id: &str) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT speaker_id, display_name FROM meeting_speakers WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// All distinct speakers in a meeting's transcript, ordered by first appearance.
pub async fn list_speakers(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<MeetingSpeaker>> {
    let rows: Vec<(String, i64, f64)> = sqlx::query_as(
        "SELECT speaker,
                COUNT(*) AS segment_count,
                COALESCE(SUM(COALESCE(audio_end_time, 0) - COALESCE(audio_start_time, 0)), 0.0) AS total_seconds
         FROM transcripts
         WHERE meeting_id = ? AND speaker IS NOT NULL AND speaker != ''
         GROUP BY speaker
         ORDER BY MIN(COALESCE(audio_start_time, 0)) ASC",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    let names = load_names(pool, meeting_id).await?;
    Ok(rows
        .into_iter()
        .map(
            |(speaker_id, segment_count, total_seconds)| MeetingSpeaker {
                is_custom: names.contains_key(&speaker_id),
                display_name: display_name(&speaker_id, &names),
                speaker_id,
                segment_count,
                total_seconds,
            },
        )
        .collect())
}

/// Rename (or reset with `None`) a speaker for one meeting. Applies to every segment of
/// that speaker because names are resolved at read time, not copied into transcripts.
pub async fn rename_speaker(
    pool: &SqlitePool,
    meeting_id: &str,
    speaker_id: &str,
    new_name: Option<&str>,
) -> Result<()> {
    if meeting_id.trim().is_empty() || speaker_id.trim().is_empty() {
        return Err(anyhow!("meeting_id and speaker_id are required"));
    }
    let exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?")
            .bind(meeting_id)
            .bind(speaker_id)
            .fetch_one(pool)
            .await?;
    if exists.0 == 0 {
        return Err(anyhow!(
            "Speaker '{}' not found in this meeting",
            speaker_id
        ));
    }

    match normalize_display_name(new_name)? {
        Some(name) => {
            sqlx::query(
                "INSERT INTO meeting_speakers (meeting_id, speaker_id, display_name, updated_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(meeting_id, speaker_id)
                 DO UPDATE SET display_name = excluded.display_name, updated_at = excluded.updated_at",
            )
            .bind(meeting_id)
            .bind(speaker_id)
            .bind(name)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ? AND speaker_id = ?")
                .bind(meeting_id)
                .bind(speaker_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Display name for an optional raw speaker id (`None` for unlabelled segments).
pub fn resolve_name(speaker: Option<&str>, names: &HashMap<String, String>) -> Option<String> {
    speaker
        .filter(|s| !s.is_empty())
        .map(|id| display_name(id, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_custom_names() {
        let mut names = HashMap::new();
        names.insert("0".to_string(), "Priya".to_string());
        assert_eq!(display_name("0", &names), "Priya");
        assert_eq!(display_name("1", &names), "Speaker 1");
        assert_eq!(display_name("2-0", &names), "Speaker 2-0");
    }

    #[test]
    fn name_normalization() {
        assert_eq!(
            normalize_display_name(Some("  Priya   Rao ")).unwrap(),
            Some("Priya Rao".into())
        );
        assert_eq!(normalize_display_name(Some("   ")).unwrap(), None);
        assert_eq!(normalize_display_name(None).unwrap(), None);
        assert!(normalize_display_name(Some(&"x".repeat(81))).is_err());
    }

    #[test]
    fn resolve_name_skips_unlabelled() {
        let names = HashMap::from([("0".to_string(), "Priya".to_string())]);
        assert_eq!(resolve_name(Some("0"), &names).as_deref(), Some("Priya"));
        assert_eq!(
            resolve_name(Some("1"), &names).as_deref(),
            Some("Speaker 1")
        );
        assert_eq!(resolve_name(None, &names), None);
        assert_eq!(resolve_name(Some(""), &names), None);
    }
}
