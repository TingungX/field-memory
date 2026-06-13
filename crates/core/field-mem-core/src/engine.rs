use std::path::Path;
use chrono::{DateTime, Utc, Duration};
use crate::types::{AnchorKey, Event, ImpactTrace, SeedConcept, EventId};
use crate::embed::{EmbedProvider, DummyEmbedProvider};
use crate::cycle::RelaxationCycle;
use crate::recall::{value_init, associate, recall, consolidate_from_recall, RecallResult};
use crate::init::init_anchors;
use crate::paradigm::{detect_paradigm_shift, orthogonalize};
use crate::ecg::{CognitiveEcg, EcgReport};
use crate::persist;
use crate::DseCoreParams;

// ── Activity monitoring ──

/// What activity the engine just performed.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ActivityKind {
    Init,
    EventInput,
    Relax,
    ParadigmShift,
    Recall,
    Associate,
    Consolidate,
    Save,
    Load,
}

/// A single activity record with timestamp.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityEntry {
    pub kind: ActivityKind,
    pub detail: String,
    pub timestamp: DateTime<Utc>,
}

/// Derive memory layer label from density.
/// L1=long-term core, L2=consolidated, L3=emerging, L4=fragile
fn anchor_layer(density: u32) -> &'static str {
    if density > 15 { "L1" }
    else if density > 8 { "L2" }
    else if density > 3 { "L3" }
    else { "L4" }
}

/// DSE-Memory engine — the only public interface.
pub struct DseEngine {
    pub anchors: Vec<AnchorKey>,
    pub events: Vec<Event>,
    pub traces: Vec<ImpactTrace>,
    pub seeds: Vec<SeedConcept>,
    pub params: DseCoreParams,
    pub recent_activity: Vec<ActivityEntry>,

    embed: Box<dyn EmbedProvider>,
    pub cycle: RelaxationCycle,
    pub ecg: CognitiveEcg,
}

impl DseEngine {
    /// Create a new engine with a dummy embedding provider.
    /// For production, use `with_embed` to inject a real embedding model.
    pub fn new(params: DseCoreParams) -> Self {
        let dim = params.vector_dim;
        let embed = Box::new(DummyEmbedProvider::new(dim));
        Self::with_embed(params, embed)
    }

    /// Create engine with a custom embedding provider.
    pub fn with_embed(params: DseCoreParams, embed: Box<dyn EmbedProvider>) -> Self {
        let cycle = RelaxationCycle::new(&params);
        let ecg = CognitiveEcg::new();
        Self {
            anchors: vec![],
            events: vec![],
            traces: vec![],
            seeds: vec![],
            params,
            embed,
            cycle,
            ecg,
            recent_activity: vec![],
        }
    }

    fn log_activity(&mut self, kind: ActivityKind, detail: String) {
        self.recent_activity.push(ActivityEntry {
            kind,
            detail,
            timestamp: Utc::now(),
        });
        if self.recent_activity.len() > 50 {
            self.recent_activity.remove(0);
        }
    }

    // ================================================================
    // Init
    // ================================================================

    /// Initialize anchors from concept descriptions.
    /// (label, initial_density) pairs.
    pub fn init(&mut self, concepts: &[(&str, u32)]) {
        let anchors = init_anchors(self.embed.as_ref(), concepts);
        let details: Vec<String> = anchors.iter().map(|a| {
            let layer = anchor_layer(a.density);
            format!("「{}」d={} s={:.2} dmp={:.2} {}", a.label, a.density, a.stiffness, a.damping, layer)
        }).collect();
        let count = anchors.len();
        self.anchors.extend(anchors);
        self.log_activity(ActivityKind::Init, format!("创建 {} 个锚点:\n  {}", count, details.join("\n  ")));
    }

    // ================================================================
    // Write
    // ================================================================

    /// Accept user input text, return event ID.
    pub fn on_user_input(&mut self, text: &str) -> EventId {
        let direction = self.embed.embed(text);
        let event = Event::new(EventId::new(), direction, text.to_string());
        let id = event.id;
        let preview = {
            let char_count = text.chars().count();
            if char_count > 60 {
                format!("{}...", text.chars().take(60).collect::<String>())
            } else {
                text.to_string()
            }
        };
        self.events.push(event);
        self.log_activity(ActivityKind::EventInput, format!("事件 #{}: 「{}」", id.0, preview));
        id
    }

    // ================================================================
    // Evolve
    // ================================================================

    /// Run one relaxation cycle. Call during idle periods.
    pub fn relax(&mut self) {
        let before = self.traces.len();
        let before_anchors = self.anchors.len();

        self.cycle.run(&self.events, &mut self.anchors, &mut self.traces);

        // Paradigm shift detection
        self.detect_paradigm_shifts();

        // ECG snapshot
        let inflow = self.events.iter()
            .filter(|e| {
                let elapsed = chrono::Utc::now().signed_duration_since(e.timestamp);
                elapsed < Duration::seconds(self.params.event_window_secs as i64)
            })
            .count();
        self.ecg.snapshot(
            &self.anchors,
            &self.traces,
            inflow,
            Duration::seconds(self.params.event_window_secs as i64),
        );

        let new_traces = self.traces.len() - before;
        let shifts = before_anchors - self.anchors.len();
        // Get anchor density changes as preview
        let anchor_summary: Vec<String> = self.anchors.iter().map(|a| {
            let layer = anchor_layer(a.density);
            format!("{} d={} {}", a.label, a.density, layer)
        }).collect();
        let mut detail = format!("新痕迹={}, 锚点=[{}]", new_traces, anchor_summary.join(", "));
        if shifts > 0 {
            detail.push_str(&format!(", 范式转移={}", shifts));
        }
        self.log_activity(ActivityKind::Relax, detail);
    }

    fn detect_paradigm_shifts(&mut self) {
        for i in 0..self.anchors.len() {
            for j in (i+1)..self.anchors.len() {
                let a_pressure = self.anchors[i].density as f32;
                let b_pressure = self.anchors[j].density as f32;

                if detect_paradigm_shift(&self.anchors[i], &self.anchors[j], a_pressure, b_pressure) {
                    let label = self.anchors[j].label.clone();
                    let seed = orthogonalize(&self.anchors[j], &self.anchors[i]);
                    self.seeds.push(seed);
                    self.anchors.remove(j);
                    self.log_activity(ActivityKind::ParadigmShift, format!("锚点「{}」被击败→种子", label));
                    return; // one shift per cycle
                }
            }
        }
    }

    // ================================================================
    // Recall
    // ================================================================

    /// Passive 1: value initialization for session start.
    pub fn value_init(&self) -> Vec<(&AnchorKey, f32)> {
        value_init(&self.anchors, 20)
            .into_iter()
            .map(|a| (a, a.density as f32))
            .collect()
    }

    /// Passive 2: associative recall (concept-level).
    pub fn associate(&self, query: &str) -> Vec<(&AnchorKey, f32)> {
        let q_dir = self.embed.embed(query);
        let result = associate(&self.anchors, &q_dir, 0.1);
        result
    }

    /// Active: full recall with event retrieval.
    pub fn recall(&self, query: &str, top_k: usize) -> RecallResult {
        let q_dir = self.embed.embed(query);
        let result = recall(&self.anchors, &self.traces, &self.events, &q_dir, top_k, 0.1);
        result
    }

    /// Memory consolidation after recall.
    pub fn consolidate_from_recall(&mut self, query: &str) {
        let q_dir = self.embed.embed(query);
        consolidate_from_recall(&mut self.anchors, &q_dir, 0.3);
        self.log_activity(ActivityKind::Consolidate, format!("query: {}", query));
    }

    // ================================================================
    // ECG
    // ================================================================

    pub fn ecg_report(&self) -> Option<EcgReport> {
        self.ecg.report()
    }

    // ================================================================
    // Persistence
    // ================================================================

    pub fn save(&self, path: &Path) -> Result<(), persist::PersistError> {
        let result = persist::save(path, &self.anchors, &self.events, &self.traces, &self.seeds);
        // Can't log activity since we're &self, but we can't mutate. The server will log.
        result
    }

    pub fn load(&mut self, path: &Path) -> Result<(), persist::PersistError> {
        let (anchors, events, traces, seeds) = persist::load(path)?;
        let a = anchors.len();
        let e = events.len();
        let t = traces.len();
        let s = seeds.len();
        self.anchors = anchors;
        self.events = events;
        self.traces = traces;
        self.seeds = seeds;
        self.log_activity(ActivityKind::Load, format!("锚点={}, 事件={}, 痕迹={}, 种子={}", a, e, t, s));
        Ok(())
    }

    /// Log save/load from outside (server side calls this since save takes &self).
    pub fn log_save(&mut self) {
        self.log_activity(ActivityKind::Save, format!("锚点={}, 事件={}", self.anchors.len(), self.events.len()));
    }
}
