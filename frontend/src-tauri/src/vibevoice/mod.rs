//! VibeVoice-ASR provider (post-meeting only).
//!
//! Talks to the local `vibevoice-server/` sidecar over HTTP on 127.0.0.1 and
//! produces speaker-labelled transcript segments. It is used ONLY by
//! Import & Enhance (retranscription / import). The live transcription path
//! never touches this module.
//!
//! Kept self-contained so upstream Meetily merges stay easy. The only hooks
//! outside this folder are small dispatch branches in `audio/retranscription.rs`
//! and `audio/import.rs`, the `speaker` fields on transcript read models, and
//! command registration in `lib.rs`.

pub mod client;
pub mod commands;
pub mod settings;
pub mod speakers;
pub mod store;
pub mod transcribe;

/// Provider id used by the frontend model selector and the retranscription/import commands.
pub const PROVIDER_ID: &str = "vibevoice";

/// Returns true if the given provider string selects VibeVoice.
pub fn is_vibevoice_provider(provider: Option<&str>) -> bool {
    provider == Some(PROVIDER_ID)
}

pub use transcribe::{ensure_server_ready, transcribe_file, VibeVoiceOutput};
