//! Persisted settings for the VibeVoice sidecar (server URL + hotwords).

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8765";
pub const MAX_HOTWORDS: usize = 200;
pub const MAX_HOTWORD_LEN: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VibeVoiceSettings {
    pub server_url: String,
    pub hotwords: Vec<String>,
}

impl Default for VibeVoiceSettings {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            hotwords: Vec::new(),
        }
    }
}

/// Validate and normalise the sidecar URL. Only loopback hosts are allowed:
/// audio must never leave the machine.
pub fn normalize_server_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(DEFAULT_SERVER_URL.to_string());
    }
    let url = url::Url::parse(trimmed).map_err(|e| anyhow!("Invalid server URL: {}", e))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(anyhow!("Server URL must start with http:// or https://"));
    }
    let local = match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !local {
        return Err(anyhow!(
            "VibeVoice must run locally: use 127.0.0.1, localhost or [::1] as the host"
        ));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(anyhow!("Server URL must not contain a path"));
    }
    Ok(trimmed.to_string())
}

/// Trim, drop empties, de-duplicate (case-insensitive), cap count and length.
pub fn normalize_hotwords(raw: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.iter()
        .map(|h| h.trim().chars().take(MAX_HOTWORD_LEN).collect::<String>())
        .filter(|h| !h.is_empty() && seen.insert(h.to_lowercase()))
        .take(MAX_HOTWORDS)
        .collect()
}

pub async fn load(pool: &SqlitePool) -> Result<VibeVoiceSettings> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT server_url, hotwords FROM vibevoice_settings WHERE id = '1'")
            .fetch_optional(pool)
            .await?;
    Ok(match row {
        Some((server_url, hotwords)) => VibeVoiceSettings {
            server_url,
            hotwords: serde_json::from_str(&hotwords).unwrap_or_default(),
        },
        None => VibeVoiceSettings::default(),
    })
}

pub async fn save(pool: &SqlitePool, settings: &VibeVoiceSettings) -> Result<VibeVoiceSettings> {
    let normalized = VibeVoiceSettings {
        server_url: normalize_server_url(&settings.server_url)?,
        hotwords: normalize_hotwords(&settings.hotwords),
    };
    sqlx::query(
        "INSERT INTO vibevoice_settings (id, server_url, hotwords) VALUES ('1', ?, ?)
         ON CONFLICT(id) DO UPDATE SET server_url = excluded.server_url, hotwords = excluded.hotwords",
    )
    .bind(&normalized.server_url)
    .bind(serde_json::to_string(&normalized.hotwords)?)
    .execute(pool)
    .await?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation() {
        assert_eq!(normalize_server_url("").unwrap(), DEFAULT_SERVER_URL);
        assert_eq!(
            normalize_server_url(" http://127.0.0.1:9000/ ").unwrap(),
            "http://127.0.0.1:9000"
        );
        assert!(normalize_server_url("http://localhost:8765").is_ok());
        assert!(normalize_server_url("http://[::1]:8765").is_ok());
        assert!(normalize_server_url("http://127.1.2.3:8765").is_ok());
        assert!(normalize_server_url("http://192.168.1.10:8765").is_err());
        assert!(normalize_server_url("https://api.example.com").is_err());
        assert!(normalize_server_url("ftp://127.0.0.1").is_err());
        assert!(normalize_server_url("http://127.0.0.1:8765/v1").is_err());
        assert!(normalize_server_url("not a url").is_err());
    }

    #[test]
    fn hotword_normalization() {
        let raw = vec![
            " Acme ".into(),
            "acme".into(),
            "".into(),
            "Jira".into(),
            "x".repeat(150),
        ];
        let out = normalize_hotwords(&raw);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], "Acme");
        assert_eq!(out[1], "Jira");
        assert_eq!(out[2].len(), MAX_HOTWORD_LEN);
        let many: Vec<String> = (0..500).map(|i| i.to_string()).collect();
        assert_eq!(normalize_hotwords(&many).len(), MAX_HOTWORDS);
    }
}
