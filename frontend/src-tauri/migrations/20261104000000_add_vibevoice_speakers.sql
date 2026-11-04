-- Migration: VibeVoice-ASR speaker separation support
--
-- transcripts.speaker already exists (20251110000001_add_speaker_field.sql) but was never
-- written. VibeVoice re-transcription stores the model's speaker id there ("0", "1", or
-- "2-0" for chunked recordings). Rows from older meetings keep speaker = NULL and render
-- exactly as before.

-- Per-meeting display names for speaker ids ("0" -> "Priya").
CREATE TABLE IF NOT EXISTS meeting_speakers (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

-- Speeds up per-meeting speaker lookups.
CREATE INDEX IF NOT EXISTS idx_transcripts_meeting_speaker ON transcripts(meeting_id, speaker);

-- Single-row settings for the local VibeVoice sidecar.
CREATE TABLE IF NOT EXISTS vibevoice_settings (
    id TEXT PRIMARY KEY,
    server_url TEXT NOT NULL,
    hotwords TEXT NOT NULL DEFAULT '[]'
);
