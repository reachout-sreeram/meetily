//! End-to-end check of the Rust VibeVoice client against a running sidecar.
//!
//! Ignored by default. Run against the fake-model server (any OS):
//!   python vibevoice-server/scripts/dev_fake_server.py --port 18765 &
//!   VIBEVOICE_TEST_URL=http://127.0.0.1:18765 cargo test --test vibevoice_sidecar_integration -- --ignored
//! or against the real MLX server on a Mac (expect real speaker ids instead of 0/1/2).

use app_lib::vibevoice::client::VibeVoiceClient;
use app_lib::vibevoice::{speakers, store};
use std::time::Duration;

fn test_url() -> String {
    std::env::var("VIBEVOICE_TEST_URL").unwrap_or_else(|_| "http://127.0.0.1:18765".into())
}

fn make_audio(dir: &std::path::Path, secs: u32) -> std::path::PathBuf {
    let path = dir.join("meeting.m4a");
    let status = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=300:duration={}", secs))
        .args(["-ac", "2", "-ar", "48000"])
        .arg(&path)
        .status()
        .expect("ffmpeg must be installed");
    assert!(status.success());
    path
}

#[tokio::test]
#[ignore]
async fn sidecar_round_trip_with_speakers_and_renames() {
    let client = VibeVoiceClient::new(&test_url()).unwrap();
    let health = client.health().await.expect("sidecar must be running");
    assert_eq!(health.status, "ok");
    assert!(!health.busy);

    let dir = std::env::temp_dir().join(format!("vv-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let audio = make_audio(&dir, 20);

    // Poll /status while the long request runs, like the app does.
    let poller = {
        let poll_client = VibeVoiceClient::new(&test_url()).unwrap();
        tokio::spawn(async move {
            let mut seen = Vec::new();
            for _ in 0..20 {
                if let Ok(st) = poll_client.status().await {
                    if st.active {
                        seen.push(st.stage);
                    }
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            seen
        })
    };
    let resp = client
        .transcribe(audio.to_str().unwrap(), &["Acme".to_string()])
        .await
        .expect("transcribe");
    let stages = poller.await.unwrap();
    assert!(!stages.is_empty(), "status polling should observe an active job");

    assert!((resp.duration - 20.0).abs() < 0.5, "duration {}", resp.duration);
    assert!(!resp.segments.is_empty());
    let speaker_ids: std::collections::BTreeSet<_> = resp.segments.iter().map(|s| s.speaker.clone()).collect();
    assert!(speaker_ids.len() >= 2, "expected multiple speakers, got {:?}", speaker_ids);

    // Store, rename, resolve: the same path the app uses after retranscription.
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 't', '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    let segs = store::build_segments(&resp);
    store::replace_meeting_transcripts(&pool, "m", &segs).await.unwrap();
    let first = segs[0].speaker.clone();
    speakers::rename_speaker(&pool, "m", &first, Some("Priya")).await.unwrap();
    let names = speakers::load_names(&pool, "m").await.unwrap();
    let renamed = segs
        .iter()
        .filter(|s| s.speaker == first)
        .all(|s| speakers::resolve_name(Some(&s.speaker), &names).as_deref() == Some("Priya"));
    assert!(renamed);

    // Bad input surfaces as a clear error, not a panic.
    let bad = dir.join("bad.wav");
    std::fs::write(&bad, b"not audio").unwrap();
    let err = client.transcribe(bad.to_str().unwrap(), &[]).await.unwrap_err();
    assert!(err.to_string().contains("Could not read audio"), "{}", err);

    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn not_running_server_gives_start_instructions() {
    // Port 9 (discard) is essentially never listening on localhost.
    let client = VibeVoiceClient::new("http://127.0.0.1:9").unwrap();
    let err = client.health().await.unwrap_err().to_string();
    assert!(err.contains("not running"), "{}", err);
    assert!(err.contains("./start.sh"), "{}", err);
}
