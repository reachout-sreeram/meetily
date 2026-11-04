//! App-level VibeVoice flows using Tauri's mock runtime and a real on-disk database.
//!
//! * `old_database_upgrades_and_old_meetings_still_open` always runs.
//! * The two flow tests need a sidecar (fake model is fine) and are ignored by default:
//!     python vibevoice-server/scripts/dev_fake_server.py --port 18765 --delay 2 &
//!     VIBEVOICE_TEST_URL=http://127.0.0.1:18765 cargo test --test vibevoice_app_flow -- --ignored --test-threads=1

use app_lib::database::manager::DatabaseManager;
use app_lib::database::repositories::meeting::MeetingsRepository;
use app_lib::state::AppState;
use app_lib::vibevoice::{settings, speakers};
use sqlx::migrate::Migrator;
use std::borrow::Cow;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{Listener, Manager};

const NEW_MIGRATION: i64 = 20261104000000;

fn test_url() -> String {
    std::env::var("VIBEVOICE_TEST_URL").unwrap_or_else(|_| "http://127.0.0.1:18765".into())
}

fn make_audio(path: &Path, secs: u32) {
    let ok = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=300:duration={}", secs))
        .arg(path)
        .status()
        .expect("ffmpeg must be installed")
        .success();
    assert!(ok);
}

/// Create a DB at `path` with only the migrations that existed before this feature.
async fn create_pre_feature_db(path: &Path) {
    use sqlx::migrate::MigrateDatabase;
    let url = path.to_str().unwrap();
    sqlx::Sqlite::create_database(url).await.unwrap();
    let pool = sqlx::SqlitePool::connect(url).await.unwrap();
    let full = sqlx::migrate!("./migrations");
    let old: Vec<_> = full.migrations.iter().filter(|m| m.version < NEW_MIGRATION).cloned().collect();
    assert!(old.len() < full.migrations.len(), "new migration must exist");
    let mut migrator = Migrator::DEFAULT;
    migrator.migrations = Cow::Owned(old);
    migrator.run(&pool).await.unwrap();
    pool.close().await;
}

async fn app_with_db(db_path: &Path) -> tauri::App<tauri::test::MockRuntime> {
    let db = DatabaseManager::new(db_path.to_str().unwrap(), "/nonexistent/legacy.db")
        .await
        .unwrap();
    tauri::test::mock_builder()
        .manage(AppState { db_manager: db })
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap()
}

fn collect(app: &tauri::App<tauri::test::MockRuntime>, event: &str) -> Arc<Mutex<Vec<String>>> {
    let sink = Arc::new(Mutex::new(Vec::new()));
    let s = sink.clone();
    app.listen_any(event, move |e| s.lock().unwrap().push(e.payload().to_string()));
    sink
}

#[tokio::test]
async fn old_database_upgrades_and_old_meetings_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("meeting_minutes.sqlite");
    create_pre_feature_db(&db_path).await;
    {
        let pool = sqlx::SqlitePool::connect(db_path.to_str().unwrap()).await.unwrap();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('old', 'Old meeting', ?, ?)")
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
             VALUES ('t1', 'old', 'hello from before', '2025-01-01T00:00:00Z', 0.0, 2.0, 2.0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    // App start runs all migrations, including the new one.
    let db = DatabaseManager::new(db_path.to_str().unwrap(), "/nonexistent/legacy.db").await.unwrap();
    let pool = db.pool();
    let meeting = MeetingsRepository::get_meeting(pool, "old").await.unwrap().unwrap();
    assert_eq!(meeting.transcripts.len(), 1);
    assert_eq!(meeting.transcripts[0].text, "hello from before");
    assert_eq!(meeting.transcripts[0].speaker, None);
    assert_eq!(meeting.transcripts[0].speaker_name, None);
    let (rows, total) = MeetingsRepository::get_meeting_transcripts_paginated(pool, "old", 10, 0).await.unwrap();
    assert_eq!((rows.len(), total), (1, 1));
    assert!(speakers::list_speakers(pool, "old").await.unwrap().is_empty());
    assert_eq!(settings::load(pool).await.unwrap(), settings::VibeVoiceSettings::default());

    // Running migrations a second time (next app start) is a no-op.
    DatabaseManager::new(db_path.to_str().unwrap(), "/nonexistent/legacy.db").await.unwrap();
}

#[tokio::test]
#[ignore]
async fn enhance_existing_meeting_with_vibevoice() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_db(&dir.path().join("db.sqlite")).await;
    let pool = app.state::<AppState>().db_manager.pool().clone();
    settings::save(&pool, &settings::VibeVoiceSettings { server_url: test_url(), hotwords: vec!["Acme".into()] })
        .await
        .unwrap();

    // An existing Whisper-transcribed meeting with a recording on disk.
    let folder = dir.path().join("meeting");
    std::fs::create_dir_all(&folder).unwrap();
    make_audio(&folder.join("audio.mp4"), 30);
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES ('m1', 'Standup', ?, ?, ?)")
        .bind(now)
        .bind(now)
        .bind(folder.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES ('w1', 'm1', 'whisper text', 'ts', 0.0)")
        .execute(&pool)
        .await
        .unwrap();

    let progress = collect(&app, "retranscription-progress");
    let complete = collect(&app, "retranscription-complete");
    let errors = collect(&app, "retranscription-error");

    let result = app_lib::audio::retranscription::start_retranscription(
        app.handle().clone(),
        "m1".into(),
        folder.to_str().unwrap().into(),
        None,
        None,
        Some("vibevoice".into()),
    )
    .await
    .expect("retranscription");
    assert!(result.segments_count >= 2);
    assert!(errors.lock().unwrap().is_empty());
    assert_eq!(complete.lock().unwrap().len(), 1);
    let progress = progress.lock().unwrap();
    assert!(progress.iter().any(|p| p.contains("Checking VibeVoice server")));
    assert!(progress.iter().any(|p| p.contains("elapsed")), "status polling should report progress");
    assert!(progress.last().unwrap().contains("\"progress_percentage\":100"));

    // Whisper text replaced by speaker-labelled segments.
    let meeting = MeetingsRepository::get_meeting(&pool, "m1").await.unwrap().unwrap();
    assert!(meeting.transcripts.iter().all(|t| t.speaker.is_some()));
    assert!(!meeting.transcripts.iter().any(|t| t.text == "whisper text"));
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("transcripts.json")).unwrap()).unwrap();
    assert_eq!(json["engine"], "vibevoice");

    // Rename "Speaker 0" once -> every segment of that speaker shows the new name.
    speakers::rename_speaker(&pool, "m1", "0", Some("Priya")).await.unwrap();
    let meeting = MeetingsRepository::get_meeting(&pool, "m1").await.unwrap().unwrap();
    for t in &meeting.transcripts {
        if t.speaker.as_deref() == Some("0") {
            assert_eq!(t.speaker_name.as_deref(), Some("Priya"));
        } else {
            assert!(t.speaker_name.as_deref().unwrap().starts_with("Speaker "));
        }
    }
}

#[tokio::test]
#[ignore]
async fn import_with_vibevoice_and_server_down_message() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_db(&dir.path().join("db.sqlite")).await;
    let pool = app.state::<AppState>().db_manager.pool().clone();
    let src = dir.path().join("customer call.m4a");
    make_audio(&src, 25);

    // 1) Server not running: clear message, nothing created.
    settings::save(&pool, &settings::VibeVoiceSettings { server_url: "http://127.0.0.1:9".into(), hotwords: vec![] })
        .await
        .unwrap();
    let err = app_lib::audio::import::start_import(
        app.handle().clone(),
        src.to_str().unwrap().into(),
        "Customer call".into(),
        None,
        None,
        Some("vibevoice".into()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("not running") && err.contains("./start.sh"), "{}", err);
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings").fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 0);

    // 2) Server running: meeting created with speaker labels.
    settings::save(&pool, &settings::VibeVoiceSettings { server_url: test_url(), hotwords: vec![] })
        .await
        .unwrap();
    let res = app_lib::audio::import::start_import(
        app.handle().clone(),
        src.to_str().unwrap().into(),
        "Customer call".into(),
        None,
        None,
        Some("vibevoice".into()),
    )
    .await
    .expect("import");
    let meeting = MeetingsRepository::get_meeting(&pool, &res.meeting_id).await.unwrap().unwrap();
    assert_eq!(meeting.transcripts.len(), res.segments_count);
    let ids: std::collections::BTreeSet<_> = meeting.transcripts.iter().filter_map(|t| t.speaker.clone()).collect();
    assert!(ids.len() >= 2);

    // Clean up the imported meeting folder (created under the default recordings folder).
    let folder: (Option<String>,) = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(&res.meeting_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    if let Some(f) = folder.0 {
        std::fs::remove_dir_all(f).ok();
    }
}
