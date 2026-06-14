use axum::{Json, extract::State};
use std::path::Path;
use std::sync::Arc;

use field_mem_core::DseEngine;
use crate::AppState;

// Use eprintln! for logging since tracing is not available in this crate
macro_rules! log_info {
    ($($arg:tt)*) => { eprintln!("[INFO] {}", format!($($arg)*)); };
}
macro_rules! log_error {
    ($($arg:tt)*) => { eprintln!("[ERROR] {}", format!($($arg)*)); };
}

// ── Response / Request structs ──

#[derive(serde::Serialize)]
pub(crate) struct AnchorBrief {
    label: String,
    /// Stable anchor id (hash of direction at creation) — for client references
    id: String,
    density: u32,
    stiffness: f32,
    damping: f32,
    /// 2D projection of direction (first 2 components) — for 2D inline viz
    direction_xy: [f32; 2],
    /// Full direction vector (32-dim unit) — for 3D / PCA projection on the client
    direction_n: Vec<f32>,
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

#[derive(Clone, serde::Deserialize)]
struct InitConcept {
    label: String,
    density: u32,
}

#[derive(Clone, serde::Deserialize)]
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
    // Hold the engine lock only long enough to clone data out.
    // The 1.5s polling cycle means we must release the lock ASAP so that
    // library_create / library_load / other mutations can acquire it.
    let (anchors, anchors_count, events_count, recent_events, traces_count,
         seeds_count, seeds, ecg, ecg_snapshots, ecg_history,
         vector_dim, event_window_secs, damping_base, stiffness_base,
         convergence_threshold, cycle_window_secs, impact_trace_threshold,
         recent_activity) = {
        let engine = state.engine.lock().unwrap();

        let anchors: Vec<AnchorBrief> = engine
            .anchors
            .iter()
            .map(|a| {
                let direction_xy = [
                    a.direction.first().copied().unwrap_or(0.0),
                    a.direction.get(1).copied().unwrap_or(0.0),
                ];
                AnchorBrief {
                    id: format!("a{}", a.id.0),
                    label: a.label.clone(),
                    density: a.density,
                    stiffness: a.stiffness,
                    damping: a.damping,
                    direction_xy,
                    direction_n: a.direction.clone(),
                }
            })
            .collect();
        let anchors_count = engine.anchors.len();

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

        let traces_count = engine.traces.len();

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

        let vector_dim = engine.params.vector_dim;
        let event_window_secs = engine.params.event_window_secs;
        let damping_base = engine.params.damping_base;
        let stiffness_base = engine.params.stiffness_base;
        let convergence_threshold = engine.params.convergence_threshold;

        let cycle_window_secs = engine.cycle.window.num_seconds();
        let impact_trace_threshold = engine.cycle.impact_trace_threshold;

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

        // Lock released here when this block ends
        (anchors, anchors_count, events_count, recent_events, traces_count,
         seeds_count, seeds, ecg, ecg_snapshots, ecg_history,
         vector_dim, event_window_secs, damping_base, stiffness_base,
         convergence_threshold, cycle_window_secs, impact_trace_threshold,
         recent_activity)
    };

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
    log_info!("library_load: path={:?}", path);
    let mut engine = state.engine.lock().unwrap();
    match engine.load(&path) {
        Ok(_) => {
            *state.active_library.lock().unwrap() = req.name.clone();
            let a = engine.anchors.len();
            let e = engine.events.len();
            // Update library registry
            state.libraries.lock().unwrap().insert(req.name.clone(), (a, e));
            log_info!("library_load: success name={} anchors={} events={}", req.name, a, e);
            Json(serde_json::json!({"ok": true, "name": req.name, "anchors_count": a, "events_count": e}))
        }
        Err(e) => {
            log_error!("library_load: failed name={} error={}", req.name, e);
            Json(serde_json::json!({"ok": false, "error": format!("load failed: {e}")}))
        }
    }
}

/// POST /api/memory/library/create — create a new empty library and switch to it
pub async fn library_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LibraryRequest>,
) -> Json<serde_json::Value> {
    if let Err(e) = ensure_library_name(&req.name) {
        return Json(serde_json::json!({"ok": false, "error": e}));
    }
    // Check name not already taken
    if state.libraries.lock().unwrap().contains_key(&req.name) {
        return Json(serde_json::json!({"ok": false, "error": format!("记忆库 '{}' 已存在", req.name)}));
    }

    // Phase 1: under a single engine lock, auto-save current state and
    // extract params for the new engine.  This avoids acquiring engine lock
    // multiple times (which causes contention with the 1.5s status poll).
    let params;
    let current_counts: (usize, usize);
    {
        let engine = state.engine.lock().unwrap();
        params = engine.params.clone();
        current_counts = (engine.anchors.len(), engine.events.len());
        // Auto-save current engine state before switching (best-effort)
        let current = state.active_library.lock().unwrap().clone();
        let path = library_path(&current);
        let _ = engine.save(&path);
    }
    // Update library registry outside the engine lock
    {
        let current = state.active_library.lock().unwrap().clone();
        state.libraries.lock().unwrap().insert(current, current_counts);
    }

    // Phase 2: create new engine and save it to disk (no lock needed)
    let new_engine = DseEngine::with_embed(params, state.embed_config.new_boxed());
    let path = library_path(&req.name);
    let _ = new_engine.save(&path);

    // Phase 3: swap in the new engine under a single short lock
    {
        let mut engine = state.engine.lock().unwrap();
        *engine = new_engine;
    }

    *state.active_library.lock().unwrap() = req.name.clone();
    state.libraries.lock().unwrap().insert(req.name.clone(), (0, 0));

    log_info!("library_create: created name={}", req.name);
    Json(serde_json::json!({"ok": true, "name": req.name, "anchors_count": 0, "events_count": 0}))
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
    let q = req.query.clone();
    let top_k = req.top_k.unwrap_or(5).min(10);
    let mode = req.mode.clone().unwrap_or_else(|| "both".into());
    let engine_arc = state.engine.clone();

    // embed() is blocking HTTP (ollama) — run in spawn_blocking
    let result = tokio::task::spawn_blocking(move || {
        let engine = engine_arc.lock().unwrap();

        let associated_anchors = if mode == "associate" || mode == "both" {
            let assoc = engine.associate(&q);
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
            let recall = engine.recall(&q, top_k);
            recall.events.iter().take(top_k).map(|(text, anchor, _imp)| {
                serde_json::json!({
                    "text": text,
                    "anchor": anchor,
                })
            }).collect()
        } else {
            vec![]
        };

        QueryResponse {
            query: q,
            mode,
            anchors_count: associated_anchors.len(),
            events_count: recalled_events.len(),
            associated_anchors,
            recalled_events,
        }
    }).await;

    match result {
        Ok(resp) => Json(resp),
        Err(e) => {
            log_error!("query: spawn_blocking panicked: {}", e);
            Json(QueryResponse {
                query: req.query,
                mode: "error".into(),
                anchors_count: 0,
                events_count: 0,
                associated_anchors: vec![],
                recalled_events: vec![],
            })
        }
    }
}

/// POST /api/memory/save
pub async fn save(State(state): State<Arc<AppState>>) -> Json<SaveResponse> {
    let mut engine = state.engine.lock().unwrap();
    let result = engine.save(Path::new("./memory_state"));
    engine.log_save();
    match result {
        Ok(_) => Json(SaveResponse { ok: true, error: None }),
        Err(e) => {
            log_error!("save: failed path=./memory_state error={}", e);
            Json(SaveResponse { ok: false, error: Some(format!("保存失败: {e}")) })
        }
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
        Err(e) => {
            log_error!("load: failed path=./memory_state error={}", e);
            Json(LoadResponse {
                ok: false,
                error: Some(format!("读取失败: {e}")),
                anchors_count: 0,
                events_count: 0,
                seeds_count: 0,
            })
        }
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

    // Clone concepts + engine + library path for spawn_blocking
    let concepts = req.concepts.clone();
    let engine_arc = state.engine.clone();
    let library_path = std::path::PathBuf::from(format!("./libraries/{}", state.active_library.lock().unwrap()));

    let save_path = library_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let crefs: Vec<(&str, u32)> = concepts.iter().map(|c| (c.label.as_str(), c.density)).collect();

        // 1. Create anchors
        {
            let mut engine = engine_arc.lock().unwrap();
            engine.init(&crefs);
        }

        // 2. Generate and inject synthetic events
        {
            let mut engine = engine_arc.lock().unwrap();
            let modifiers = [
                "擅长", "不喜欢", "需要改进", "重点关注", "积累经验",
                "讨论过", "遇到的问题", "学到的教训",
            ];

            for (label, _) in &crefs {
                for i in 0..events_per {
                    let mod_idx = i.min(modifiers.len() - 1);
                    let event_text = if mod_idx == 0 {
                        format!("{}: 这是最核心的原则", label)
                    } else {
                        format!("{}: {} 相关的讨论和记录", modifiers[mod_idx], label)
                    };
                    engine.on_user_input(&event_text);

                    if i % 3 == 0 {
                        engine.on_user_input(&format!("关于{}的补充思考第{}条", label, i + 1));
                    }
                }
            }
        }

        // 3. Run relaxation cycles
        {
            let mut engine = engine_arc.lock().unwrap();
            for _ in 0..cycles {
                engine.relax();
            }
        }

        // 4. Persist to disk so data survives restart
        //    (the engine's sled DB is at ./libraries/<active_library>).
        //    We can't access active_library here, but we can save to the
        //    library path via the persist module on the engine itself.
        //    The caller (main seed fn) already knows the active library
        //    name; we log it here as best-effort.
        {
            let engine = engine_arc.lock().unwrap();
            let _ = engine.save(&save_path);
        }

        // 5. Collect result
        let engine = engine_arc.lock().unwrap();
        SeedResponse {
            anchors_count: engine.anchors.len(),
            events_count: engine.events.len(),
            seeds_count: engine.seeds.len(),
            total_traces: engine.traces.len(),
            field_tension: engine.ecg_report().map(|r| r.current.tension).unwrap_or(0.0),
        }
    }).await;

    match result {
        Ok(resp) => Json(resp),
        Err(e) => {
            log_error!("seed: spawn_blocking panicked: {}", e);
            Json(SeedResponse {
                anchors_count: 0,
                events_count: 0,
                seeds_count: 0,
                total_traces: 0,
                field_tension: 0.0,
            })
        }
    }
}
