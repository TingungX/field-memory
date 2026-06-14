use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Vector type alias — f32 dynamic-length vector
pub type Vector = Vec<f32>;

// ================================================================
// EventSource — where an event came from
// ================================================================

/// Classifies the origin of an event to prevent duplicate ingestion.
///
/// - `User`    — genuine user input, always recorded
/// - `Seed`    — synthetic events from `seed_memory` tool
/// - `RecallEcho` — content that originated from a recall (should NOT be re-ingested)
/// - `System`  — system prompt / framework-generated text (should NOT be re-ingested)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    User,
    Seed,
    RecallEcho,
    System,
}

impl Default for EventSource {
    fn default() -> Self {
        Self::User
    }
}

// ================================================================
// Event
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub direction: Vector,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    /// Origin of this event — used to filter out recall echoes and system text.
    #[serde(default)]
    pub source: EventSource,
}

impl Event {
    pub fn new(id: EventId, direction: Vector, text: String) -> Self {
        Self { id, direction, text, timestamp: Utc::now(), source: EventSource::User }
    }

    pub fn with_source(id: EventId, direction: Vector, text: String, source: EventSource) -> Self {
        Self { id, direction, text, timestamp: Utc::now(), source }
    }
}

// ================================================================
// AnchorKey
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorKey {
    pub id: AnchorId,
    pub label: String,
    pub direction: Vector,
    pub density: u32,
    pub stiffness: f32,
    pub damping: f32,
    pub origin_direction: Vector,
}

impl AnchorKey {
    pub fn new(id: AnchorId, label: String, direction: Vector, initial_density: u32) -> Self {
        let density = initial_density.max(1);
        let stiffness = (density as f32).sqrt();
        let damping = 1.0 / (density as f32).sqrt();
        let origin_direction = direction.clone();
        Self { id, label, direction, density, stiffness, damping, origin_direction }
    }

    /// Recompute stiffness and damping from current density
    pub fn update_mechanics(&mut self) {
        let d = (self.density as f32).max(1.0);
        self.stiffness = d.sqrt();
        self.damping = 1.0 / d.sqrt();
    }
}

// ================================================================
// ImpactTrace
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactTrace {
    pub event_id: EventId,
    pub anchor_id: AnchorId,
    pub impact: f32,
    pub timestamp: DateTime<Utc>,
}

// ================================================================
// SeedConcept (L4)
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedConcept {
    pub id: SeedId,
    pub orthogonal_direction: Vector,
    pub shadow_anchor: AnchorKey,
    pub defeated_by: AnchorId,
    pub pressure_accumulated: f32,
}

// ================================================================
// Anisotropy for ECG
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anisotropy {
    pub direction: Vector,
    pub magnitude: f32,
    pub anchor_count: usize,
}

// ================================================================
// ID types
// ================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnchorId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SeedId(pub u64);

impl EventId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl AnchorId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl SeedId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}
