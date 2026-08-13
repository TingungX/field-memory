//! Versioned snapshot persistence and the v2 canonical hash codec.
//!
//! JSON is a human-readable transport container only.  Every authoritative
//! digest is encoded through `fm_v2_canonical_le_v1`, never by hashing JSON or
//! a Rust memory representation.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, V2Error};
use crate::event::{
    canonical_f64_bits, CoordinateLogEntry, Event, EventCoordinate, EventId, FieldState, Lifecycle,
    StepId,
};
use crate::geometry::Direction;
use crate::numeric::EPS_ANGLE;
use crate::version::{EmbeddingIdentity, FieldVersion};

/// The snapshot envelope schema frozen by the v2 implementation contract.
pub const SNAPSHOT_STATE_SCHEMA_VERSION: u32 = 2;

const MAPPING_VERSION: u32 = 2;
const STATE_SHA_DOMAIN: &[u8] = b"fm-v2/state/v2\0";
const MAPPING_SHA_DOMAIN: &[u8] = b"fm-v2/mapping/v2\0";
static TEMP_FILE_NONCE: AtomicU64 = AtomicU64::new(0);

/// One row of the independently hashed Event-to-derived-geometry mapping.
///
/// `geometry_gauge_rank` is the contract's rank after sorting geometry units
/// by `(intrinsic_signature, geometry_label)`.  `site_id` remains absent for
/// Building snapshots until a final resolution has been derived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappingEntry {
    pub event_id: EventId,
    pub geometry_gauge_rank: u64,
    pub site_id: Option<u64>,
}

/// Canonical writer shared by all v2 SHA payloads.
///
/// Later physics/readiness modules use this codec through `pub(crate)` access;
/// no caller may serialize a Rust struct or JSON value and hash those bytes.
pub(crate) struct CanonicalWriter {
    hasher: Sha256,
}

impl CanonicalWriter {
    pub(crate) fn new(domain_tag: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain_tag);
        Self { hasher }
    }

    pub(crate) fn write_u8(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    pub(crate) fn write_u32(&mut self, value: u32) {
        self.hasher.update(value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.hasher.update(value.to_le_bytes());
    }

    pub(crate) fn write_f64(&mut self, value: f64) -> Result<()> {
        if !value.is_finite() {
            return Err(V2Error::Serialization(
                "fm_v2_canonical_le_v1 cannot encode NaN or infinity".into(),
            ));
        }
        self.write_u64(canonical_f64_bits(value));
        Ok(())
    }

    pub(crate) fn write_string(&mut self, value: &str) -> Result<()> {
        self.write_len(value.len())?;
        self.hasher.update(value.as_bytes());
        Ok(())
    }

    pub(crate) fn write_len(&mut self, value: usize) -> Result<()> {
        let value = u64::try_from(value).map_err(|_| {
            V2Error::Serialization("canonical vector/string length exceeds u64".into())
        })?;
        self.write_u64(value);
        Ok(())
    }

    pub(crate) fn finish_hex(self) -> String {
        let digest = self.hasher.finalize();
        let mut output = String::with_capacity(digest.len() * 2);
        for byte in digest {
            // Formatting into a String cannot fail.
            write!(&mut output, "{byte:02x}").expect("formatting a String is infallible");
        }
        output
    }
}

/// Hash the exact State payload defined in contract section 9.2.
pub fn state_sha256(state: &FieldState) -> Result<String> {
    state.validate()?;

    let mut writer = CanonicalWriter::new(STATE_SHA_DOMAIN);
    canonical_encode_field_version(&mut writer, &state.version)?;
    writer.write_u8(state.lifecycle.canonical_tag());
    writer.write_u64(state.next_event_id.0);
    writer.write_u64(state.next_step_id.0);

    writer.write_len(state.events.len())?;
    for event in &state.events {
        canonical_encode_event(&mut writer, event)?;
    }

    writer.write_len(state.coordinate_log.len())?;
    for entry in &state.coordinate_log {
        canonical_encode_log_entry(&mut writer, entry)?;
    }
    Ok(writer.finish_hex())
}

/// Hash the exact Mapping payload defined in contract section 9.2.
pub fn mapping_sha256(entries: &[MappingEntry]) -> Result<String> {
    let entries = canonical_mapping_entries(entries)?;
    let mut writer = CanonicalWriter::new(MAPPING_SHA_DOMAIN);
    writer.write_u32(MAPPING_VERSION);
    writer.write_len(entries.len())?;
    for entry in entries {
        writer.write_u64(entry.event_id.0);
        writer.write_u64(entry.geometry_gauge_rank);
        match entry.site_id {
            None => writer.write_u8(0),
            Some(site_id) => {
                writer.write_u8(1);
                writer.write_u64(site_id);
            }
        }
    }
    Ok(writer.finish_hex())
}

/// Rebuild the complete Building-only geometry mapping.
///
/// This deliberately derives no `DensitySite`: a Building state is legal even
/// when `G_K` is infeasible.  The geometry rank is nevertheless fully
/// deterministic and follows the contract's representative-greedy and
/// intrinsic-signature rules.
pub fn derive_building_mapping(state: &FieldState) -> Result<Vec<MappingEntry>> {
    state.validate()?;
    if state.lifecycle != Lifecycle::Building {
        return Err(active_mapping_requires_resolution());
    }

    let mut units: Vec<BuildingGeometryUnit> = Vec::new();
    let mut event_unit_indices = Vec::with_capacity(state.events.len());
    for event in &state.events {
        let mut selected_unit: Option<usize> = None;
        for (index, unit) in units.iter().enumerate() {
            if unit.representative.angle(&event.coordinate)? <= EPS_ANGLE {
                selected_unit = match selected_unit {
                    None => Some(index),
                    Some(current) if unit.geometry_label < units[current].geometry_label => {
                        Some(index)
                    }
                    Some(current) => Some(current),
                };
            }
        }

        let unit_index = selected_unit.unwrap_or_else(|| {
            units.push(BuildingGeometryUnit {
                geometry_label: event.id,
                representative: event.coordinate.clone(),
            });
            units.len() - 1
        });
        event_unit_indices.push(unit_index);
    }

    let signatures = intrinsic_signatures(&units)?;
    let mut sorted_unit_indices: Vec<usize> = (0..units.len()).collect();
    sorted_unit_indices.sort_by(|left, right| {
        compare_geometry_gauge(
            &signatures[*left],
            units[*left].geometry_label,
            &signatures[*right],
            units[*right].geometry_label,
        )
    });

    let mut ranks = vec![0u64; units.len()];
    for (rank, unit_index) in sorted_unit_indices.into_iter().enumerate() {
        ranks[unit_index] = u64::try_from(rank)
            .map_err(|_| V2Error::Serialization("geometry rank exceeds u64".into()))?;
    }

    let entries = state
        .events
        .iter()
        .zip(event_unit_indices)
        .map(|(event, unit_index)| MappingEntry {
            event_id: event.id,
            geometry_gauge_rank: ranks[unit_index],
            site_id: None,
        })
        .collect::<Vec<_>>();
    validate_mapping_for_state(state, &entries)?;
    Ok(entries)
}

/// Atomically save a snapshot using a caller-derived complete mapping.
///
/// Active callers must supply the resolution-integrated mapping so the saved
/// `mapping_sha256` represents the final site assignment without storing
/// derived rows as FieldState truth.
pub fn save_v2_with_mapping(
    path: impl AsRef<Path>,
    state: &FieldState,
    mapping: &[MappingEntry],
) -> Result<()> {
    state.validate()?;
    validate_mapping_for_state(state, mapping)?;
    let snapshot = SnapshotV2::from_state(state, state_sha256(state)?, mapping_sha256(mapping)?);
    let bytes = serde_json::to_vec(&snapshot).map_err(|error| {
        V2Error::Serialization(format!("cannot serialize v2 snapshot: {error}"))
    })?;
    atomic_write_json(path.as_ref(), &bytes)
}

/// Atomically save a Building snapshot with its geometry-only mapping.
///
/// Until resolution is connected, Active snapshots must use
/// [`save_v2_with_mapping`]; pretending that site assignment is absent would
/// make the mapping hash non-authoritative.
pub fn save_v2(path: impl AsRef<Path>, state: &FieldState) -> Result<()> {
    let mapping = derive_building_mapping(state)?;
    save_v2_with_mapping(path, state, &mapping)
}

/// Load and validate a snapshot against the caller's expected frozen version.
///
/// The mapping derivation closure runs only after JSON/schema/version/state
/// validation, then its canonical hash is checked against the persisted
/// mapping digest.  Mapping rows themselves are intentionally not persisted.
pub fn load_v2_with_mapping<F>(
    path: impl AsRef<Path>,
    expected_version: &FieldVersion,
    derive_mapping: F,
) -> Result<FieldState>
where
    F: FnOnce(&FieldState) -> Result<Vec<MappingEntry>>,
{
    expected_version.validate()?;
    let snapshot = read_snapshot(path.as_ref())?;
    if snapshot.state_schema_version != SNAPSHOT_STATE_SCHEMA_VERSION {
        return Err(unsupported_schema(snapshot.state_schema_version));
    }
    snapshot.version.validate()?;
    if snapshot.version != *expected_version {
        return Err(V2Error::InvalidVersion(
            "snapshot FieldVersion does not exactly match the expected frozen identity".into(),
        ));
    }

    let persisted_state_sha = parse_lowercase_sha256(&snapshot.state_sha256, "state_sha256")?;
    let persisted_mapping_sha = parse_lowercase_sha256(&snapshot.mapping_sha256, "mapping_sha256")?;
    let state = snapshot.into_state();
    state.validate()?;

    let derived_mapping = derive_mapping(&state)?;
    validate_mapping_for_state(&state, &derived_mapping)?;
    let actual_mapping_sha = mapping_sha256(&derived_mapping)?;
    if actual_mapping_sha != persisted_mapping_sha {
        return Err(V2Error::Persistence(
            "snapshot mapping_sha256 does not match the deterministically derived mapping".into(),
        ));
    }

    let actual_state_sha = state_sha256(&state)?;
    if actual_state_sha != persisted_state_sha {
        return Err(V2Error::Persistence(
            "snapshot state_sha256 does not match the canonical state payload".into(),
        ));
    }
    Ok(state)
}

/// Load a Building snapshot with a geometry-only mapping.
///
/// Active snapshots require the later resolution integration and are rejected
/// explicitly instead of silently treating `SiteId` as absent.
pub fn load_v2(path: impl AsRef<Path>, expected_version: &FieldVersion) -> Result<FieldState> {
    load_v2_with_mapping(path, expected_version, derive_building_mapping)
}

pub(crate) fn canonical_encode_field_version(
    writer: &mut CanonicalWriter,
    version: &FieldVersion,
) -> Result<()> {
    version.validate()?;
    writer.write_u32(version.state_schema_version);
    writer.write_string(&version.algorithm_id)?;
    writer.write_string(&version.scalar)?;
    writer.write_u32(version.dimension);
    writer.write_u32(version.sample_budget);
    writer.write_u8(version.backend_kind as u8);
    writer.write_u8(version.space_kind as u8);
    writer.write_string(&version.measure)?;
    writer.write_string(&version.kernel_id)?;
    writer.write_string(&version.projection_arithmetic)?;
    writer.write_string(&version.sample_projection_id)?;
    writer.write_string(&version.graph_id)?;
    writer.write_string(&version.gauge_id)?;
    writer.write_f64(version.sigma)?;
    canonical_encode_option_string(writer, version.dataset_sha256.as_deref())?;
    canonical_encode_option_embedding_identity(writer, version.embedding_identity.as_ref())?;
    Ok(())
}

fn canonical_encode_event(writer: &mut CanonicalWriter, event: &Event) -> Result<()> {
    writer.write_u64(event.id.0);
    writer.write_string(&event.content)?;
    canonical_encode_coordinate(writer, &event.coordinate)
}

fn canonical_encode_log_entry(
    writer: &mut CanonicalWriter,
    entry: &CoordinateLogEntry,
) -> Result<()> {
    writer.write_u64(entry.step_id.0);
    writer.write_u64(entry.event_id.0);
    writer.write_u8(entry.phase.canonical_tag());
    canonical_encode_coordinate(writer, &entry.before_coordinate)?;
    canonical_encode_coordinate(writer, &entry.after_coordinate)
}

pub(crate) fn canonical_encode_coordinate(
    writer: &mut CanonicalWriter,
    coordinate: &EventCoordinate,
) -> Result<()> {
    writer.write_len(coordinate.dimension())?;
    for value in coordinate.as_slice() {
        writer.write_f64(*value)?;
    }
    Ok(())
}

fn canonical_encode_option_string(writer: &mut CanonicalWriter, value: Option<&str>) -> Result<()> {
    match value {
        None => writer.write_u8(0),
        Some(value) => {
            writer.write_u8(1);
            writer.write_string(value)?;
        }
    }
    Ok(())
}

fn canonical_encode_option_embedding_identity(
    writer: &mut CanonicalWriter,
    value: Option<&EmbeddingIdentity>,
) -> Result<()> {
    match value {
        None => writer.write_u8(0),
        Some(value) => {
            writer.write_u8(1);
            canonical_encode_embedding_identity(writer, value)?;
        }
    }
    Ok(())
}

fn canonical_encode_embedding_identity(
    writer: &mut CanonicalWriter,
    identity: &EmbeddingIdentity,
) -> Result<()> {
    writer.write_string(&identity.model)?;
    writer.write_string(&identity.model_digest)?;
    writer.write_string(&identity.dataset_sha256)?;
    writer.write_u32(identity.source_dimension);
    writer.write_u32(identity.target_dimension);
    writer.write_string(&identity.projection_family)?;
    writer.write_string(&identity.projection_archive_sha256)?;
    writer.write_string(&identity.projection_content_sha256)?;
    writer.write_string(&identity.ollama_version)?;
    Ok(())
}

fn canonical_mapping_entries(entries: &[MappingEntry]) -> Result<Vec<MappingEntry>> {
    let mut entries = entries.to_vec();
    entries.sort_by_key(|entry| entry.event_id);
    for (index, entry) in entries.iter().enumerate() {
        let expected_event_id = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or_else(|| {
                V2Error::Serialization("mapping length exceeds the EventId domain".into())
            })?;
        if entry.event_id != EventId(expected_event_id) {
            return Err(V2Error::Persistence(format!(
                "mapping entries must contain each EventId exactly once in 1..={}: found {} at position {}",
                entries.len(),
                entry.event_id.0,
                index
            )));
        }
    }
    Ok(entries)
}

fn validate_mapping_for_state(state: &FieldState, entries: &[MappingEntry]) -> Result<()> {
    let entries = canonical_mapping_entries(entries)?;
    if entries.len() != state.events.len() {
        return Err(V2Error::Persistence(format!(
            "mapping row count must equal event count: {} != {}",
            entries.len(),
            state.events.len()
        )));
    }
    for (event, entry) in state.events.iter().zip(entries) {
        if event.id != entry.event_id {
            return Err(V2Error::Persistence(
                "mapping EventIds do not match the persistent Event rows".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct BuildingGeometryUnit {
    geometry_label: EventId,
    representative: Direction,
}

fn intrinsic_signatures(units: &[BuildingGeometryUnit]) -> Result<Vec<Vec<f64>>> {
    units
        .iter()
        .map(|unit| {
            let mut signature = units
                .iter()
                .map(|other| unit.representative.angle(&other.representative))
                .collect::<Result<Vec<_>>>()?;
            signature.sort_by(f64::total_cmp);
            Ok(signature)
        })
        .collect()
}

fn compare_geometry_gauge(
    left_signature: &[f64],
    left_label: EventId,
    right_signature: &[f64],
    right_label: EventId,
) -> Ordering {
    for (left, right) in left_signature.iter().zip(right_signature) {
        if (left - right).abs() > EPS_ANGLE {
            return left.total_cmp(right);
        }
    }
    left_label.cmp(&right_label)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotV2 {
    state_schema_version: u32,
    version: FieldVersion,
    lifecycle: Lifecycle,
    next_event_id: EventId,
    next_step_id: StepId,
    events: Vec<Event>,
    coordinate_log: Vec<CoordinateLogEntry>,
    state_sha256: String,
    mapping_sha256: String,
}

impl SnapshotV2 {
    fn from_state(state: &FieldState, state_sha256: String, mapping_sha256: String) -> Self {
        Self {
            state_schema_version: SNAPSHOT_STATE_SCHEMA_VERSION,
            version: state.version.clone(),
            lifecycle: state.lifecycle,
            next_event_id: state.next_event_id,
            next_step_id: state.next_step_id,
            events: state.events.clone(),
            coordinate_log: state.coordinate_log.clone(),
            state_sha256,
            mapping_sha256,
        }
    }

    fn into_state(self) -> FieldState {
        FieldState {
            version: self.version,
            lifecycle: self.lifecycle,
            next_event_id: self.next_event_id,
            next_step_id: self.next_step_id,
            events: self.events,
            coordinate_log: self.coordinate_log,
        }
    }
}

fn read_snapshot(path: &Path) -> Result<SnapshotV2> {
    let bytes = fs::read(path).map_err(|error| {
        V2Error::Persistence(format!(
            "cannot read v2 snapshot {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        V2Error::Persistence(format!(
            "cannot parse v2 snapshot {} as schema-2 JSON (v1 persistence is not supported): {error}",
            path.display()
        ))
    })
}

fn atomic_write_json(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (temporary_path, mut temporary_file) = create_unique_temporary_file(parent)?;

    let write_result = (|| -> std::io::Result<()> {
        temporary_file.write_all(bytes)?;
        temporary_file.flush()?;
        temporary_file.sync_all()?;
        Ok(())
    })();
    drop(temporary_file);

    if let Err(error) = write_result {
        let cleanup_detail = cleanup_temporary_file(&temporary_path);
        return Err(V2Error::Persistence(format!(
            "cannot write temporary v2 snapshot {}: {error}{cleanup_detail}",
            temporary_path.display(),
        )));
    }

    if let Err(error) = fs::rename(&temporary_path, path) {
        let cleanup_detail = cleanup_temporary_file(&temporary_path);
        return Err(V2Error::Persistence(format!(
            "cannot atomically replace v2 snapshot {}: {error}{cleanup_detail}",
            path.display(),
        )));
    }

    let directory = File::open(parent).map_err(|error| {
        V2Error::Persistence(format!(
            "cannot open v2 snapshot parent directory {} for sync: {error}",
            parent.display()
        ))
    })?;
    directory.sync_all().map_err(|error| {
        V2Error::Persistence(format!(
            "cannot sync v2 snapshot parent directory {}: {error}",
            parent.display()
        ))
    })
}

fn create_unique_temporary_file(parent: &Path) -> Result<(PathBuf, File)> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for _ in 0..64 {
        let nonce = TEMP_FILE_NONCE.fetch_add(1, AtomicOrdering::Relaxed);
        let temporary_path = parent.join(format!(
            ".fm-v2-snapshot-{}-{timestamp}-{nonce}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(V2Error::Persistence(format!(
                    "cannot create temporary v2 snapshot {}: {error}",
                    temporary_path.display()
                )));
            }
        }
    }
    Err(V2Error::Persistence(
        "could not allocate a unique temporary v2 snapshot path".into(),
    ))
}

fn cleanup_temporary_file(path: &Path) -> String {
    match fs::remove_file(path) {
        Ok(()) => String::new(),
        Err(error) => format!(
            "; also failed to remove temporary file {}: {error}",
            path.display()
        ),
    }
}

fn parse_lowercase_sha256(value: &str, field_name: &str) -> Result<String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(V2Error::Persistence(format!(
            "snapshot {field_name} must be a lowercase 64-character SHA-256 hex string"
        )));
    }
    Ok(value.to_owned())
}

fn unsupported_schema(schema_version: u32) -> V2Error {
    if schema_version == 1 {
        V2Error::Persistence(
            "v1 persistence is intentionally unsupported; create a new v2 Building library and re-deposit content".into(),
        )
    } else {
        V2Error::Persistence(format!(
            "unsupported v2 snapshot state_schema_version {schema_version}; expected {SNAPSHOT_STATE_SCHEMA_VERSION}"
        ))
    }
}

fn active_mapping_requires_resolution() -> V2Error {
    V2Error::Persistence(
        "Active snapshot mapping requires resolution integration; use save_v2_with_mapping/load_v2_with_mapping"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::event::new_building;

    fn point(x: f64, y: f64, z: f64) -> Direction {
        Direction::new(vec![x, y, z]).unwrap()
    }

    #[test]
    fn building_snapshot_round_trips_with_canonical_hashes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let version = FieldVersion::physics_reference_s2(4).unwrap();
        let mut state = new_building(version.clone()).unwrap();
        state
            .deposit_building("first", point(1.0, 0.0, 0.0))
            .unwrap();
        state
            .deposit_building("duplicate", point(1.0, 0.0, 0.0))
            .unwrap();
        state
            .deposit_building("other", point(0.0, 1.0, 0.0))
            .unwrap();

        let state_hash = state_sha256(&state).unwrap();
        let mapping = derive_building_mapping(&state).unwrap();
        assert_eq!(
            mapping[0].geometry_gauge_rank,
            mapping[1].geometry_gauge_rank
        );
        save_v2(&path, &state).unwrap();

        let parsed: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            parsed["state_schema_version"],
            SNAPSHOT_STATE_SCHEMA_VERSION
        );
        assert!(parsed.get("mapping").is_none());
        assert_eq!(parsed["state_sha256"], state_hash);

        let loaded = load_v2(&path, &version).unwrap();
        assert_eq!(loaded, state);
        assert_eq!(state_sha256(&loaded).unwrap(), state_hash);
        assert_eq!(
            mapping_sha256(&derive_building_mapping(&loaded).unwrap()).unwrap(),
            mapping_sha256(&mapping).unwrap()
        );
    }

    #[test]
    fn mapping_hash_is_order_independent_but_rejects_missing_event_ids() {
        let ordered = vec![
            MappingEntry {
                event_id: EventId(1),
                geometry_gauge_rank: 1,
                site_id: None,
            },
            MappingEntry {
                event_id: EventId(2),
                geometry_gauge_rank: 0,
                site_id: Some(0),
            },
        ];
        let reversed = vec![ordered[1].clone(), ordered[0].clone()];
        assert_eq!(
            mapping_sha256(&ordered).unwrap(),
            mapping_sha256(&reversed).unwrap()
        );
        assert!(mapping_sha256(&[MappingEntry {
            event_id: EventId(2),
            geometry_gauge_rank: 0,
            site_id: None,
        }])
        .is_err());
    }

    #[test]
    fn load_rejects_v1_schema_and_tampered_state_hash() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let version = FieldVersion::physics_reference_s2(4).unwrap();
        let mut state = new_building(version.clone()).unwrap();
        state.deposit_building("one", point(1.0, 0.0, 0.0)).unwrap();
        save_v2(&path, &state).unwrap();

        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        snapshot["state_schema_version"] = serde_json::json!(1);
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(load_v2(&path, &version).is_err());

        save_v2(&path, &state).unwrap();
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        snapshot["state_sha256"] = serde_json::json!("0".repeat(64));
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(load_v2(&path, &version).is_err());
    }

    #[test]
    fn active_requires_explicit_resolution_mapping() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let version = FieldVersion::physics_reference_s2(4).unwrap();
        let mut state = new_building(version.clone()).unwrap();
        state.deposit_building("one", point(1.0, 0.0, 0.0)).unwrap();
        state.lifecycle = Lifecycle::Active;
        state.validate().unwrap();

        assert!(save_v2(&path, &state).is_err());
        let mapping = vec![MappingEntry {
            event_id: EventId(1),
            geometry_gauge_rank: 0,
            site_id: Some(0),
        }];
        save_v2_with_mapping(&path, &state, &mapping).unwrap();
        let loaded = load_v2_with_mapping(&path, &version, |_| Ok(mapping.clone())).unwrap();
        assert_eq!(loaded, state);
    }
}
