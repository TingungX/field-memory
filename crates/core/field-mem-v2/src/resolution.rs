//! Deterministic geometry reduction and resolution scaffolding for v2.
//!
//! This module owns only the EventCoordinate -> GeometryUnit -> DensitySite
//! portion of the contract.  It deliberately does not approximate the
//! residual-greedy Sample projection: until that projector exists every scale
//! is recorded as unavailable rather than pretending that a resolution passed.

use std::cmp::Ordering;

use crate::{
    error::{Result, V2Error},
    event::{Event, EventId, FieldState},
    geometry::Direction,
    numeric::{EPS_ANGLE, EPS_REPRESENTATION},
    version::MIN_SAMPLE_BUDGET,
};

/// The inclusive maximum `n` in the contract's `ell_n = pi * 2^-n` ladder.
pub const SCALE_LEVEL_MAX: u32 = 20;

/// A stable label assigned from the first EventId represented by a geometry unit.
pub type GeometryLabel = u64;
/// The rotation-invariant, versioned order of a geometry unit.
pub type GeometryGaugeRank = u64;
/// The position of a density-site center in a deterministic FPS prefix.
pub type SiteId = u64;

/// A fixed member of the contract's dyadic scale ladder.
///
/// There is intentionally no public constructor: callers obtain levels from
/// [`scale_ladder`] or [`ScaleLevel::from_level`], so an independent runtime
/// `ell` cannot enter the resolution path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleLevel {
    level: u32,
    ell: f64,
}

impl ScaleLevel {
    /// Returns the only contract-defined scale for `level`.
    pub fn from_level(level: u32) -> Result<Self> {
        if level > SCALE_LEVEL_MAX {
            return Err(V2Error::ResolutionInfeasible);
        }

        Ok(Self {
            level,
            ell: std::f64::consts::PI * 2.0_f64.powi(-(level as i32)),
        })
    }

    pub fn level(self) -> u32 {
        self.level
    }

    pub fn ell(self) -> f64 {
        self.ell
    }
}

/// Returns the complete, coarsest-to-finest contract scale ladder.
pub fn scale_ladder() -> Vec<ScaleLevel> {
    (0..=SCALE_LEVEL_MAX)
        .map(|level| ScaleLevel {
            level,
            ell: std::f64::consts::PI * 2.0_f64.powi(-(level as i32)),
        })
        .collect()
}

/// A representative-greedy equivalence class of currently indistinguishable
/// EventCoordinates.
#[derive(Clone, Debug)]
pub struct GeometryUnit {
    /// The first (and therefore lowest) EventId admitted to this unit.
    pub geometry_label: GeometryLabel,
    /// Rank after `(intrinsic_signature, geometry_label)` ordering.
    pub gauge_rank: GeometryGaugeRank,
    /// The first member's coordinate. It is never averaged or otherwise moved.
    pub representative: Direction,
    /// Strictly ascending EventIds assigned to this unit.
    pub member_event_ids: Vec<EventId>,
    /// Sorted distances to every GeometryUnit representative, including zero.
    pub intrinsic_signature: Vec<f64>,
}

impl GeometryUnit {
    pub fn member_count(&self) -> usize {
        self.member_event_ids.len()
    }
}

/// A selected FPS center and its unit physical mass.
#[derive(Clone, Debug)]
pub struct DensitySite {
    /// SiteId is the deterministic FPS selection position, starting at zero.
    pub id: SiteId,
    /// Gauge rank of the GeometryUnit selected as this site's center.
    pub geometry_gauge_rank: GeometryGaugeRank,
    pub direction: Direction,
    /// Contractually one for every DensitySite; never derived from member count.
    pub mu: f64,
}

/// The scale-local cluster mapping needed to scatter a site load back to every
/// represented GeometryUnit. Rows are always sorted by geometry gauge rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometrySiteAssignment {
    pub geometry_gauge_rank: GeometryGaugeRank,
    pub site_id: SiteId,
}

/// Stable candidate-key wire shape reserved for the later residual projector.
/// It remains empty in Phase 0; resolution itself must not invent one.
pub type CandidateKey = (u8, u64, u8);

/// A deterministic reason why an attempt has no passing Sample witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionFailure {
    EmptyGeometry,
    SampleProjectionUnavailable,
    /// A contract-frozen structural code reported by the Sample projector.
    /// The resolution layer preserves it verbatim for the artifact/wire row.
    ProjectionFailure(String),
}

impl ResolutionFailure {
    /// Stable artifact/hash code; do not derive this from Rust's debug names.
    pub fn code(&self) -> &str {
        match self {
            Self::EmptyGeometry => "empty_geometry",
            Self::SampleProjectionUnavailable => "sample_projection_unavailable",
            Self::ProjectionFailure(code) => code,
        }
    }
}

/// One complete scale attempt. Its site construction is always present for a
/// non-empty geometry; a projection witness is absent until `sample.rs` has
/// actually established every structural gate.
#[derive(Clone, Debug)]
pub struct ResolutionAttempt {
    pub scale: ScaleLevel,
    pub site_count: u64,
    pub sites: Vec<DensitySite>,
    pub assignments: Vec<GeometrySiteAssignment>,
    /// `None` means the full residual-greedy projector did not establish an
    /// actual first-passing prefix for this scale.
    pub n_req: Option<u64>,
    /// `None` accompanies an unavailable/failed projection; it is never a
    /// synthetic zero used to make a scale look feasible.
    pub representation_error: Option<f64>,
    pub failure: Option<ResolutionFailure>,
    /// The actual first-passing prefix key sequence, empty until projection.
    pub witness_candidate_keys: Vec<CandidateKey>,
}

impl ResolutionAttempt {
    pub fn is_passing_for(&self, sample_budget: u32) -> bool {
        let (Some(n_req), Some(representation_error)) = (self.n_req, self.representation_error)
        else {
            return false;
        };

        self.failure.is_none()
            && n_req <= u64::from(sample_budget)
            && representation_error.is_finite()
            && (0.0..=EPS_REPRESENTATION).contains(&representation_error)
    }
}

/// The selected resolution witness. Only a real Sample projection may create
/// one; Phase 0 derives no instances of this type.
#[derive(Clone, Debug)]
pub struct ResolutionWitness {
    pub scale: ScaleLevel,
    pub sites: Vec<DensitySite>,
    pub assignments: Vec<GeometrySiteAssignment>,
    pub n_req: u64,
    pub representation_error: f64,
    pub candidate_keys: Vec<CandidateKey>,
}

/// All auditable scale attempts plus the one finest feasible witness, if any.
#[derive(Clone, Debug)]
pub struct DerivedResolution {
    pub geometry_units: Vec<GeometryUnit>,
    pub sample_budget: u32,
    pub attempts: Vec<ResolutionAttempt>,
    pub chosen: Option<ResolutionWitness>,
    pub failure: Option<ResolutionFailure>,
}

impl DerivedResolution {
    /// Returns the selected G_K witness or the contract's infeasible error.
    pub fn require_witness(&self) -> Result<&ResolutionWitness> {
        self.chosen.as_ref().ok_or(V2Error::ResolutionInfeasible)
    }

    /// Selects the numerically smallest `ell` among attempts already proven by
    /// the Sample projector. The ladder is coarsest-to-finest, so the last
    /// passing attempt is the unique G_K witness.
    pub fn select_finest_passing(&mut self) {
        self.chosen = self
            .attempts
            .iter()
            .filter_map(|attempt| {
                if !attempt.is_passing_for(self.sample_budget) {
                    return None;
                }
                Some(ResolutionWitness {
                    scale: attempt.scale,
                    sites: attempt.sites.clone(),
                    assignments: attempt.assignments.clone(),
                    n_req: attempt.n_req?,
                    representation_error: attempt.representation_error?,
                    candidate_keys: attempt.witness_candidate_keys.clone(),
                })
            })
            .next_back();

        self.failure = if self.chosen.is_some() {
            None
        } else if self.geometry_units.is_empty() {
            Some(ResolutionFailure::EmptyGeometry)
        } else {
            Some(ResolutionFailure::SampleProjectionUnavailable)
        };
    }
}

/// Builds representative-greedy GeometryUnits by scanning EventIds in ascending
/// order. This deliberately does not perform transitive closure.
pub fn derive_geometry_units(events: &[Event]) -> Result<Vec<GeometryUnit>> {
    let mut ordered_events: Vec<&Event> = events.iter().collect();
    ordered_events.sort_by_key(|event| event.id.0);

    for pair in ordered_events.windows(2) {
        if pair[0].id.0 == pair[1].id.0 {
            return Err(V2Error::InvalidSampleField(
                "duplicate EventId while deriving GeometryUnit".to_owned(),
            ));
        }
    }

    let mut units: Vec<GeometryUnit> = Vec::new();
    for event in ordered_events {
        let mut matching_unit: Option<usize> = None;
        for (index, unit) in units.iter().enumerate() {
            let distance = unit.representative.angle(&event.coordinate)?;
            if distance <= EPS_ANGLE
                && matching_unit
                    .map(|current| unit.geometry_label < units[current].geometry_label)
                    .unwrap_or(true)
            {
                matching_unit = Some(index);
            }
        }

        if let Some(index) = matching_unit {
            units[index].member_event_ids.push(event.id);
        } else {
            units.push(GeometryUnit {
                geometry_label: event.id.0,
                gauge_rank: 0,
                representative: event.coordinate.clone(),
                member_event_ids: vec![event.id],
                intrinsic_signature: Vec::new(),
            });
        }
    }

    populate_intrinsic_signatures_and_gauge_ranks(&mut units)?;
    Ok(units)
}

/// Derives one deterministic DensitySite clustering from a contract scale.
pub fn derive_density_sites(
    geometry_units: &[GeometryUnit],
    scale: ScaleLevel,
) -> Result<(Vec<DensitySite>, Vec<GeometrySiteAssignment>)> {
    if geometry_units.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    validate_geometry_units(geometry_units)?;
    let selected = farthest_point_prefix(geometry_units, scale.ell())?;
    let mut sites = Vec::with_capacity(selected.len());

    for (site_index, &unit_index) in selected.iter().enumerate() {
        let unit = &geometry_units[unit_index];
        sites.push(DensitySite {
            id: site_index as SiteId,
            geometry_gauge_rank: unit.gauge_rank,
            direction: unit.representative.clone(),
            mu: 1.0,
        });
    }

    let mut assignments = Vec::with_capacity(geometry_units.len());
    for unit in geometry_units {
        let chosen_site_index = nearest_site_index(unit, geometry_units, &selected)?;
        assignments.push(GeometrySiteAssignment {
            geometry_gauge_rank: unit.gauge_rank,
            site_id: chosen_site_index as SiteId,
        });
    }

    assignments.sort_by_key(|assignment| assignment.geometry_gauge_rank);
    Ok((sites, assignments))
}

/// Derives all geometry/site attempts for a FieldState. Phase 0 intentionally
/// records every non-empty attempt as projection-unavailable.
pub fn derive_resolution(state: &FieldState) -> Result<DerivedResolution> {
    state.validate()?;
    derive_resolution_for_events(&state.events, state.version.sample_budget)
}

/// Event-slice entry point used by construction and tests before a FieldState
/// is persisted. It has the same G_K semantics as [`derive_resolution`].
pub fn derive_resolution_for_events(
    events: &[Event],
    sample_budget: u32,
) -> Result<DerivedResolution> {
    if sample_budget < MIN_SAMPLE_BUDGET {
        return Err(V2Error::InvalidSampleBudget);
    }
    let geometry_units = derive_geometry_units(events)?;
    if geometry_units.is_empty() {
        return Ok(DerivedResolution {
            geometry_units,
            sample_budget,
            attempts: Vec::new(),
            chosen: None,
            failure: Some(ResolutionFailure::EmptyGeometry),
        });
    }

    let mut attempts = Vec::with_capacity((SCALE_LEVEL_MAX + 1) as usize);
    for scale in scale_ladder() {
        let (sites, assignments) = derive_density_sites(&geometry_units, scale)?;
        attempts.push(ResolutionAttempt {
            scale,
            site_count: sites.len() as u64,
            sites,
            assignments,
            n_req: None,
            representation_error: None,
            failure: Some(ResolutionFailure::SampleProjectionUnavailable),
            witness_candidate_keys: Vec::new(),
        });
    }

    Ok(DerivedResolution {
        geometry_units,
        sample_budget,
        attempts,
        chosen: None,
        failure: Some(ResolutionFailure::SampleProjectionUnavailable),
    })
}

fn populate_intrinsic_signatures_and_gauge_ranks(units: &mut [GeometryUnit]) -> Result<()> {
    for unit_index in 0..units.len() {
        let mut signature = Vec::with_capacity(units.len());
        for other in units.iter() {
            signature.push(
                units[unit_index]
                    .representative
                    .angle(&other.representative)?,
            );
        }
        signature.sort_by(f64::total_cmp);
        units[unit_index].intrinsic_signature = signature;
    }

    units.sort_by(compare_geometry_unit_order);
    for (rank, unit) in units.iter_mut().enumerate() {
        unit.gauge_rank = rank as GeometryGaugeRank;
    }
    Ok(())
}

fn validate_geometry_units(geometry_units: &[GeometryUnit]) -> Result<()> {
    for (expected_rank, unit) in geometry_units.iter().enumerate() {
        if unit.gauge_rank != expected_rank as GeometryGaugeRank {
            return Err(V2Error::InvalidSampleField(
                "GeometryUnit gauge ranks must be contiguous and ordered".to_owned(),
            ));
        }
        if unit.intrinsic_signature.len() != geometry_units.len() {
            return Err(V2Error::InvalidSampleField(
                "GeometryUnit intrinsic signature has the wrong cardinality".to_owned(),
            ));
        }
    }
    Ok(())
}

fn farthest_point_prefix(geometry_units: &[GeometryUnit], ell: f64) -> Result<Vec<usize>> {
    debug_assert!(!geometry_units.is_empty());
    let mut selected = vec![0_usize];

    loop {
        let mut best_index: Option<usize> = None;
        let mut best_distance = f64::NEG_INFINITY;

        for candidate_index in 0..geometry_units.len() {
            if selected.contains(&candidate_index) {
                continue;
            }

            let minimum_distance =
                selected
                    .iter()
                    .try_fold(f64::INFINITY, |minimum, &center| {
                        Ok::<_, V2Error>(
                            minimum.min(
                                geometry_units[candidate_index]
                                    .representative
                                    .angle(&geometry_units[center].representative)?,
                            ),
                        )
                    })?;

            let replaces_best = minimum_distance > best_distance + EPS_ANGLE
                || ((minimum_distance - best_distance).abs() <= EPS_ANGLE
                    && best_index
                        .map(|current| {
                            compare_geometry_unit_order(
                                &geometry_units[candidate_index],
                                &geometry_units[current],
                            ) == Ordering::Less
                        })
                        .unwrap_or(true));

            if replaces_best {
                best_index = Some(candidate_index);
                best_distance = minimum_distance;
            }
        }

        let Some(next) = best_index else {
            break;
        };
        if best_distance <= ell {
            break;
        }
        selected.push(next);
    }

    Ok(selected)
}

fn nearest_site_index(
    unit: &GeometryUnit,
    geometry_units: &[GeometryUnit],
    selected: &[usize],
) -> Result<usize> {
    let mut nearest: Option<usize> = None;
    let mut nearest_distance = f64::INFINITY;

    for (site_index, &center_index) in selected.iter().enumerate() {
        let distance = unit
            .representative
            .angle(&geometry_units[center_index].representative)?;
        let replaces_nearest = distance < nearest_distance - EPS_ANGLE
            || ((distance - nearest_distance).abs() <= EPS_ANGLE
                && nearest
                    .map(|current_site_index| {
                        let current_center = selected[current_site_index];
                        compare_geometry_unit_order(
                            &geometry_units[center_index],
                            &geometry_units[current_center],
                        ) == Ordering::Less
                    })
                    .unwrap_or(true));

        if replaces_nearest {
            nearest = Some(site_index);
            nearest_distance = distance;
        }
    }

    nearest.ok_or_else(|| {
        V2Error::InvalidSampleField("DensitySite construction produced no center".to_owned())
    })
}

/// The contract's EPS-aware lexicographic comparison of intrinsic signatures,
/// followed only by the stable geometry-label gauge.
fn compare_geometry_unit_order(left: &GeometryUnit, right: &GeometryUnit) -> Ordering {
    for (&left_value, &right_value) in left
        .intrinsic_signature
        .iter()
        .zip(right.intrinsic_signature.iter())
    {
        if (left_value - right_value).abs() > EPS_ANGLE {
            return left_value.total_cmp(&right_value);
        }
    }

    left.geometry_label.cmp(&right.geometry_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direction(values: &[f64]) -> Direction {
        Direction::new(values.to_vec()).expect("test coordinate is valid")
    }

    fn event(id: u64, coordinate: &[f64]) -> Event {
        Event {
            id: EventId(id),
            content: format!("event-{id}"),
            coordinate: direction(coordinate),
        }
    }

    #[test]
    fn duplicate_content_coordinate_is_one_geometry_unit_and_not_weight() {
        let events = vec![
            event(3, &[0.0, 1.0, 0.0]),
            event(1, &[1.0, 0.0, 0.0]),
            event(2, &[1.0, 0.0, 0.0]),
        ];

        let units = derive_geometry_units(&events).expect("geometry derives");
        assert_eq!(units.len(), 2);
        let duplicate_unit = units
            .iter()
            .find(|unit| unit.geometry_label == 1)
            .expect("first coordinate remains the label gauge");
        assert_eq!(
            duplicate_unit.member_event_ids,
            vec![EventId(1), EventId(2)]
        );

        let fine = ScaleLevel::from_level(2).expect("contract level");
        let (sites, assignments) = derive_density_sites(&units, fine).expect("sites derive");
        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|site| site.mu == 1.0));
        assert_eq!(assignments.len(), 2);
    }

    #[test]
    fn symmetric_signature_uses_event_label_only_as_gauge_tie_break() {
        let events = vec![
            event(9, &[1.0, 0.0, 0.0]),
            event(3, &[-1.0, 0.0, 0.0]),
            event(7, &[0.0, 1.0, 0.0]),
            event(5, &[0.0, -1.0, 0.0]),
        ];

        let units = derive_geometry_units(&events).expect("geometry derives");
        let ranks: Vec<_> = units
            .iter()
            .map(|unit| (unit.gauge_rank, unit.geometry_label))
            .collect();
        assert_eq!(ranks, vec![(0, 3), (1, 5), (2, 7), (3, 9)]);

        let (sites, _) =
            derive_density_sites(&units, ScaleLevel::from_level(20).expect("contract level"))
                .expect("sites derive");
        assert_eq!(sites[0].geometry_gauge_rank, 0);
    }

    #[test]
    fn fps_site_order_and_assignments_are_rotation_invariant() {
        let original = vec![
            event(1, &[1.0, 0.0, 0.0]),
            event(2, &[0.0, 1.0, 0.0]),
            event(3, &[0.0, 0.0, 1.0]),
        ];
        // A rigid +90 degree rotation around z: (x, y, z) -> (-y, x, z).
        let rotated = vec![
            event(1, &[0.0, 1.0, 0.0]),
            event(2, &[-1.0, 0.0, 0.0]),
            event(3, &[0.0, 0.0, 1.0]),
        ];

        let original_units = derive_geometry_units(&original).expect("geometry derives");
        let rotated_units = derive_geometry_units(&rotated).expect("geometry derives");
        let scale = ScaleLevel::from_level(2).expect("contract level");
        let (original_sites, original_assignments) =
            derive_density_sites(&original_units, scale).expect("sites derive");
        let (rotated_sites, rotated_assignments) =
            derive_density_sites(&rotated_units, scale).expect("sites derive");

        assert_eq!(
            original_sites
                .iter()
                .map(|site| (site.id, site.geometry_gauge_rank))
                .collect::<Vec<_>>(),
            rotated_sites
                .iter()
                .map(|site| (site.id, site.geometry_gauge_rank))
                .collect::<Vec<_>>(),
        );
        assert_eq!(original_assignments, rotated_assignments);
    }

    #[test]
    fn phase_zero_records_each_scale_as_projection_unavailable() {
        let events = vec![event(1, &[1.0, 0.0, 0.0]), event(2, &[0.0, 1.0, 0.0])];
        let derived = derive_resolution_for_events(&events, 64).expect("resolution derives");

        assert_eq!(derived.attempts.len(), (SCALE_LEVEL_MAX + 1) as usize);
        assert!(derived.chosen.is_none());
        assert_eq!(
            derived.failure,
            Some(ResolutionFailure::SampleProjectionUnavailable)
        );
        assert!(derived
            .attempts
            .iter()
            .all(|attempt| attempt.n_req.is_none()
                && attempt.representation_error.is_none()
                && attempt.failure == Some(ResolutionFailure::SampleProjectionUnavailable)));
        assert!(matches!(
            derived.require_witness(),
            Err(V2Error::ResolutionInfeasible)
        ));
    }

    #[test]
    fn direct_resolution_entry_rejects_a_non_versioned_budget() {
        let events = vec![event(1, &[1.0, 0.0, 0.0])];
        assert!(matches!(
            derive_resolution_for_events(&events, 3),
            Err(V2Error::InvalidSampleBudget)
        ));
    }

    #[test]
    fn scale_ladder_is_exactly_the_contract_dyadic_sequence() {
        let ladder = scale_ladder();
        assert_eq!(ladder.len(), (SCALE_LEVEL_MAX + 1) as usize);
        assert_eq!(ladder[0].ell(), std::f64::consts::PI);
        assert_eq!(
            ladder[SCALE_LEVEL_MAX as usize].ell(),
            std::f64::consts::PI * 2.0_f64.powi(-(SCALE_LEVEL_MAX as i32)),
        );
    }
}
