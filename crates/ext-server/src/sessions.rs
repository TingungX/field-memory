use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::PathBuf;

use crate::AppState;

const SESSIONS_FILE: &str = "data/sessions.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memoryCtx: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub messages: Vec<StoredMessage>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionsData {
    #[serde(default)]
    pub sessions: Vec<StoredSession>,
    #[serde(default)]
    pub active_id: Option<String>,
}

fn data_path() -> PathBuf {
    let p = PathBuf::from(SESSIONS_FILE);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    p
}

/// Load sessions from JSON file.
pub fn load_sessions_from_disk() -> SessionsData {
    let path = data_path();
    if !path.exists() {
        return SessionsData { sessions: vec![], active_id: None };
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SessionsData { sessions: vec![], active_id: None },
    }
}

/// Save sessions to JSON file.
pub fn save_sessions_to_disk(data: &SessionsData) -> Result<(), String> {
    let path = data_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let s = serde_json::to_string_pretty(data).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, &s).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// GET /api/sessions — load all sessions from disk
pub async fn load(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let data = load_sessions_from_disk();

    Json(serde_json::json!({
        "ok": true,
        "sessions": data.sessions,
        "active_id": data.active_id,
    }))
}

/// POST /api/sessions — save all sessions to disk
pub async fn save(
    State(_state): State<Arc<AppState>>,
    Json(data): Json<SessionsData>,
) -> Json<serde_json::Value> {
    match save_sessions_to_disk(&data) {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}
