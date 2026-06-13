use std::path::Path;
use chrono::Duration;
use crate::types::{AnchorKey, Event, ImpactTrace, SeedConcept, EventId};
use crate::embed::{EmbedProvider, DummyEmbedProvider};
use crate::cycle::RelaxationCycle;
use crate::recall::{value_init, associate, recall, consolidate_from_recall, RecallResult};
use crate::init::init_anchors;
use crate::paradigm::{detect_paradigm_shift, orthogonalize};
use crate::ecg::{CognitiveEcg, EcgReport};
use crate::persist;
use crate::DseCoreParams;

/// DSE-Memory engine — the only public interface.
pub struct DseEngine {
    pub anchors: Vec<AnchorKey>,
    pub events: Vec<Event>,
    pub traces: Vec<ImpactTrace>,
    pub seeds: Vec<SeedConcept>,
    pub params: DseCoreParams,

    embed: Box<dyn EmbedProvider>,
    cycle: RelaxationCycle,
    ecg: CognitiveEcg,
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
        }
    }

    // ================================================================
    // Init
    // ================================================================

    /// Initialize anchors from concept descriptions.
    /// (label, initial_density) pairs.
    pub fn init(&mut self, concepts: &[(&str, u32)]) {
        let anchors = init_anchors(self.embed.as_ref(), concepts);
        self.anchors.extend(anchors);
    }

    // ================================================================
    // Write
    // ================================================================

    /// Accept user input text, return event ID.
    pub fn on_user_input(&mut self, text: &str) -> EventId {
        let direction = self.embed.embed(text);
        let event = Event::new(EventId::new(), direction, text.to_string());
        let id = event.id;
        self.events.push(event);
        id
    }

    // ================================================================
    // Evolve
    // ================================================================

    /// Run one relaxation cycle. Call during idle periods.
    pub fn relax(&mut self) {
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
    }

    fn detect_paradigm_shifts(&mut self) {
        for i in 0..self.anchors.len() {
            for j in (i+1)..self.anchors.len() {
                // Compute pressure (simplified: use density as proxy)
                let a_pressure = self.anchors[i].density as f32;
                let b_pressure = self.anchors[j].density as f32;

                if detect_paradigm_shift(&self.anchors[i], &self.anchors[j], a_pressure, b_pressure) {
                    let seed = orthogonalize(&self.anchors[j], &self.anchors[i]);
                    self.seeds.push(seed);
                    self.anchors.remove(j);
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
        associate(&self.anchors, &q_dir, 0.1)
    }

    /// Active: full recall with event retrieval.
    pub fn recall(&self, query: &str, top_k: usize) -> RecallResult {
        let q_dir = self.embed.embed(query);
        recall(&self.anchors, &self.traces, &self.events, &q_dir, top_k, 0.1)
    }

    /// Memory consolidation after recall.
    pub fn consolidate_from_recall(&mut self, query: &str) {
        let q_dir = self.embed.embed(query);
        consolidate_from_recall(&mut self.anchors, &q_dir, 0.3);
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
        persist::save(path, &self.anchors, &self.events, &self.traces, &self.seeds)
    }

    pub fn load(&mut self, path: &Path) -> Result<(), persist::PersistError> {
        let (anchors, events, traces, seeds) = persist::load(path)?;
        self.anchors = anchors;
        self.events = events;
        self.traces = traces;
        self.seeds = seeds;
        Ok(())
    }
}
