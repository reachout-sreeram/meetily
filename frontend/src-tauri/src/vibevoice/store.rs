//! Persisting VibeVoice segments (with speaker ids) to the local database and meeting folder.

use super::client::TranscribeResponse;
use anyhow::{anyhow, Result};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpeakerSegment {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    pub speaker: String,
    pub audio_start_time: f64,
    pub audio_end_time: f64,
    pub duration: f64,
}

/// Convert a parsed sidecar response into DB-ready segments.
pub fn build_segments(resp: &TranscribeResponse) -> Vec<SpeakerSegment> {
    let now = chrono::Utc::now().to_rfc3339();
    resp.segments
        .iter()
        .map(|s| SpeakerSegment {
            id: format!("transcript-{}", Uuid::new_v4()),
            text: s.text.clone(),
            timestamp: now.clone(),
            speaker: s.speaker.clone(),
            audio_start_time: s.start,
            audio_end_time: s.end,
            duration: (s.end - s.start).max(0.0),
        })
        .collect()
}

/// Replace all transcripts of a meeting with speaker-labelled segments in one transaction.
///
/// Speaker renames for this meeting are cleared: a new VibeVoice pass assigns fresh ids,
/// so old "0 -> Priya" mappings may no longer refer to the same person.
pub async fn replace_meeting_transcripts(
    pool: &SqlitePool,
    meeting_id: &str,
    segments: &[SpeakerSegment],
) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| anyhow!("Failed to start transaction: {}", e))?;

    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to delete existing transcripts: {}", e))?;
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to reset speaker names: {}", e))?;

    for s in segments {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&s.id)
        .bind(meeting_id)
        .bind(&s.text)
        .bind(&s.timestamp)
        .bind(s.audio_start_time)
        .bind(s.audio_end_time)
        .bind(s.duration)
        .bind(&s.speaker)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to insert transcript: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| anyhow!("Failed to commit transaction: {}", e))?;
    Ok(())
}

/// Write transcripts.json (same shape as `audio::common::write_transcripts_json`, plus `speaker`).
pub fn write_transcripts_json(folder: &Path, segments: &[SpeakerSegment]) -> Result<()> {
    let transcript_path = folder.join("transcripts.json");
    let temp_path = folder.join(".transcripts.json.tmp");
    let json = serde_json::json!({
        "version": "1.0",
        "last_updated": chrono::Utc::now().to_rfc3339(),
        "total_segments": segments.len(),
        "engine": "vibevoice",
        "segments": segments.iter().enumerate().map(|(i, s)| serde_json::json!({
            "id": s.id,
            "text": s.text,
            "timestamp": s.timestamp,
            "speaker": s.speaker,
            "audio_start_time": s.audio_start_time,
            "audio_end_time": s.audio_end_time,
            "duration": s.duration,
            "sequence_id": i
        })).collect::<Vec<_>>()
    });
    std::fs::write(&temp_path, serde_json::to_string_pretty(&json)?)?;
    std::fs::rename(&temp_path, &transcript_path)?;
    log::debug!(
        "Wrote {} VibeVoice segments to transcripts.json",
        segments.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vibevoice::client::parse_transcribe_response;
    use crate::vibevoice::speakers;

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'T', '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn sample() -> TranscribeResponse {
        parse_transcribe_response(
            r#"{"segments":[
              {"speaker":"0","start":0,"end":2,"text":"hello"},
              {"speaker":"1","start":2,"end":5,"text":"hi there"},
              {"speaker":"0","start":5,"end":6,"text":"ok"}],"duration":6}"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn store_list_and_rename_speakers() {
        let pool = test_pool().await;
        let segs = build_segments(&sample());
        replace_meeting_transcripts(&pool, "m1", &segs)
            .await
            .unwrap();

        let list = speakers::list_speakers(&pool, "m1").await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].speaker_id, "0");
        assert_eq!(list[0].segment_count, 2);
        assert_eq!(list[0].display_name, "Speaker 0");
        assert!((list[0].total_seconds - 3.0).abs() < 1e-9);

        // Rename once -> applies to every segment of that speaker via the name map.
        speakers::rename_speaker(&pool, "m1", "0", Some(" Priya "))
            .await
            .unwrap();
        let names = speakers::load_names(&pool, "m1").await.unwrap();
        let (rows, _) = crate::database::repositories::meeting::MeetingsRepository::get_meeting_transcripts_paginated(&pool, "m1", 100, 0)
            .await
            .unwrap();
        let resolved: Vec<_> = rows
            .iter()
            .map(|t| speakers::resolve_name(t.speaker.as_deref(), &names).unwrap())
            .collect();
        assert_eq!(resolved, vec!["Priya", "Speaker 1", "Priya"]);

        // Reset to default.
        speakers::rename_speaker(&pool, "m1", "0", Some("  "))
            .await
            .unwrap();
        assert!(speakers::load_names(&pool, "m1").await.unwrap().is_empty());

        // Unknown speaker is rejected.
        assert!(speakers::rename_speaker(&pool, "m1", "9", Some("X"))
            .await
            .is_err());

        // Re-transcribing clears stale names.
        speakers::rename_speaker(&pool, "m1", "1", Some("Sam"))
            .await
            .unwrap();
        replace_meeting_transcripts(&pool, "m1", &segs[..1])
            .await
            .unwrap();
        assert!(speakers::load_names(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn legacy_meeting_without_speakers_still_loads() {
        let pool = test_pool().await;
        // Row shaped like one written before this feature (no speaker column value).
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
             VALUES ('t1', 'm1', 'old text', 'ts', 0.0, 1.0, 1.0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (rows, total) = crate::database::repositories::meeting::MeetingsRepository::get_meeting_transcripts_paginated(&pool, "m1", 10, 0)
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].transcript, "old text");
        assert_eq!(rows[0].speaker, None);
        let details =
            crate::database::repositories::meeting::MeetingsRepository::get_meeting(&pool, "m1")
                .await
                .unwrap()
                .unwrap();
        assert_eq!(details.transcripts.len(), 1);
        assert!(speakers::list_speakers(&pool, "m1")
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn transcripts_json_includes_speaker() {
        let dir = std::env::temp_dir().join(format!("vv-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let segs = build_segments(&sample());
        write_transcripts_json(&dir, &segs).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("transcripts.json")).unwrap())
                .unwrap();
        assert_eq!(v["total_segments"], 3);
        assert_eq!(v["segments"][1]["speaker"], "1");
        assert_eq!(v["segments"][1]["duration"], 3.0);
        std::fs::remove_dir_all(dir).ok();
    }
}
