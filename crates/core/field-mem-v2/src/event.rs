//! Persistent event-state types and Building-only mutation.
//!
//! The v2 numeric state is the current event coordinates.  Everything derived
//! from them (geometry units, density, samples, transport and readout) stays
//! out of this module and out of the snapshot state.

use serde::{Deserialize, Serialize};

use crate::error::{Result, V2Error};
use crate::geometry::Direction;
use crate::version::FieldVersion;

/// A content payload attached to an event.
///
/// It intentionally remains a `String`: the contract requires the original
/// UTF-8 bytes to survive deposits and dynamics unchanged.
pub type EventContent = String;

/// A normalized coordinate in the field's versioned direction space.
pub type EventCoordinate = Direction;

/// Stable identifier for an event row.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
#[serde(transparent)]
pub struct EventId(pub u64);

/// Stable identifier for one successful Active event step.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
#[serde(transparent)]
pub struct StepId(pub u64);

/// The only two lifecycle stages of a persisted v2 field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Building,
    Active,
}

impl Lifecycle {
    /// Frozen `fm_v2_canonical_le_v1` enum tag.
    pub const fn canonical_tag(self) -> u8 {
        match self {
            Self::Building => 0,
            Self::Active => 1,
        }
    }
}

/// Phase in which one coordinate transition occurred.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatePhase {
    Natural,
    External,
}

impl CoordinatePhase {
    /// Frozen `fm_v2_canonical_le_v1` enum tag.
    pub const fn canonical_tag(self) -> u8 {
        match self {
            Self::Natural => 0,
            Self::External => 1,
        }
    }
}

/// Immutable content plus the current coordinate of one event.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: EventId,
    pub content: EventContent,
    pub coordinate: EventCoordinate,
}

impl Event {
    pub fn new(id: EventId, content: EventContent, coordinate: EventCoordinate) -> Result<Self> {
        if id.0 == 0 {
            return Err(invalid_state("event IDs start at 1"));
        }
        Ok(Self {
            id,
            content,
            coordinate,
        })
    }
}

/// Audit-only coordinate transition emitted by an Active event step.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinateLogEntry {
    pub step_id: StepId,
    pub event_id: EventId,
    pub phase: CoordinatePhase,
    pub before_coordinate: EventCoordinate,
    pub after_coordinate: EventCoordinate,
}

impl CoordinateLogEntry {
    pub fn new(
        step_id: StepId,
        event_id: EventId,
        phase: CoordinatePhase,
        before_coordinate: EventCoordinate,
        after_coordinate: EventCoordinate,
    ) -> Result<Self> {
        if step_id.0 == 0 {
            return Err(invalid_state("step IDs start at 1"));
        }
        if event_id.0 == 0 {
            return Err(invalid_state("event IDs start at 1"));
        }
        if before_coordinate.dimension() != after_coordinate.dimension() {
            return Err(V2Error::DimensionMismatch {
                expected: before_coordinate.dimension(),
                actual: after_coordinate.dimension(),
            });
        }
        Ok(Self {
            step_id,
            event_id,
            phase,
            before_coordinate,
            after_coordinate,
        })
    }
}

/// The complete persisted v2 state.
///
/// Derived numerical structures deliberately do not appear here.  The state
/// can therefore be replayed from coordinates alone, while `coordinate_log`
/// remains an audit trail rather than a numerical input.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldState {
    pub version: FieldVersion,
    pub lifecycle: Lifecycle,
    pub next_event_id: EventId,
    pub next_step_id: StepId,
    pub events: Vec<Event>,
    pub coordinate_log: Vec<CoordinateLogEntry>,
}

impl FieldState {
    /// Start an empty field that can only receive Building deposits.
    pub fn new_building(version: FieldVersion) -> Result<Self> {
        version.validate()?;
        Ok(Self {
            version,
            lifecycle: Lifecycle::Building,
            next_event_id: EventId(1),
            next_step_id: StepId(1),
            events: Vec::new(),
            coordinate_log: Vec::new(),
        })
    }

    /// Add one original event during `Building` without running dynamics or
    /// creating an audit record.
    pub fn deposit_building(
        &mut self,
        content: impl Into<EventContent>,
        coordinate: EventCoordinate,
    ) -> Result<EventId> {
        self.validate()?;
        if self.lifecycle != Lifecycle::Building {
            return Err(V2Error::InvalidLifecycle);
        }
        validate_coordinate_dimension(&coordinate, &self.version)?;

        let id = self.next_event_id;
        let next_event_id =
            id.0.checked_add(1)
                .map(EventId)
                .ok_or_else(|| invalid_state("event ID space is exhausted"))?;
        let event = Event::new(id, content.into(), coordinate)?;

        // All fallible work happens before these two mutations.  A failed
        // deposit consequently cannot consume an identifier.
        self.events.push(event);
        self.next_event_id = next_event_id;
        Ok(id)
    }

    /// Check every persisted-state invariant required before hashing or I/O.
    pub fn validate(&self) -> Result<()> {
        self.version.validate()?;

        if self.next_event_id.0 == 0 || self.next_step_id.0 == 0 {
            return Err(invalid_state("next IDs must be non-zero"));
        }

        let expected_dimension = self.version.dimension as usize;
        for (index, event) in self.events.iter().enumerate() {
            let expected_id = u64::try_from(index)
                .map_err(|_| invalid_state("event count exceeds the ID domain"))?
                .checked_add(1)
                .ok_or_else(|| invalid_state("event count exceeds the ID domain"))?;
            if event.id != EventId(expected_id) {
                return Err(invalid_state(format!(
                    "events must be strictly ordered and contiguous: expected event ID {expected_id}, got {}",
                    event.id.0
                )));
            }
            validate_coordinate_dimension(&event.coordinate, &self.version)?;
        }

        let expected_next_event_id = u64::try_from(self.events.len())
            .map_err(|_| invalid_state("event count exceeds the ID domain"))?
            .checked_add(1)
            .ok_or_else(|| invalid_state("event count exceeds the ID domain"))?;
        if self.next_event_id != EventId(expected_next_event_id) {
            return Err(invalid_state(format!(
                "next_event_id must be {expected_next_event_id}, got {}",
                self.next_event_id.0
            )));
        }

        for entry in &self.coordinate_log {
            if entry.step_id.0 == 0 || entry.event_id.0 == 0 {
                return Err(invalid_state("coordinate-log IDs must be non-zero"));
            }
            if entry.before_coordinate.dimension() != expected_dimension {
                return Err(V2Error::DimensionMismatch {
                    expected: expected_dimension,
                    actual: entry.before_coordinate.dimension(),
                });
            }
            if entry.after_coordinate.dimension() != expected_dimension {
                return Err(V2Error::DimensionMismatch {
                    expected: expected_dimension,
                    actual: entry.after_coordinate.dimension(),
                });
            }
        }

        match self.lifecycle {
            Lifecycle::Building => {
                if self.next_step_id != StepId(1) {
                    return Err(invalid_state(
                        "Building state must not have consumed a step ID",
                    ));
                }
                if !self.coordinate_log.is_empty() {
                    return Err(invalid_state(
                        "Building state must not contain coordinate-log entries",
                    ));
                }
            }
            Lifecycle::Active => self.validate_active_log()?,
        }

        Ok(())
    }

    fn validate_active_log(&self) -> Result<()> {
        let completed_steps = self
            .next_step_id
            .0
            .checked_sub(1)
            .ok_or_else(|| invalid_state("next_step_id must be non-zero"))?;
        let event_count = u64::try_from(self.events.len())
            .map_err(|_| invalid_state("event count exceeds the ID domain"))?;
        if completed_steps > event_count {
            return Err(invalid_state(
                "an Active state cannot contain more completed steps than events",
            ));
        }

        let building_event_count = event_count - completed_steps;
        let expected_log_count = expected_active_log_count(building_event_count, completed_steps)?;
        if self.coordinate_log.len() as u128 != expected_log_count {
            return Err(invalid_state(format!(
                "coordinate-log cardinality mismatch: expected {expected_log_count}, got {}",
                self.coordinate_log.len()
            )));
        }

        // Activation itself does not consume a StepId or generate audit rows.
        // A freshly activated field is therefore a valid Active state with no
        // coordinate transitions yet.
        if completed_steps == 0 {
            return Ok(());
        }

        let completed_steps = usize::try_from(completed_steps)
            .map_err(|_| invalid_state("completed step count exceeds addressable memory"))?;
        let building_event_count = usize::try_from(building_event_count)
            .map_err(|_| invalid_state("event count exceeds addressable memory"))?;
        let mut cursor = 0usize;
        let mut last_external_after: Vec<Option<&EventCoordinate>> = vec![None; self.events.len()];

        for step_offset in 0..completed_steps {
            let step_id = StepId((step_offset as u64) + 1);
            let old_event_count = building_event_count + step_offset;

            for (event_offset, previous_after) in
                last_external_after.iter().enumerate().take(old_event_count)
            {
                let entry = &self.coordinate_log[cursor + event_offset];
                let event_id = EventId((event_offset as u64) + 1);
                validate_log_identity(entry, step_id, event_id, CoordinatePhase::Natural)?;
                if let Some(previous_after) = previous_after {
                    if !canonical_coordinate_eq(&entry.before_coordinate, previous_after) {
                        return Err(invalid_state(format!(
                            "natural log for event {} in step {} does not continue the prior external coordinate",
                            event_id.0, step_id.0
                        )));
                    }
                }
            }

            let external_start = cursor + old_event_count;
            for (event_offset, last_after) in last_external_after
                .iter_mut()
                .enumerate()
                .take(old_event_count)
            {
                let natural = &self.coordinate_log[cursor + event_offset];
                let external = &self.coordinate_log[external_start + event_offset];
                let event_id = EventId((event_offset as u64) + 1);
                validate_log_identity(external, step_id, event_id, CoordinatePhase::External)?;
                if !canonical_coordinate_eq(&natural.after_coordinate, &external.before_coordinate)
                {
                    return Err(invalid_state(format!(
                        "natural and external log entries for event {} in step {} do not connect",
                        event_id.0, step_id.0
                    )));
                }
                *last_after = Some(&external.after_coordinate);
            }
            cursor = external_start + old_event_count;
        }

        debug_assert_eq!(cursor, self.coordinate_log.len());
        let old_event_count_at_last_step = building_event_count + completed_steps.saturating_sub(1);
        for (event, final_after) in self.events[..old_event_count_at_last_step]
            .iter()
            .zip(last_external_after.iter())
        {
            let final_after = final_after.ok_or_else(|| {
                invalid_state(format!(
                    "event {} is missing its final external coordinate",
                    event.id.0
                ))
            })?;
            if !canonical_coordinate_eq(&event.coordinate, final_after) {
                return Err(invalid_state(format!(
                    "event {} coordinate does not equal its final external log coordinate",
                    event.id.0
                )));
            }
        }

        Ok(())
    }
}

/// Contract-shaped free-function entry point for a new Building state.
pub fn new_building(version: FieldVersion) -> Result<FieldState> {
    FieldState::new_building(version)
}

/// Contract-shaped free-function entry point for a Building deposit.
pub fn deposit_building(
    state: &mut FieldState,
    content: impl Into<EventContent>,
    coordinate: EventCoordinate,
) -> Result<EventId> {
    state.deposit_building(content, coordinate)
}

pub(crate) fn canonical_coordinate_eq(left: &EventCoordinate, right: &EventCoordinate) -> bool {
    left.dimension() == right.dimension()
        && left
            .as_slice()
            .iter()
            .zip(right.as_slice())
            .all(|(left, right)| canonical_f64_bits(*left) == canonical_f64_bits(*right))
}

pub(crate) fn canonical_f64_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0_f64.to_bits()
    } else {
        value.to_bits()
    }
}

fn validate_coordinate_dimension(
    coordinate: &EventCoordinate,
    version: &FieldVersion,
) -> Result<()> {
    let expected = version.dimension as usize;
    let actual = coordinate.dimension();
    if actual != expected {
        return Err(V2Error::DimensionMismatch { expected, actual });
    }
    Ok(())
}

fn validate_log_identity(
    entry: &CoordinateLogEntry,
    expected_step_id: StepId,
    expected_event_id: EventId,
    expected_phase: CoordinatePhase,
) -> Result<()> {
    if entry.step_id != expected_step_id
        || entry.event_id != expected_event_id
        || entry.phase != expected_phase
    {
        return Err(invalid_state(format!(
            "coordinate-log order must be (step_id, phase, event_id); expected ({}, {:?}, {}), got ({}, {:?}, {})",
            expected_step_id.0,
            expected_phase,
            expected_event_id.0,
            entry.step_id.0,
            entry.phase,
            entry.event_id.0,
        )));
    }
    Ok(())
}

fn expected_active_log_count(building_event_count: u64, completed_steps: u64) -> Result<u128> {
    let completed_steps = u128::from(completed_steps);
    let building_event_count = u128::from(building_event_count);
    let per_phase = completed_steps
        .checked_mul(building_event_count)
        .and_then(|value| {
            completed_steps
                .checked_mul(completed_steps.saturating_sub(1))
                .and_then(|triangle| value.checked_add(triangle / 2))
        })
        .ok_or_else(|| invalid_state("coordinate-log cardinality overflows"))?;
    per_phase
        .checked_mul(2)
        .ok_or_else(|| invalid_state("coordinate-log cardinality overflows"))
}

fn invalid_state(message: impl Into<String>) -> V2Error {
    V2Error::Persistence(format!("invalid v2 field state: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::FieldVersion;

    fn point(x: f64, y: f64, z: f64) -> Direction {
        Direction::new(vec![x, y, z]).unwrap()
    }

    #[test]
    fn building_deposits_consume_only_event_ids_and_preserve_content() {
        let mut state =
            FieldState::new_building(FieldVersion::physics_reference_s2(4).unwrap()).unwrap();
        assert_eq!(
            state
                .deposit_building("\u{0000} unchanged", point(1.0, 0.0, 0.0))
                .unwrap(),
            EventId(1)
        );
        assert_eq!(
            state
                .deposit_building("second", point(0.0, 1.0, 0.0))
                .unwrap(),
            EventId(2)
        );

        assert_eq!(state.next_event_id, EventId(3));
        assert_eq!(state.next_step_id, StepId(1));
        assert!(state.coordinate_log.is_empty());
        assert_eq!(state.events[0].content.as_bytes(), b"\0 unchanged");
        state.validate().unwrap();
    }

    #[test]
    fn failed_building_deposit_does_not_consume_an_id() {
        let mut state =
            FieldState::new_building(FieldVersion::physics_reference_s2(4).unwrap()).unwrap();
        let mismatched = Direction::new(vec![1.0, 0.0]).unwrap();

        assert!(matches!(
            state.deposit_building("wrong dimension", mismatched),
            Err(V2Error::DimensionMismatch { .. })
        ));
        assert!(state.events.is_empty());
        assert_eq!(state.next_event_id, EventId(1));
        assert_eq!(state.next_step_id, StepId(1));
    }

    #[test]
    fn active_log_requires_phase_order_cardinality_and_coordinate_chain() {
        let mut state =
            FieldState::new_building(FieldVersion::physics_reference_s2(4).unwrap()).unwrap();
        let first = point(1.0, 0.0, 0.0);
        state.deposit_building("first", first.clone()).unwrap();
        state.lifecycle = Lifecycle::Active;

        let intermediate = point(0.0, 1.0, 0.0);
        let final_coordinate = point(0.0, 0.0, 1.0);
        state.coordinate_log = vec![
            CoordinateLogEntry::new(
                StepId(1),
                EventId(1),
                CoordinatePhase::Natural,
                first,
                intermediate.clone(),
            )
            .unwrap(),
            CoordinateLogEntry::new(
                StepId(1),
                EventId(1),
                CoordinatePhase::External,
                intermediate,
                final_coordinate.clone(),
            )
            .unwrap(),
        ];
        state.events[0].coordinate = final_coordinate;
        state
            .events
            .push(Event::new(EventId(2), "new".into(), point(-1.0, 0.0, 0.0)).unwrap());
        state.next_event_id = EventId(3);
        state.next_step_id = StepId(2);

        state.validate().unwrap();
        state.coordinate_log.swap(0, 1);
        assert!(state.validate().is_err());
    }
}
