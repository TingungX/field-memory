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
    /// 2D projection of direction (first 2 components normalized) — for visualization
    direction_xy: [f32; 2],
}

#[derive(serde::Serialize)]
pub(crate) struct EventBrief {
    text: String,
    timestamp: String,
}

#[derive(serde::Serialize)]
pub(crate) struct SeedBrief {
    pub(crate) orthogonal_direction_0: f32,
    pub(crate) defeated_by: u64,
    pub(crate) pressure_accumulated: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct EcgBrief {
    field_tension: f32,
    convergence_rate: f32,
    anisotropy_magnitude: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct EcgSnapshotBrief {
    tension: f32,
    convergence_rate: f32,
    anchor_count: usize,
    event_inflow: usize,
    timestamp: String,
}

#[derive(serde::Serialize)]
pub(crate) struct MemoryStatus {
    // ── Anchors (right panel) ──
    anchors: Vec<AnchorBrief>,
    anchors_count: usize,

    // ── Events (right panel) ──
    events_count: usize,
    recent_events: Vec<EventBrief>,

    // ── Traces + Seeds (right panel) ──
    traces_count: usize,
    seeds: Vec<SeedBrief>,
    seeds_count: usize,

    // ── ECG (both panels) ──
    ecg: Option<EcgBrief>,
    ecg_snapshots: usize,
    ecg_history: Vec<EcgSnapshotBrief>,

    // ── Engine params (left panel) ──
    vector_dim: usize,
    event_window_secs: u64,
    damping_base: f32,
    stiffness_base: f32,
    convergence_threshold: f32,

    // ── Cycle state (left panel) ──
    cycle_window_secs: i64,
    impact_trace_threshold: f32,

    // ── Recent activity (left panel) ──
    recent_activity: Vec<serde_json::Value>,
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

#[derive(serde::Deserialize)]
pub(crate) struct SeedRequest {
    concepts: Vec<InitConcept>,
    /// Number of synthetic events per anchor (default 8)
    events_per_anchor: Option<usize>,
    /// Relaxation cycles to run after seeding (default 3)
    relax_cycles: Option<usize>,
}

#[derive(serde::Serialize)]
pub(crate) struct SeedResponse {
    anchors_count: usize,
    events_count: usize,
    seeds_count: usize,
    total_traces: usize,
    field_tension: f32,
}

#[derive(serde::Deserialize)]
pub(crate) struct QueryRequest {
    query: String,
    mode: Option<String>, // "recall", "associate", or "both" (default)
    top_k: Option<usize>,
}

#[derive(serde::Serialize)]
pub(crate) struct QueryResponse {
    query: String,
    mode: String,
    associated_anchors: Vec<serde_json::Value>,
    recalled_events: Vec<serde_json::Value>,
    anchors_count: usize,
    events_count: usize,
}

// ── Handlers ──

/// GET /api/memory/ping
pub async fn ping(State(_state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

/// GET /api/memory/status — full engine deep status for both panels
pub async fn status(State(state): State<Arc<AppState>>) -> Json<MemoryStatus> {
    let engine = state.engine.lock().unwrap();

    // Anchors
    let anchors: Vec<AnchorBrief> = engine
        .anchors
        .iter()
        .map(|a| {
            // First 2 components of direction as 2D projection for visualization
            let direction_xy = [
                a.direction.first().copied().unwrap_or(0.0),
                a.direction.get(1).copied().unwrap_or(0.0),
            ];
            AnchorBrief {
                label: a.label.clone(),
                density: a.density,
                stiffness: a.stiffness,
                damping: a.damping,
                direction_xy,
            }
        })
        .collect();
    let anchors_count = engine.anchors.len();

    // Events
    let events_count = engine.events.len();
    let recent_events: Vec<EventBrief> = engine
        .events
        .iter()
        .rev()
        .take(10)
        .map(|e| EventBrief {
            text: e.text.clone(),
            timestamp: e.timestamp.to_rfc3339(),
        })
        .collect();

    // Traces
    let traces_count = engine.traces.len();

    // Seeds
    let seeds_count = engine.seeds.len();
    let seeds: Vec<SeedBrief> = engine
        .seeds
        .iter()
        .map(|s| SeedBrief {
            orthogonal_direction_0: s.orthogonal_direction.first().copied().unwrap_or(0.0),
            defeated_by: s.defeated_by.0,
            pressure_accumulated: s.pressure_accumulated,
        })
        .collect();

    // ECG
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
    let ecg_snapshots = engine.ecg.snapshots.len();
    let ecg_history: Vec<EcgSnapshotBrief> = engine
        .ecg
        .snapshots
        .iter()
        .rev()
        .take(20)
        .map(|s| EcgSnapshotBrief {
            tension: s.tension,
            convergence_rate: s.convergence_rate,
            anchor_count: s.anchor_count,
            event_inflow: s.event_inflow,
            timestamp: s.timestamp.to_rfc3339(),
        })
        .collect();

    // Engine params
    let vector_dim = engine.params.vector_dim;
    let event_window_secs = engine.params.event_window_secs;
    let damping_base = engine.params.damping_base;
    let stiffness_base = engine.params.stiffness_base;
    let convergence_threshold = engine.params.convergence_threshold;

    // Cycle state
    let cycle_window_secs = engine.cycle.window.num_seconds();
    let impact_trace_threshold = engine.cycle.impact_trace_threshold;

    // Recent activity
    let recent_activity: Vec<serde_json::Value> = engine
        .recent_activity
        .iter()
        .rev()
        .take(25)
        .map(|a| serde_json::json!({
            "kind": format!("{:?}", a.kind),
            "detail": a.detail,
            "timestamp": a.timestamp.to_rfc3339(),
        }))
        .collect();

    Json(MemoryStatus {
        anchors,
        anchors_count,
        events_count,
        recent_events,
        traces_count,
        seeds,
        seeds_count,
        ecg,
        ecg_snapshots,
        ecg_history,
        vector_dim,
        event_window_secs,
        damping_base,
        stiffness_base,
        convergence_threshold,
        cycle_window_secs,
        impact_trace_threshold,
        recent_activity,
    })
}

// ── Library management ──

fn library_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("./libraries/{}", name))
}

fn ensure_library_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("library name must be 1-64 chars".into());
    }
    if name.chars().any(|c| c.is_whitespace() || c == '/' || c == '.' || c == '\\') {
        return Err("library name cannot contain whitespace or . / \\".into());
    }
    Ok(())
}

/// GET /api/memory/libraries — list all known libraries
pub async fn list_libraries(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let libs = state.libraries.lock().unwrap();
    Json(serde_json::json!({
        "libraries": libs.iter().map(|(name, count)| {
            serde_json::json!({
                "name": name,
                "anchors_count": count.0,
                "events_count": count.1,
            })
        }).collect::<Vec<_>>(),
        "active": *state.active_library.lock().unwrap(),
    }))
}

#[derive(serde::Deserialize)]
pub(crate) struct LibraryRequest {
    name: String,
}

/// POST /api/memory/library/save — save current state as named library
pub async fn library_save(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LibraryRequest>,
) -> Json<serde_json::Value> {
    if let Err(e) = ensure_library_name(&req.name) {
        return Json(serde_json::json!({"ok": false, "error": e}));
    }
    let engine = state.engine.lock().unwrap();
    let path = library_path(&req.name);
    match engine.save(&path) {
        Ok(_) => {
            let a = engine.anchors.len();
            let e = engine.events.len();
            state.libraries.lock().unwrap().insert(req.name.clone(), (a, e));
            Json(serde_json::json!({"ok": true, "name": req.name, "anchors_count": a, "events_count": e}))
        }
        Err(e) => Json(serde_json::json!({"ok": false, "error": format!("save failed: {e}")})),
    }
}

/// POST /api/memory/library/load — load named library as current
pub async fn library_load(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LibraryRequest>,
) -> Json<serde_json::Value> {
    if let Err(e) = ensure_library_name(&req.name) {
        return Json(serde_json::json!({"ok": false, "error": e}));
    }
    let path = library_path(&req.name);
    let mut engine = state.engine.lock().unwrap();
    match engine.load(&path) {
        Ok(_) => {
            *state.active_library.lock().unwrap() = req.name.clone();
            let a = engine.anchors.len();
            let e = engine.events.len();
            // Update library registry
            state.libraries.lock().unwrap().insert(req.name.clone(), (a, e));
            Json(serde_json::json!({"ok": true, "name": req.name, "anchors_count": a, "events_count": e}))
        }
        Err(e) => Json(serde_json::json!({"ok": false, "error": format!("load failed: {e}")})),
    }
}

/// DELETE /api/memory/library — delete a named library
pub async fn library_delete(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LibraryRequest>,
) -> Json<serde_json::Value> {
    if let Err(e) = ensure_library_name(&req.name) {
        return Json(serde_json::json!({"ok": false, "error": e}));
    }
    if *state.active_library.lock().unwrap() == req.name {
        return Json(serde_json::json!({"ok": false, "error": "cannot delete active library"}));
    }
    let path = library_path(&req.name);
    let _ = std::fs::remove_dir_all(&path);
    state.libraries.lock().unwrap().remove(&req.name);
    Json(serde_json::json!({"ok": true, "name": req.name}))
}


pub async fn query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let engine = state.engine.lock().unwrap();
    let q = req.query.as_str();
    let top_k = req.top_k.unwrap_or(5).min(10);
    let mode = req.mode.as_deref().unwrap_or("both");

    let associated_anchors = if mode == "associate" || mode == "both" {
        let assoc = engine.associate(q);
        assoc.iter().take(top_k).map(|(a, imp)| {
            serde_json::json!({
                "label": a.label,
                "density": a.density,
                "impact": format!("{:.3}", imp),
            })
        }).collect()
    } else {
        vec![]
    };

    let recalled_events = if mode == "recall" || mode == "both" {
        let recall = engine.recall(q, top_k);
        recall.events.iter().take(top_k).map(|(text, anchor, _imp)| {
            serde_json::json!({
                "text": text,
                "anchor": anchor,
            })
        }).collect()
    } else {
        vec![]
    };

    Json(QueryResponse {
        query: req.query,
        mode: mode.to_string(),
        anchors_count: associated_anchors.len(),
        events_count: recalled_events.len(),
        associated_anchors,
        recalled_events,
    })
}

/// POST /api/memory/save
pub async fn save(State(state): State<Arc<AppState>>) -> Json<SaveResponse> {
    let mut engine = state.engine.lock().unwrap();
    let result = engine.save(Path::new("./memory_state"));
    engine.log_save();
    match result {
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

/// POST /api/memory/seed — large-scale memory seeding
///
/// 1. Create anchors from seed concepts
/// 2. Generate synthetic events from concept variations
/// 3. Run multiple relaxation cycles
/// 4. Return final field state
pub async fn seed(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SeedRequest>,
) -> Json<SeedResponse> {
    let events_per = req.events_per_anchor.unwrap_or(8).max(1);
    let cycles = req.relax_cycles.unwrap_or(3).max(1);

    // 1. Create anchors
    let concepts: Vec<(&str, u32)> = req
        .concepts
        .iter()
        .map(|c| (c.label.as_str(), c.density))
        .collect();
    {
        let mut engine = state.engine.lock().unwrap();
        engine.init(&concepts);
    }

    // 2. Generate and inject synthetic events
    {
        let eng = state.engine.clone();
        let mut engine = eng.lock().unwrap();
        let modifiers = [
            "擅长", "不喜欢", "需要改进", "重点关注", "积累经验",
            "讨论过", "遇到的问题", "学到的教训",
        ];

        for concept in &req.concepts {
            for i in 0..events_per {
                let mod_idx = i.min(modifiers.len() - 1);
                let event_text = if mod_idx == 0 {
                    format!("{}: 这是最核心的原则", concept.label)
                } else {
                    format!("{}: {} 相关的讨论和记录", modifiers[mod_idx], concept.label)
                };
                engine.on_user_input(&event_text);

                if i % 3 == 0 {
                    engine.on_user_input(&format!("关于{}的补充思考第{}条", concept.label, i + 1));
                }
            }
        }
    }

    // 3. Run relaxation cycles
    {
        let eng = state.engine.clone();
        let mut engine = eng.lock().unwrap();
        for _ in 0..cycles {
            engine.relax();
        }
    }

    // 4. Return result
    let engine = state.engine.lock().unwrap();
    let events_count = engine.events.len();
    let anchors_count = engine.anchors.len();
    let seeds_count = engine.seeds.len();
    let traces_count = engine.traces.len();
    let tension = engine.ecg_report()
        .map(|r| r.current.tension)
        .unwrap_or(0.0);

    Json(SeedResponse {
        anchors_count,
        events_count,
        seeds_count,
        total_traces: traces_count,
        field_tension: tension,
    })
}
