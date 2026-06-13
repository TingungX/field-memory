use axum::{Json, extract::State};
use std::path::Path;
use std::sync::Arc;

use crate::AppState;

// ── Response / Request structs ──

#[derive(serde::Serialize)]
pub(crate) struct AnchorBrief {
    label: String,
    density: u32,
    stiffness: f32,
    damping: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct EcgBrief {
    field_tension: f32,
    convergence_rate: f32,
    anisotropy_magnitude: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct MemoryStatus {
    anchors: Vec<AnchorBrief>,
    anchors_count: usize,
    events_count: usize,
    seeds_count: usize,
    ecg: Option<EcgBrief>,
}

#[derive(serde::Serialize)]
pub(crate) struct SaveResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct LoadResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    anchors_count: usize,
    events_count: usize,
    seeds_count: usize,
}

#[derive(serde::Deserialize)]
struct InitConcept {
    label: String,
    density: u32,
}

#[derive(serde::Deserialize)]
pub(crate) struct InitRequest {
    concepts: Vec<InitConcept>,
}

#[derive(serde::Serialize)]
pub(crate) struct InitResponse {
    anchors_count: usize,
}

// ── Handlers ──

/// GET /api/memory/ping
pub async fn ping(State(_state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

/// GET /api/memory/status
pub async fn status(State(state): State<Arc<AppState>>) -> Json<MemoryStatus> {
    let engine = state.engine.lock().unwrap();

    let anchors: Vec<AnchorBrief> = engine
        .anchors
        .iter()
        .map(|a| AnchorBrief {
            label: a.label.clone(),
            density: a.density,
            stiffness: a.stiffness,
            damping: a.damping,
        })
        .collect();

    let anchors_count = engine.anchors.len();
    let events_count = engine.events.len();
    let seeds_count = engine.seeds.len();

    let ecg = engine.ecg_report().map(|r| {
        let mag = r
            .current
            .anisotropies
            .first()
            .map(|a| a.magnitude)
            .unwrap_or(0.0);
        EcgBrief {
            field_tension: r.current.tension,
            convergence_rate: r.current.convergence_rate,
            anisotropy_magnitude: mag,
        }
    });

    Json(MemoryStatus {
        anchors,
        anchors_count,
        events_count,
        seeds_count,
        ecg,
    })
}

/// POST /api/memory/save
pub async fn save(State(state): State<Arc<AppState>>) -> Json<SaveResponse> {
    let engine = state.engine.lock().unwrap();
    match engine.save(Path::new("./memory_state")) {
        Ok(_) => Json(SaveResponse { ok: true, error: None }),
        Err(e) => Json(SaveResponse { ok: false, error: Some(format!("读写失败: {e}")) }),
    }
}

/// POST /api/memory/load
pub async fn load(State(state): State<Arc<AppState>>) -> Json<LoadResponse> {
    let mut engine = state.engine.lock().unwrap();
    match engine.load(Path::new("./memory_state")) {
        Ok(_) => Json(LoadResponse {
            ok: true,
            error: None,
            anchors_count: engine.anchors.len(),
            events_count: engine.events.len(),
            seeds_count: engine.seeds.len(),
        }),
        Err(e) => Json(LoadResponse {
            ok: false,
            error: Some(format!("{e}")),
            anchors_count: 0,
            events_count: 0,
            seeds_count: 0,
        }),
    }
}

/// POST /api/memory/init
pub async fn init(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InitRequest>,
) -> Json<InitResponse> {
    let concepts: Vec<(&str, u32)> = req
        .concepts
        .iter()
        .map(|c| (c.label.as_str(), c.density))
        .collect();
    let mut engine = state.engine.lock().unwrap();
    engine.init(&concepts);
    Json(InitResponse {
        anchors_count: engine.anchors.len(),
    })
}
