use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    /// Reasoning trace text from the model (if any). Always optional so old
    /// session files lacking the field still deserialize — JSON ignores
    /// missing fields when `#[serde(default)]` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Accumulated tool calls during streaming (if any). Mirrors OpenAI's
    /// shape: array of {id, type, function: {name, arguments}}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolCalls: Option<serde_json::Value>,
    /// Thinking chain: ordered sequence of reasoning/tool_call steps.
    /// Stored as an array of {type, content} where type ∈ "reasoning" | "tool_call".
    /// Optional so old session files still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinkingChain: Option<serde_json::Value>,
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

/// Cold start: load sessions from JSON file (called once at server startup).
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

/// Atomic save: write to tmp file → fsync → rename.
/// The old file stays intact until rename completes, so partial writes can never
/// corrupt sessions.json on crash.
pub fn save_sessions_to_disk(data: &SessionsData) -> Result<(), String> {
    use std::io::Write;
    let path = data_path();
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(data).map_err(|e| format!("serialize: {e}"))?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("open tmp: {e}"))?;
        f.write_all(s.as_bytes()).map_err(|e| format!("write tmp: {e}"))?;
        f.sync_all().map_err(|e| format!("fsync tmp: {e}"))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Build the shared sessions state for AppState.
pub fn shared() -> Arc<Mutex<SessionsData>> {
    Arc::new(Mutex::new(load_sessions_from_disk()))
}

/// Mutation helper: lock → mutate → atomic save.
/// `f` returns Err to abort (no save). Used by every write endpoint so no
/// mutation path can skip the atomic save step.
pub fn mutate<F, R>(state: &Arc<Mutex<SessionsData>>, f: F) -> Result<R, String>
where
    F: FnOnce(&mut SessionsData) -> Result<R, String>,
{
    let mut data = state.lock().map_err(|e| format!("lock poisoned: {e}"))?;
    let result = f(&mut data)?;
    save_sessions_to_disk(&data)?;
    Ok(result)
}

// ─── Request types ───

#[derive(Debug, Default, Deserialize)]
pub struct CreateSessionReq {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PatchSessionReq {
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppendMessageReq {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub memoryCtx: Option<serde_json::Value>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub toolCalls: Option<serde_json::Value>,
    #[serde(default)]
    pub thinkingChain: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMessageReq {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub memoryCtx: Option<serde_json::Value>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub toolCalls: Option<serde_json::Value>,
    #[serde(default)]
    pub thinkingChain: Option<serde_json::Value>,
}

// ─── Endpoints ───

/// GET /api/sessions — list all sessions (from in-memory state).
pub async fn list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let data = state.sessions.lock().unwrap();
    Json(serde_json::json!({
        "ok": true,
        "sessions": data.sessions,
        "active_id": data.active_id,
    }))
}

/// POST /api/sessions — create a new session.
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionReq>,
) -> Json<serde_json::Value> {
    let title = req.title.unwrap_or_else(|| "新会话".to_string());
    let result = mutate(&state.sessions, |data| {
        let id = format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| format!("clock: {e}"))?
                .as_millis()
        );
        let s = StoredSession { id: id.clone(), title, messages: vec![] };
        data.sessions.push(s.clone());
        Ok(s)
    });
    match result {
        Ok(s) => Json(serde_json::json!({"ok": true, "session": s})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// PATCH /api/sessions/:id — set active flag and/or rename.
/// Multi-device semantics: only the device that opened a session updates
/// active_id; other devices keep their own active locally.
pub async fn patch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchSessionReq>,
) -> Json<serde_json::Value> {
    let result = mutate(&state.sessions, |data| {
        let Some(i) = data.sessions.iter().position(|s| s.id == id) else {
            return Err("session not found".to_string());
        };
        if let Some(t) = req.title.clone() {
            data.sessions[i].title = t;
        }
        if req.active == Some(true) {
            data.active_id = Some(id.clone());
        }
        Ok(data.sessions[i].clone())
    });
    match result {
        Ok(s) => Json(serde_json::json!({"ok": true, "session": s})),
        Err(e) if e == "session not found" =>
            Json(serde_json::json!({"ok": false, "error": e})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// DELETE /api/sessions/:id — remove a session.
/// If the deleted session was active, clear active_id.
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let result = mutate(&state.sessions, |data| {
        let before = data.sessions.len();
        data.sessions.retain(|s| s.id != id);
        if data.sessions.len() == before {
            return Err("session not found".to_string());
        }
        if data.active_id.as_deref() == Some(id.as_str()) {
            data.active_id = None;
        }
        Ok(())
    });
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// POST /api/sessions/:id/messages — append a message.
/// Returns the new message index so the client can PATCH it later (streaming finish).
pub async fn append_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AppendMessageReq>,
) -> Json<serde_json::Value> {
    let time = req.time.unwrap_or_else(local_hh_mm);
    let result = mutate(&state.sessions, |data| {
        let Some(s) = data.sessions.iter_mut().find(|s| s.id == id) else {
            return Err("session not found".to_string());
        };
let msg = StoredMessage {
            role: req.role,
            content: req.content,
            time,
            memoryCtx: req.memoryCtx,
            reasoning: req.reasoning,
            toolCalls: req.toolCalls,
            thinkingChain: req.thinkingChain,
        };
        s.messages.push(msg);
        Ok(s.messages.len() - 1)
    });
    match result {
        Ok(idx) => Json(serde_json::json!({"ok": true, "idx": idx})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// PATCH /api/sessions/:id/messages/:idx — update message content (stream-finish).
pub async fn update_message(
    State(state): State<Arc<AppState>>,
    Path((id, idx)): Path<(String, usize)>,
    Json(req): Json<UpdateMessageReq>,
) -> Json<serde_json::Value> {
    let result = mutate(&state.sessions, |data| {
        let Some(s) = data.sessions.iter_mut().find(|s| s.id == id) else {
            return Err("session not found".to_string());
        };
        if idx >= s.messages.len() {
            return Err(format!("message index {} out of range (len {})", idx, s.messages.len()));
        }
        let m = &mut s.messages[idx];
if let Some(c) = req.content {
            m.content = c;
        }
        if req.memoryCtx.is_some() {
            m.memoryCtx = req.memoryCtx;
        }
        if req.reasoning.is_some() {
            m.reasoning = req.reasoning;
        }
        if req.toolCalls.is_some() {
            m.toolCalls = req.toolCalls;
        }
        if req.thinkingChain.is_some() {
            m.thinkingChain = req.thinkingChain;
        }
        Ok(())
    });
match result {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// DELETE /api/sessions/:id/messages/:idx — drop a single message.
/// Used by the front-end's edit-and-resend flow: it PATCHes the edited user
/// message in place, then loops DELETE on every message after it to truncate
/// the assistant half before re-streaming. Each call removes exactly one
/// message so the server stays stateless about the front-end's intent.
pub async fn delete_message(
    State(state): State<Arc<AppState>>,
    Path((id, idx)): Path<(String, usize)>,
) -> Json<serde_json::Value> {
    let result = mutate(&state.sessions, |data| {
        let Some(s) = data.sessions.iter_mut().find(|s| s.id == id) else {
            return Err("session not found".to_string());
        };
        if idx >= s.messages.len() {
            return Err(format!(
                "message index {} out of range (len {})",
                idx,
                s.messages.len()
            ));
        }
        s.messages.remove(idx);
        Ok(())
    });
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

/// Local HH:MM in 24h, no chrono dep.
fn local_hh_mm() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let total_min = (secs / 60) % (24 * 60);
    let h = total_min / 60;
    let m = total_min % 60;
    format!("{:02}:{:02}", h, m)
}
