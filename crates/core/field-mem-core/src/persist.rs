use crate::types::{AnchorKey, Event, ImpactTrace, SeedConcept};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("missing data: {0}")]
    MissingData(&'static str),
}

/// Save all DSE state to disk.
pub fn save(
    path: &Path,
    anchors: &[AnchorKey],
    events: &[Event],
    traces: &[ImpactTrace],
    seeds: &[SeedConcept],
) -> Result<(), PersistError> {
    let db = sled::open(path)?;

    db.insert("anchors", bincode::serialize(anchors).map_err(|e| PersistError::Serialization(e.to_string()))?)?;
    db.insert("events", bincode::serialize(events).map_err(|e| PersistError::Serialization(e.to_string()))?)?;
    db.insert("traces", bincode::serialize(traces).map_err(|e| PersistError::Serialization(e.to_string()))?)?;
    db.insert("seeds", bincode::serialize(seeds).map_err(|e| PersistError::Serialization(e.to_string()))?)?;
    db.insert("meta", bincode::serialize(&Meta {
        saved_at: chrono::Utc::now().to_rfc3339(),
        anchor_count: anchors.len(),
        event_count: events.len(),
        trace_count: traces.len(),
        seed_count: seeds.len(),
    }).map_err(|e| PersistError::Serialization(e.to_string()))?)?;

    db.flush()?;
    Ok(())
}

/// Load all DSE state from disk.
pub fn load(
    path: &Path,
) -> Result<(Vec<AnchorKey>, Vec<Event>, Vec<ImpactTrace>, Vec<SeedConcept>), PersistError> {
    let db = sled::open(path)?;

    let anchors: Vec<AnchorKey> = bincode::deserialize(
        &db.get("anchors")?.ok_or_else(|| PersistError::MissingData("anchors"))?,
    ).map_err(|e| PersistError::Serialization(e.to_string()))?;
    let events: Vec<Event> = bincode::deserialize(
        &db.get("events")?.ok_or_else(|| PersistError::MissingData("events"))?,
    ).map_err(|e| PersistError::Serialization(e.to_string()))?;
    let traces: Vec<ImpactTrace> = bincode::deserialize(
        &db.get("traces")?.ok_or_else(|| PersistError::MissingData("traces"))?,
    ).map_err(|e| PersistError::Serialization(e.to_string()))?;
    let seeds: Vec<SeedConcept> = bincode::deserialize(
        &db.get("seeds")?.unwrap_or_default(),
    ).unwrap_or_default();

    Ok((anchors, events, traces, seeds))
}

#[derive(serde::Serialize)]
struct Meta {
    saved_at: String,
    anchor_count: usize,
    event_count: usize,
    trace_count: usize,
    seed_count: usize,
}

