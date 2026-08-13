//! Density-only Sample projection for both v2 backends.
//!
//! This module stops at a frozen `SampleField`.  Transport and Response consume
//! that field later; they must not call back into the `DensitySite` or Event
//! representation.  Both backends share the residual-greedy candidate loop;
//! only the prefix integrals differ (semantic finite controls vs S² cubature).

use std::cmp::Ordering;

use crate::{
    error::{Result, V2Error},
    geometry::Direction,
    kernel::{
        integrate_s2_adaptive, normalize_s2, semantic_column_weights, S2CubatureCell,
        S2CubatureLeaf, S2Normalization, S2SupportCap, WendlandC2,
    },
    numeric::{
        log_sum_exp, KahanSum, CUT_LOCUS_MARGIN_RAD, EPS_ABS, EPS_ANGLE, EPS_QUADRATURE, EPS_REL,
        EPS_REPRESENTATION, SIGMA,
    },
    resolution::{CandidateKey, DensitySite, ScaleLevel},
    version::{BackendKind, FieldVersion, SpaceKind},
};

/// Stable role tags used by the canonical Sample wire representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SampleRole {
    Carrier = 0,
    Physical = 1,
    CarrierPhysical = 2,
}

impl SampleRole {
    pub const fn wire_tag(self) -> u8 {
        self as u8
    }

    fn is_physical(self) -> bool {
        matches!(self, Self::Physical | Self::CarrierPhysical)
    }

    fn has_broad_carrier_profile(self) -> bool {
        matches!(self, Self::Carrier | Self::CarrierPhysical)
    }
}

/// One frozen Sample node.  `physical_candidate_key` is provenance for a
/// CarrierPhysical promotion and does not affect the Sample wire identity.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub id: u64,
    pub candidate_key: CandidateKey,
    pub physical_candidate_key: Option<CandidateKey>,
    pub role: SampleRole,
    pub direction: Direction,
    pub transport_radius: f64,
    pub transport_volume: f64,
    pub physical_volume: Option<f64>,
    pub mass: Option<f64>,
    pub density: Option<f64>,
}

/// A finite graph edge.  Endpoints are always stored in ascending SampleId
/// order, independently of the order in which the pair was inspected.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEdge {
    pub low_sample_id: u64,
    pub high_sample_id: u64,
    pub overlap: f64,
    pub conductance: f64,
}

/// A hash-ready finite operational SampleField.
///
/// All matrices use rows in ascending SampleId (or physical SampleId where
/// documented) and columns in ascending DensitySiteId/control order:
/// `phi`/`chi` are `sample x control`, the semantic operational kernel is a
/// deterministic lazy `control x site` column stream, `coupling` is `physical
/// sample x site`, and `absorption` is `sample owner x physical absorber`.
#[derive(Clone, Debug)]
pub struct SampleField {
    pub version: FieldVersion,
    pub scale: ScaleLevel,
    pub sites: Vec<DensitySite>,
    pub samples: Vec<Sample>,
    pub phi: Vec<Vec<f64>>,
    pub chi: Vec<Vec<f64>>,
    /// Lazy semantic columns.  The MxM matrix is never retained as runtime
    /// truth; callers/hashers can deterministically stream one source column.
    pub operational_kernel: Option<SemanticOperationalKernel>,
    pub coupling: Vec<Vec<f64>>,
    pub graph_edges: Vec<GraphEdge>,
    pub absorption: Vec<Vec<f64>>,
    pub density_values: Vec<f64>,
    pub probability_values: Vec<f64>,
    pub reconstructed_probability: Vec<f64>,
    pub uncovered_physical_mass: f64,
    pub representation_error: f64,
    pub structural_pass: bool,
    pub structural_failure: Option<String>,
    pub candidate_keys: Vec<CandidateKey>,
    /// Frozen S² adaptive-cubature leaves.  Semantic fields store `None`.
    pub cubature_leaves: Option<Vec<S2CubatureLeaf>>,
}

/// Distinguishes a backend that is not implemented from a finite projection
/// that exhausted its deterministic candidate pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionStatus {
    Available,
    Infeasible,
    Unavailable,
}

/// One complete, auditable attempt at a single contract scale.
#[derive(Clone, Debug)]
pub struct SampleProjectionOutcome {
    pub scale: ScaleLevel,
    pub status: ProjectionStatus,
    /// `None` means no first-passing prefix exists (or the backend is
    /// unavailable); it is not a synthetic zero.
    pub n_req: Option<u64>,
    pub representation_error: Option<f64>,
    pub failure_code: Option<String>,
    pub witness_candidate_keys: Vec<CandidateKey>,
    pub sample_field: Option<SampleField>,
}

impl SampleProjectionOutcome {
    pub fn is_passing_for(&self, sample_budget: u32) -> bool {
        self.status == ProjectionStatus::Available
            && self
                .n_req
                .map(|value| value <= u64::from(sample_budget))
                .unwrap_or(false)
            && self
                .representation_error
                .map(|value| value.is_finite() && (0.0..=EPS_REPRESENTATION).contains(&value))
                .unwrap_or(false)
            && self.failure_code.is_none()
            && self.sample_field.is_some()
    }
}

/// Project one scale using the backend selected by `version`.
pub fn project_sample_field(
    version: &FieldVersion,
    scale: ScaleLevel,
    sites: &[DensitySite],
) -> Result<SampleProjectionOutcome> {
    version.validate()?;
    match (version.backend_kind, version.space_kind) {
        (BackendKind::SemanticProduction, SpaceKind::SemanticProduction384) => {
            project_semantic_sample_field(version, scale, sites)
        }
        (BackendKind::PhysicsReference, SpaceKind::PhysicsReferenceS2) => {
            project_s2_sample_field(version, scale, sites)
        }
        _ => Err(V2Error::InvalidVersion(
            "backend and space identities are an invalid combination".to_owned(),
        )),
    }
}

/// Semantic-production specialization, exposed so future resolution wiring
/// can call it without re-dispatching on the backend enum.
pub fn project_semantic_sample_field(
    version: &FieldVersion,
    scale: ScaleLevel,
    sites: &[DensitySite],
) -> Result<SampleProjectionOutcome> {
    if version.backend_kind != BackendKind::SemanticProduction
        || version.space_kind != SpaceKind::SemanticProduction384
    {
        return Err(V2Error::InvalidVersion(
            "semantic projection requires semantic_production_384".to_owned(),
        ));
    }
    validate_sites(version, sites)?;
    if sites.is_empty() {
        return Ok(infeasible_outcome(scale, "empty_density_sites"));
    }

    let density = SemanticDensity::build(scale, sites)?;
    match run_residual_greedy(scale, sites, |nodes| {
        evaluate_semantic_prefix(scale, &density, nodes)
    })? {
        GreedyResult::Outcome(outcome) => Ok(outcome),
        GreedyResult::Passed {
            evaluation,
            selected_keys,
        } => {
            let sample_field = evaluation.into_semantic_sample_field(
                version,
                scale,
                &density,
                selected_keys.clone(),
            )?;
            available_outcome(scale, selected_keys, sample_field)
        }
    }
}

/// Continuous S² specialization.  Uses the same residual-greedy loop as the
/// semantic backend; prefix integrals come from adaptive cube-face cubature.
pub fn project_s2_sample_field(
    version: &FieldVersion,
    scale: ScaleLevel,
    sites: &[DensitySite],
) -> Result<SampleProjectionOutcome> {
    if version.backend_kind != BackendKind::PhysicsReference
        || version.space_kind != SpaceKind::PhysicsReferenceS2
    {
        return Err(V2Error::InvalidVersion(
            "S2 projection requires physics_reference_s2".to_owned(),
        ));
    }
    validate_sites(version, sites)?;
    if sites.is_empty() {
        return Ok(infeasible_outcome(scale, "empty_density_sites"));
    }

    let density = S2Density::build(scale, sites)?;
    match run_residual_greedy(scale, sites, |nodes| {
        evaluate_s2_prefix(scale, &density, nodes)
    })? {
        GreedyResult::Outcome(outcome) => Ok(outcome),
        GreedyResult::Passed {
            evaluation,
            selected_keys,
        } => {
            let sample_field = evaluation.into_s2_sample_field(
                version,
                scale,
                &density,
                selected_keys.clone(),
            )?;
            available_outcome(scale, selected_keys, sample_field)
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum GreedyResult {
    Outcome(SampleProjectionOutcome),
    Passed {
        evaluation: PrefixEvaluation,
        selected_keys: Vec<CandidateKey>,
    },
}

fn available_outcome(
    scale: ScaleLevel,
    selected_keys: Vec<CandidateKey>,
    sample_field: SampleField,
) -> Result<SampleProjectionOutcome> {
    let n_req = u64::try_from(sample_field.samples.len())
        .map_err(|_| V2Error::InvalidSampleField("Sample node count exceeds u64".to_owned()))?;
    Ok(SampleProjectionOutcome {
        scale,
        status: ProjectionStatus::Available,
        n_req: Some(n_req),
        representation_error: Some(sample_field.representation_error),
        failure_code: None,
        witness_candidate_keys: selected_keys,
        sample_field: Some(sample_field),
    })
}

fn run_residual_greedy(
    scale: ScaleLevel,
    sites: &[DensitySite],
    mut evaluate_prefix: impl FnMut(&mut [Sample]) -> Result<PrefixEvaluation>,
) -> Result<GreedyResult> {
    let candidates = build_candidates(sites)?;
    let mut nodes = vec![candidates[0].as_carrier(0), candidates[1].as_carrier(1)];
    let mut selected_keys = vec![candidates[0].key, candidates[1].key];
    let mut remaining: Vec<Candidate> = candidates.into_iter().skip(2).collect();

    loop {
        let mut best: Option<(Candidate, PrefixEvaluation)> = None;
        for candidate in &remaining {
            let Some(mut operation_nodes) = apply_candidate(&nodes, candidate, scale.ell())? else {
                // No-op physical duplicates cannot improve a prefix and must
                // not be selected merely to consume a candidate key.
                continue;
            };
            let evaluation = evaluate_prefix(&mut operation_nodes)?;
            if !evaluation.candidate_score_valid {
                continue;
            }
            let replaces = best
                .as_ref()
                .map(|(current, current_eval)| {
                    compare_score(
                        evaluation.uncovered_physical_mass,
                        evaluation.representation_error,
                        candidate.key,
                        current_eval.uncovered_physical_mass,
                        current_eval.representation_error,
                        current.key,
                    ) == Ordering::Less
                })
                .unwrap_or(true);
            if replaces {
                best = Some((candidate.clone(), evaluation));
            }
        }

        let Some((candidate, evaluation)) = best else {
            return Ok(GreedyResult::Outcome(SampleProjectionOutcome {
                scale,
                status: ProjectionStatus::Infeasible,
                n_req: None,
                representation_error: None,
                failure_code: Some("projection_candidate_exhausted".to_owned()),
                witness_candidate_keys: selected_keys,
                sample_field: None,
            }));
        };

        let operation_key = candidate.key;
        nodes = evaluation.nodes.clone();
        selected_keys.push(operation_key);
        remaining.retain(|candidate| candidate.key != operation_key);

        if evaluation.structural_pass
            && evaluation.uncovered_physical_mass == 0.0
            && evaluation.representation_error <= EPS_REPRESENTATION
        {
            return Ok(GreedyResult::Passed {
                evaluation,
                selected_keys,
            });
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    key: CandidateKey,
    /// Key/direction of the first representative in the full candidate pool
    /// whose angular distance to this candidate is <= EPS_ANGLE.
    representative_key: CandidateKey,
    representative_direction: Direction,
    direction: Direction,
    broad_carrier: bool,
    physical: bool,
}

impl Candidate {
    fn as_carrier(&self, id: u64) -> Sample {
        Sample {
            id,
            candidate_key: self.representative_key,
            physical_candidate_key: None,
            role: SampleRole::Carrier,
            direction: self.representative_direction.clone(),
            transport_radius: crate::numeric::CARRIER_RADIUS_RAD,
            transport_volume: 0.0,
            physical_volume: None,
            mass: None,
            density: None,
        }
    }
}

#[derive(Clone, Debug)]
struct SemanticDensity {
    sites: Vec<DensitySite>,
    operational_kernel: SemanticOperationalKernel,
    density_values: Vec<f64>,
    probability_values: Vec<f64>,
    omega: f64,
    total_mass: f64,
}

/// Finite semantic kernel identity with deterministic column evaluation.
/// This is deliberately a column stream rather than a materialized MxM
/// allocation, while preserving the exact `K_ai` contract formula.
#[derive(Clone, Debug)]
pub struct SemanticOperationalKernel {
    pub scale: ScaleLevel,
    pub sites: Vec<DensitySite>,
}

impl SemanticOperationalKernel {
    pub fn column(&self, source_index: usize) -> Result<Vec<f64>> {
        let source = self.sites.get(source_index).ok_or_else(|| {
            V2Error::InvalidSampleField("semantic kernel source index out of bounds".to_owned())
        })?;
        let logs = self
            .sites
            .iter()
            .map(|control| {
                let theta = source.direction.angle(&control.direction)?;
                WendlandC2.log_profile(theta / self.scale.ell())
            })
            .collect::<Result<Vec<_>>>()?;
        let weights = semantic_column_weights(logs)?;
        validate_partition(&weights, "semantic kernel column")?;
        Ok(weights)
    }
}

impl SemanticDensity {
    fn build(scale: ScaleLevel, sites: &[DensitySite]) -> Result<Self> {
        let count = sites.len();
        let omega = 1.0 / count as f64;
        if !omega.is_finite() || omega <= 0.0 {
            return Err(V2Error::InvalidSampleField(
                "semantic control weight is non-finite".to_owned(),
            ));
        }

        let kernel = SemanticOperationalKernel {
            scale,
            sites: sites.to_vec(),
        };
        let mut density_sums = vec![KahanSum::default(); count];
        for (source_index, source_site) in sites.iter().enumerate() {
            let column = kernel.column(source_index)?;
            for (density_sum, weight) in density_sums.iter_mut().zip(column) {
                density_sum.add(source_site.mu * weight);
            }
        }
        let mut density_values = density_sums
            .into_iter()
            .map(KahanSum::total)
            .collect::<Vec<_>>();
        for density in &mut density_values {
            *density = finite_nonnegative(*density, "semantic density control")?;
        }
        let mut total_mass_sum = KahanSum::default();
        for site in sites {
            if !site.mu.is_finite() || site.mu <= 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "DensitySite mass must be finite and positive".to_owned(),
                ));
            }
            total_mass_sum.add(site.mu);
        }
        let total_mass = finite_positive(total_mass_sum.total(), "semantic total mass")?;
        let probability_values = density_values
            .iter()
            .map(|value| {
                let probability = value / (total_mass * omega);
                if !probability.is_finite() || probability < 0.0 {
                    return Err(V2Error::InvalidSampleField(
                        "semantic probability control is non-finite".to_owned(),
                    ));
                }
                Ok(probability)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            sites: sites.to_vec(),
            operational_kernel: kernel,
            density_values,
            probability_values,
            omega,
            total_mass,
        })
    }
}

#[derive(Clone, Debug)]
struct PrefixEvaluation {
    nodes: Vec<Sample>,
    phi: Vec<Vec<f64>>,
    chi: Vec<Vec<f64>>,
    coupling: Vec<Vec<f64>>,
    graph_edges: Vec<GraphEdge>,
    absorption: Vec<Vec<f64>>,
    transport_volumes: Vec<f64>,
    physical_volumes: Vec<Option<f64>>,
    masses: Vec<Option<f64>>,
    densities: Vec<Option<f64>>,
    reconstructed_probability: Vec<f64>,
    uncovered_physical_mass: f64,
    representation_error: f64,
    structural_pass: bool,
    structural_failure: Option<String>,
    candidate_score_valid: bool,
    cubature_leaves: Option<Vec<S2CubatureLeaf>>,
}

impl PrefixEvaluation {
    fn freeze_samples(&mut self) -> Result<Vec<Sample>> {
        let mut samples = std::mem::take(&mut self.nodes);
        for (sample_index, sample) in samples.iter_mut().enumerate() {
            sample.id = u64::try_from(sample_index)
                .map_err(|_| V2Error::InvalidSampleField("SampleId exceeds u64".to_owned()))?;
            sample.transport_volume = self.transport_volumes[sample_index];
            sample.physical_volume = self.physical_volumes[sample_index];
            sample.mass = self.masses[sample_index];
            sample.density = self.densities[sample_index];
        }
        Ok(samples)
    }

    fn into_semantic_sample_field(
        mut self,
        version: &FieldVersion,
        scale: ScaleLevel,
        density: &SemanticDensity,
        candidate_keys: Vec<CandidateKey>,
    ) -> Result<SampleField> {
        if !self.structural_pass {
            return Err(V2Error::InvalidSampleField(
                "cannot freeze a structurally invalid SampleField".to_owned(),
            ));
        }
        let samples = self.freeze_samples()?;
        Ok(SampleField {
            version: version.clone(),
            scale,
            sites: density.sites.clone(),
            samples,
            phi: self.phi,
            chi: self.chi,
            operational_kernel: Some(density.operational_kernel.clone()),
            coupling: self.coupling,
            graph_edges: self.graph_edges,
            absorption: self.absorption,
            density_values: density.density_values.clone(),
            probability_values: density.probability_values.clone(),
            reconstructed_probability: self.reconstructed_probability,
            uncovered_physical_mass: self.uncovered_physical_mass,
            representation_error: self.representation_error,
            structural_pass: self.structural_pass,
            structural_failure: self.structural_failure,
            candidate_keys,
            cubature_leaves: None,
        })
    }

    fn into_s2_sample_field(
        mut self,
        version: &FieldVersion,
        scale: ScaleLevel,
        density: &S2Density,
        candidate_keys: Vec<CandidateKey>,
    ) -> Result<SampleField> {
        if !self.structural_pass {
            return Err(V2Error::InvalidSampleField(
                "cannot freeze a structurally invalid SampleField".to_owned(),
            ));
        }
        let samples = self.freeze_samples()?;
        Ok(SampleField {
            version: version.clone(),
            scale,
            sites: density.sites.clone(),
            samples,
            phi: self.phi,
            chi: self.chi,
            operational_kernel: None,
            coupling: self.coupling,
            graph_edges: self.graph_edges,
            absorption: self.absorption,
            density_values: Vec::new(),
            probability_values: Vec::new(),
            reconstructed_probability: self.reconstructed_probability,
            uncovered_physical_mass: self.uncovered_physical_mass,
            representation_error: self.representation_error,
            structural_pass: self.structural_pass,
            structural_failure: self.structural_failure,
            candidate_keys,
            cubature_leaves: self.cubature_leaves,
        })
    }
}

fn evaluate_semantic_prefix(
    scale: ScaleLevel,
    density: &SemanticDensity,
    nodes: &mut [Sample],
) -> Result<PrefixEvaluation> {
    if nodes.is_empty() {
        return Err(V2Error::InvalidSampleField(
            "Sample prefix cannot be empty".to_owned(),
        ));
    }
    let sample_count = nodes.len();
    let control_count = density.sites.len();
    let omega = density.omega;

    let mut phi = vec![vec![0.0; control_count]; sample_count];
    for (control_index, control_site) in density.sites.iter().enumerate() {
        let logs = nodes
            .iter()
            .map(|sample| {
                if !sample.transport_radius.is_finite() || sample.transport_radius <= 0.0 {
                    return Err(V2Error::InvalidSampleField(
                        "transport radius must be finite and positive".to_owned(),
                    ));
                }
                let normalized =
                    control_site.direction.angle(&sample.direction)? / sample.transport_radius;
                WendlandC2.log_profile(normalized)
            })
            .collect::<Result<Vec<_>>>()?;
        let weights = semantic_column_weights(logs)?;
        validate_partition(&weights, "transport responsibility")?;
        let weights = weights.into_iter();
        for (sample_row, weight) in phi.iter_mut().zip(weights) {
            sample_row[control_index] = weight;
        }
    }

    let mut transport_volumes = vec![0.0; sample_count];
    for sample_index in 0..sample_count {
        let mut sum = KahanSum::default();
        for weight in phi[sample_index].iter().take(control_count) {
            sum.add(omega * *weight);
        }
        transport_volumes[sample_index] = finite_nonnegative(sum.total(), "transport volume")?;
    }
    let total_transport_volume = kahan_sum_finite(&transport_volumes, "transport volumes")?;

    let physical_indices: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| sample.role.is_physical().then_some(index))
        .collect();
    let mut chi = vec![vec![0.0; control_count]; sample_count];
    let mut uncovered_physical_mass = 0.0;
    for (control_index, control_site) in density.sites.iter().enumerate() {
        let logs = physical_indices
            .iter()
            .map(|&sample_index| {
                let normalized = control_site
                    .direction
                    .angle(&nodes[sample_index].direction)?
                    / (2.0 * scale.ell());
                WendlandC2.log_profile(normalized)
            })
            .collect::<Result<Vec<_>>>()?;
        let Some(weights) = normalized_profile_weights(logs)? else {
            uncovered_physical_mass += omega * density.density_values[control_index];
            continue;
        };
        for (physical_position, &sample_index) in physical_indices.iter().enumerate() {
            chi[sample_index][control_index] = weights[physical_position];
        }
    }
    let total_mass = density.total_mass;

    let mut physical_volumes = vec![None; sample_count];
    for &sample_index in &physical_indices {
        let mut sum = KahanSum::default();
        for weight in chi[sample_index].iter().take(control_count) {
            sum.add(omega * *weight);
        }
        let value = sum.total();
        if !value.is_finite() || value < 0.0 {
            return Err(V2Error::InvalidSampleField(
                "physical responsibility volume is non-finite".to_owned(),
            ));
        }
        physical_volumes[sample_index] = Some(value);
    }

    let mut coupling = vec![vec![0.0; control_count]; physical_indices.len()];
    let mut masses = vec![None; sample_count];
    let mut densities = vec![None; sample_count];
    for (physical_position, &sample_index) in physical_indices.iter().enumerate() {
        for (site_index, target) in coupling[physical_position].iter_mut().enumerate() {
            let mut sum = KahanSum::default();
            let kernel_column = density.operational_kernel.column(site_index)?;
            for (chi_weight, kernel_weight) in chi[sample_index].iter().zip(kernel_column) {
                sum.add(*chi_weight * kernel_weight);
            }
            *target = density.sites[site_index].mu * sum.total();
        }
        let mass = kahan_sum_finite(&coupling[physical_position], "physical sample mass")?;
        masses[sample_index] = Some(mass);
        let volume = physical_volumes[sample_index]
            .ok_or_else(|| V2Error::InvalidSampleField("physical volume disappeared".to_owned()))?;
        if volume > 0.0 {
            densities[sample_index] = Some(mass / volume);
        }
    }

    let mut reconstructed_probability = vec![0.0; control_count];
    for domain_index in 0..control_count {
        let mut sum = KahanSum::default();
        for &sample_index in &physical_indices {
            let density_value = densities[sample_index].unwrap_or(0.0);
            sum.add(density_value * chi[sample_index][domain_index]);
        }
        reconstructed_probability[domain_index] = sum.total() / total_mass;
    }
    let mut representation_error_sum = KahanSum::default();
    for (target, reconstructed) in density
        .probability_values
        .iter()
        .zip(&reconstructed_probability)
    {
        representation_error_sum.add(omega * (target - reconstructed).abs());
    }
    let representation_error = 0.5 * representation_error_sum.total();

    let mut graph_edges = Vec::new();
    let mut graph_adjacency = vec![Vec::new(); sample_count];
    let mut graph_cut_locus = false;
    for low in 0..sample_count {
        for high in (low + 1)..sample_count {
            let mut overlap_sum = KahanSum::default();
            for (low_weight, high_weight) in phi[low].iter().zip(&phi[high]) {
                overlap_sum.add(omega * low_weight * high_weight);
            }
            let overlap = overlap_sum.total();
            if !overlap.is_finite() || overlap < 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "graph overlap is non-finite".to_owned(),
                ));
            }
            if overlap <= EPS_ABS {
                continue;
            }
            let separation = nodes[low].direction.angle(&nodes[high].direction)?;
            if separation <= EPS_ANGLE
                || separation >= std::f64::consts::PI - crate::numeric::CUT_LOCUS_MARGIN_RAD
            {
                graph_cut_locus = true;
                continue;
            }
            let conductance = overlap / separation;
            if !conductance.is_finite() || conductance <= 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "graph conductance is non-finite".to_owned(),
                ));
            }
            graph_edges.push(GraphEdge {
                low_sample_id: low as u64,
                high_sample_id: high as u64,
                overlap,
                conductance,
            });
            graph_adjacency[low].push(high);
            graph_adjacency[high].push(low);
        }
    }
    let graph_connected = graph_is_connected(&graph_adjacency);

    let mut absorption = vec![vec![0.0; physical_indices.len()]; sample_count];
    for owner in 0..sample_count {
        for (absorber_position, &absorber) in physical_indices.iter().enumerate() {
            let mut overlap = KahanSum::default();
            for domain_index in 0..control_count {
                overlap.add(omega * phi[owner][domain_index] * chi[absorber][domain_index]);
            }
            let rho = densities[absorber].unwrap_or(0.0);
            let value = SIGMA * rho * overlap.total();
            if !value.is_finite() || value < 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "absorption coefficient is non-finite".to_owned(),
                ));
            }
            absorption[owner][absorber_position] = value;
        }
    }

    let support_certificate = scale.ell() > EPS_ANGLE && uncovered_physical_mass <= EPS_ABS;
    let physical_positive = physical_indices.iter().all(|&index| {
        physical_volumes[index].is_some_and(|value| value > 0.0)
            && masses[index].is_some_and(|value| value > 0.0)
            && densities[index].is_some_and(|value| value.is_finite() && value > 0.0)
    });
    let coupling_marginals = coupling_marginals_hold(&coupling, density);
    let coupling_cut_locus = coupling.iter().enumerate().any(|(physical_position, row)| {
        row.iter().enumerate().any(|(site_index, value)| {
            *value > 0.0
                && nodes[physical_indices[physical_position]]
                    .direction
                    .angle(&density.sites[site_index].direction)
                    .map(|theta| {
                        theta >= std::f64::consts::PI - crate::numeric::CUT_LOCUS_MARGIN_RAD
                    })
                    .unwrap_or(true)
        })
    });
    let absorption_cut_locus = absorption.iter().enumerate().any(|(owner, row)| {
        row.iter().enumerate().any(|(absorber_position, value)| {
            *value > 0.0
                && nodes[owner]
                    .direction
                    .angle(&nodes[physical_indices[absorber_position]].direction)
                    .map(|theta| {
                        theta >= std::f64::consts::PI - crate::numeric::CUT_LOCUS_MARGIN_RAD
                    })
                    .unwrap_or(true)
        })
    });

    let mut structural_failure = None;
    let duplicate_direction = (0..sample_count).any(|left| {
        ((left + 1)..sample_count).any(|right| {
            nodes[left]
                .direction
                .angle(&nodes[right].direction)
                .map(|value| value <= EPS_ANGLE)
                .unwrap_or(true)
        })
    });
    if duplicate_direction {
        structural_failure = Some("sample_degenerate".to_owned());
    } else if transport_volumes.iter().any(|value| *value <= 0.0)
        || (total_transport_volume - 1.0).abs() > EPS_ABS + EPS_REL
    {
        structural_failure = Some("transport_coverage".to_owned());
    } else if !support_certificate || uncovered_physical_mass > EPS_ABS {
        structural_failure = Some("physical_support_uncovered".to_owned());
    } else if !physical_positive || !coupling_marginals {
        structural_failure = Some("physical_marginals".to_owned());
    } else if coupling_cut_locus {
        structural_failure = Some("coupling_cut_locus".to_owned());
    } else if graph_cut_locus {
        structural_failure = Some("graph_cut_locus".to_owned());
    } else if !graph_connected {
        structural_failure = Some("graph_disconnected".to_owned());
    } else if absorption_cut_locus {
        structural_failure = Some("absorption_cut_locus".to_owned());
    }

    let structural_pass = structural_failure.is_none();
    let candidate_score_valid = physical_indices.iter().any(|&index| {
        physical_volumes[index].is_some_and(|value| value > 0.0)
            && masses[index].is_some_and(|value| value > 0.0)
    });

    Ok(PrefixEvaluation {
        nodes: nodes.to_vec(),
        phi,
        chi,
        coupling,
        graph_edges,
        absorption,
        transport_volumes,
        physical_volumes,
        masses,
        densities,
        reconstructed_probability,
        uncovered_physical_mass,
        representation_error,
        structural_pass,
        structural_failure,
        candidate_score_valid,
        cubature_leaves: None,
    })
}

#[derive(Clone, Debug)]
struct S2Density {
    sites: Vec<DensitySite>,
    normalization: S2Normalization,
    total_mass: f64,
}

impl S2Density {
    fn build(scale: ScaleLevel, sites: &[DensitySite]) -> Result<Self> {
        let mut total_mass_sum = KahanSum::default();
        for site in sites {
            if !site.mu.is_finite() || site.mu <= 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "DensitySite mass must be finite and positive".to_owned(),
                ));
            }
            total_mass_sum.add(site.mu);
        }
        Ok(Self {
            sites: sites.to_vec(),
            normalization: normalize_s2(scale.ell())?,
            total_mass: finite_positive(total_mass_sum.total(), "S2 total mass")?,
        })
    }

    fn ell(&self) -> f64 {
        self.normalization.ell
    }

    fn kernel_at(&self, at: &Direction, site: &DensitySite) -> Result<f64> {
        s2_kernel_value(self.normalization, at.angle(&site.direction)?)
    }

    fn rho_at(&self, at: &Direction) -> Result<f64> {
        let mut sum = KahanSum::default();
        for site in &self.sites {
            sum.add(site.mu * self.kernel_at(at, site)?);
        }
        finite_nonnegative(sum.total(), "S2 density")
    }

    fn support_positive(&self, at: &Direction) -> Result<bool> {
        for site in &self.sites {
            if at.angle(&site.direction)? < self.ell() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn s2_kernel_value(normalization: S2Normalization, angle: f64) -> Result<f64> {
    let log = normalization.log_evaluate(angle)?;
    if log.is_nan() || log == f64::INFINITY {
        return Err(V2Error::NonFiniteVector("S2 kernel"));
    }
    if log.is_finite() {
        Ok(log.exp())
    } else {
        Ok(0.0)
    }
}

struct S2StructureLayout {
    sample_count: usize,
    physical_count: usize,
    site_count: usize,
    overlap_offset: usize,
    vp_offset: usize,
    coupling_offset: usize,
    uncovered_offset: usize,
    phi_chi_offset: usize,
    component_count: usize,
}

impl S2StructureLayout {
    fn new(sample_count: usize, physical_count: usize, site_count: usize) -> Self {
        let pair_count = sample_count.saturating_mul(sample_count.saturating_sub(1)) / 2;
        let overlap_offset = sample_count;
        let vp_offset = overlap_offset + pair_count;
        let coupling_offset = vp_offset + physical_count;
        let uncovered_offset = coupling_offset + physical_count * site_count;
        let phi_chi_offset = uncovered_offset + 1;
        let component_count = phi_chi_offset + sample_count * physical_count;
        Self {
            sample_count,
            physical_count,
            site_count,
            overlap_offset,
            vp_offset,
            coupling_offset,
            uncovered_offset,
            phi_chi_offset,
            component_count,
        }
    }

    fn pair_index(&self, low: usize, high: usize) -> usize {
        low * (2 * self.sample_count - low - 1) / 2 + (high - low - 1)
    }

    fn overlap(&self, low: usize, high: usize) -> usize {
        self.overlap_offset + self.pair_index(low, high)
    }

    fn coupling(&self, physical_position: usize, site_index: usize) -> usize {
        self.coupling_offset + physical_position * self.site_count + site_index
    }

    fn phi_chi(&self, owner: usize, absorber_position: usize) -> usize {
        self.phi_chi_offset + owner * self.physical_count + absorber_position
    }
}

fn evaluate_s2_prefix(
    scale: ScaleLevel,
    density: &S2Density,
    nodes: &mut [Sample],
) -> Result<PrefixEvaluation> {
    if nodes.is_empty() {
        return Err(V2Error::InvalidSampleField(
            "Sample prefix cannot be empty".to_owned(),
        ));
    }
    let sample_count = nodes.len();
    let physical_indices: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| sample.role.is_physical().then_some(index))
        .collect();
    let layout = S2StructureLayout::new(sample_count, physical_indices.len(), density.sites.len());
    let support = s2_structure_support(nodes, density)?;
    let structure = integrate_s2_adaptive(
        layout.component_count,
        &support,
        |at| evaluate_s2_structure_integrand(at, nodes, &physical_indices, density, &layout),
        |cell| s2_structure_activity_bounds(cell, nodes, &physical_indices, density, &layout),
    )?;

    let mut transport_volumes = vec![0.0; sample_count];
    for (sample_index, volume) in transport_volumes.iter_mut().enumerate() {
        *volume = finite_nonnegative(structure.fine_values[sample_index], "transport volume")?;
    }
    let total_transport_volume = kahan_sum_finite(&transport_volumes, "transport volumes")?;

    let mut physical_volumes = vec![None; sample_count];
    for (physical_position, &sample_index) in physical_indices.iter().enumerate() {
        let value = finite_nonnegative(
            structure.fine_values[layout.vp_offset + physical_position],
            "physical responsibility volume",
        )?;
        physical_volumes[sample_index] = Some(value);
    }

    let mut coupling = vec![vec![0.0; density.sites.len()]; physical_indices.len()];
    let mut masses = vec![None; sample_count];
    let mut densities = vec![None; sample_count];
    for (physical_position, &sample_index) in physical_indices.iter().enumerate() {
        for (site_index, target) in coupling[physical_position].iter_mut().enumerate() {
            let component = layout.coupling(physical_position, site_index);
            *target = finite_nonnegative(structure.fine_values[component], "coupling")?;
        }
        let mass = kahan_sum_finite(&coupling[physical_position], "physical sample mass")?;
        masses[sample_index] = Some(mass);
        if let Some(volume) = physical_volumes[sample_index] {
            if volume > 0.0 {
                densities[sample_index] = Some(mass / volume);
            }
        }
    }

    let uncovered_integral =
        finite_nonnegative(structure.fine_values[layout.uncovered_offset], "uncovered mass")?;
    let uncovered_error =
        finite_nonnegative(structure.error_bounds[layout.uncovered_offset], "uncovered error")?;
    let mut uncovered_physical_mass = uncovered_integral / density.total_mass;
    if !uncovered_physical_mass.is_finite() || uncovered_physical_mass < 0.0 {
        return Err(V2Error::InvalidSampleField(
            "uncovered physical mass is non-finite".to_owned(),
        ));
    }

    let mut graph_edges = Vec::new();
    let mut graph_adjacency = vec![Vec::new(); sample_count];
    let mut graph_cut_locus = false;
    let mut graph_unresolved = false;
    for low in 0..sample_count {
        for high in (low + 1)..sample_count {
            let component = layout.overlap(low, high);
            let overlap = finite_nonnegative(structure.fine_values[component], "graph overlap")?;
            let overlap_error =
                finite_nonnegative(structure.error_bounds[component], "graph overlap error")?;
            match s2_overlap_class(overlap, overlap_error) {
                S2OverlapClass::Zero => {}
                S2OverlapClass::Unresolved => graph_unresolved = true,
                S2OverlapClass::Nonzero => {
                    let separation = nodes[low].direction.angle(&nodes[high].direction)?;
                    if separation <= EPS_ANGLE
                        || separation >= std::f64::consts::PI - CUT_LOCUS_MARGIN_RAD
                    {
                        graph_cut_locus = true;
                        continue;
                    }
                    let conductance = overlap / separation;
                    if !conductance.is_finite() || conductance <= 0.0 {
                        return Err(V2Error::InvalidSampleField(
                            "graph conductance is non-finite".to_owned(),
                        ));
                    }
                    graph_edges.push(GraphEdge {
                        low_sample_id: low as u64,
                        high_sample_id: high as u64,
                        overlap,
                        conductance,
                    });
                    graph_adjacency[low].push(high);
                    graph_adjacency[high].push(low);
                }
            }
        }
    }
    let graph_connected = graph_is_connected(&graph_adjacency);

    let mut absorption = vec![vec![0.0; physical_indices.len()]; sample_count];
    for (owner, owner_row) in absorption.iter_mut().enumerate() {
        for (absorber_position, &absorber) in physical_indices.iter().enumerate() {
            let overlap = finite_nonnegative(
                structure.fine_values[layout.phi_chi(owner, absorber_position)],
                "absorption overlap",
            )?;
            let rho = densities[absorber].unwrap_or(0.0);
            let value = SIGMA * rho * overlap;
            if !value.is_finite() || value < 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "absorption coefficient is non-finite".to_owned(),
                ));
            }
            owner_row[absorber_position] = value;
        }
    }

    let geometric_certificate =
        s2_site_support_certificate(nodes, &density.sites, scale.ell())?;
    if geometric_certificate
        && uncovered_physical_mass <= EPS_ABS.max(uncovered_error / density.total_mass)
    {
        uncovered_physical_mass = 0.0;
    }

    let candidate_score_valid = physical_indices.iter().any(|&index| {
        physical_volumes[index].is_some_and(|value| value > 0.0)
            && masses[index].is_some_and(|value| value > 0.0)
    });

    let representation_error = if candidate_score_valid {
        s2_representation_error(nodes, &physical_indices, density, &densities)?
    } else {
        f64::INFINITY
    };

    let physical_positive = physical_indices.iter().all(|&index| {
        physical_volumes[index].is_some_and(|value| value > 0.0)
            && masses[index].is_some_and(|value| value > 0.0)
            && densities[index].is_some_and(|value| value.is_finite() && value > 0.0)
    });
    let coupling_marginals = s2_coupling_marginals_hold(&coupling, &density.sites);
    let coupling_cut_locus = coupling.iter().enumerate().any(|(physical_position, row)| {
        row.iter().enumerate().any(|(site_index, value)| {
            *value > EPS_ABS
                && nodes[physical_indices[physical_position]]
                    .direction
                    .angle(&density.sites[site_index].direction)
                    .map(|theta| theta >= std::f64::consts::PI - CUT_LOCUS_MARGIN_RAD)
                    .unwrap_or(true)
        })
    });
    let absorption_cut_locus = absorption.iter().enumerate().any(|(owner, row)| {
        row.iter().enumerate().any(|(absorber_position, value)| {
            *value > EPS_ABS
                && nodes[owner]
                    .direction
                    .angle(&nodes[physical_indices[absorber_position]].direction)
                    .map(|theta| theta >= std::f64::consts::PI - CUT_LOCUS_MARGIN_RAD)
                    .unwrap_or(true)
        })
    });

    let mut structural_failure = None;
    let duplicate_direction = (0..sample_count).any(|left| {
        ((left + 1)..sample_count).any(|right| {
            nodes[left]
                .direction
                .angle(&nodes[right].direction)
                .map(|value| value <= EPS_ANGLE)
                .unwrap_or(true)
        })
    });
    if duplicate_direction {
        structural_failure = Some("sample_degenerate".to_owned());
    } else if transport_volumes.iter().any(|value| *value <= 0.0)
        || (total_transport_volume - 1.0).abs() > EPS_ABS + EPS_REL
    {
        structural_failure = Some("transport_coverage".to_owned());
    } else if !geometric_certificate || uncovered_physical_mass > EPS_ABS {
        structural_failure = Some("physical_support_uncovered".to_owned());
    } else if !physical_positive || !coupling_marginals {
        structural_failure = Some("physical_marginals".to_owned());
    } else if coupling_cut_locus {
        structural_failure = Some("coupling_cut_locus".to_owned());
    } else if graph_unresolved {
        structural_failure = Some("graph_unresolved".to_owned());
    } else if graph_cut_locus {
        structural_failure = Some("graph_cut_locus".to_owned());
    } else if !graph_connected {
        structural_failure = Some("graph_disconnected".to_owned());
    } else if absorption_cut_locus {
        structural_failure = Some("absorption_cut_locus".to_owned());
    }

    Ok(PrefixEvaluation {
        nodes: nodes.to_vec(),
        phi: Vec::new(),
        chi: Vec::new(),
        coupling,
        graph_edges,
        absorption,
        transport_volumes,
        physical_volumes,
        masses,
        densities,
        reconstructed_probability: Vec::new(),
        uncovered_physical_mass,
        representation_error,
        structural_pass: structural_failure.is_none(),
        structural_failure,
        candidate_score_valid,
        cubature_leaves: Some(structure.leaves),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S2OverlapClass {
    Zero,
    Nonzero,
    Unresolved,
}

fn s2_overlap_class(fine: f64, error: f64) -> S2OverlapClass {
    if fine <= EPS_ABS {
        if fine + error <= EPS_ABS {
            S2OverlapClass::Zero
        } else {
            S2OverlapClass::Unresolved
        }
    } else if fine - error > EPS_ABS {
        S2OverlapClass::Nonzero
    } else {
        S2OverlapClass::Unresolved
    }
}

fn s2_site_support_certificate(
    nodes: &[Sample],
    sites: &[DensitySite],
    ell: f64,
) -> Result<bool> {
    for site in sites {
        let mut covered = false;
        for sample in nodes.iter().filter(|sample| sample.role.is_physical()) {
            if site.direction.angle(&sample.direction)? <= ell - EPS_ANGLE {
                covered = true;
                break;
            }
        }
        if !covered {
            return Ok(false);
        }
    }
    Ok(true)
}

fn s2_coupling_marginals_hold(coupling: &[Vec<f64>], sites: &[DensitySite]) -> bool {
    for (site_index, site) in sites.iter().enumerate() {
        let mut sum = KahanSum::default();
        for row in coupling {
            sum.add(row[site_index]);
        }
        if (sum.total() - site.mu).abs() > EPS_ABS + EPS_REL * site.mu.abs().max(1.0) {
            return false;
        }
    }
    let total = coupling
        .iter()
        .flat_map(|row| row.iter().copied())
        .fold(KahanSum::default(), |mut sum, value| {
            sum.add(value);
            sum
        })
        .total();
    let mass = sites.len() as f64;
    (total - mass).abs() <= EPS_ABS + EPS_REL * mass.max(1.0)
}

fn s2_representation_error(
    nodes: &[Sample],
    physical_indices: &[usize],
    density: &S2Density,
    densities: &[Option<f64>],
) -> Result<f64> {
    let support: Vec<S2SupportCap> = density
        .sites
        .iter()
        .map(|site| S2SupportCap::new(site.direction.clone(), density.ell()))
        .collect::<Result<_>>()?;
    let physical_density: Vec<f64> = physical_indices
        .iter()
        .map(|&index| densities[index].unwrap_or(0.0))
        .collect();
    let report = integrate_s2_adaptive(
        3,
        &support,
        |at| {
            let probability = density.rho_at(at)? / density.total_mass;
            let chi = physical_chi_at(nodes, physical_indices, density, at)?.0;
            let mut reconstructed = KahanSum::default();
            for (weight, sample_density) in chi.iter().zip(&physical_density) {
                reconstructed.add(*weight * *sample_density);
            }
            let reconstructed = reconstructed.total() / density.total_mass;
            if !probability.is_finite() || probability < 0.0 || !reconstructed.is_finite() {
                return Err(V2Error::InvalidSampleField(
                    "S2 reconstruction is non-finite".to_owned(),
                ));
            }
            Ok(vec![
                probability,
                reconstructed,
                0.5 * (probability - reconstructed).abs(),
            ])
        },
        |cell| {
            let rho_upper = s2_rho_upper_bound(cell, density)?;
            let p_bound = rho_upper / density.total_mass;
            let mut reconstructed_upper = KahanSum::default();
            for (sample_density, &sample_index) in
                physical_density.iter().zip(physical_indices.iter())
            {
                let chi_bound = physical_chi_bound(cell, &nodes[sample_index].direction, density, rho_upper)?;
                reconstructed_upper.add(*sample_density * chi_bound);
            }
            let reconstructed_bound = reconstructed_upper.total() / density.total_mass;
            Ok(vec![
                p_bound,
                reconstructed_bound,
                0.5 * (p_bound + reconstructed_bound),
            ])
        },
    )?;
    let probability_mass = report.fine_values[0];
    let tv = report.fine_values[2];
    if !tv.is_finite() || tv < 0.0 {
        return Err(V2Error::InvalidSampleField(
            "S2 representation error is non-finite".to_owned(),
        ));
    }
    let mass_tolerance = EPS_QUADRATURE + EPS_REL * 1.0;
    if (probability_mass - 1.0).abs() > mass_tolerance + report.error_bounds[0] {
        return Err(V2Error::InvalidSampleField(
            "S2 density does not integrate to one".to_owned(),
        ));
    }
    Ok(tv)
}

fn s2_structure_support(nodes: &[Sample], density: &S2Density) -> Result<Vec<S2SupportCap>> {
    let mut caps = Vec::new();
    for site in &density.sites {
        caps.push(S2SupportCap::new(site.direction.clone(), density.ell())?);
    }
    for sample in nodes {
        caps.push(S2SupportCap::new(
            sample.direction.clone(),
            sample.transport_radius.min(std::f64::consts::PI),
        )?);
        if sample.role.is_physical() {
            caps.push(S2SupportCap::new(
                sample.direction.clone(),
                (2.0 * density.ell()).min(std::f64::consts::PI),
            )?);
        }
    }
    Ok(caps)
}

fn evaluate_s2_structure_integrand(
    at: &Direction,
    nodes: &[Sample],
    physical_indices: &[usize],
    density: &S2Density,
    layout: &S2StructureLayout,
) -> Result<Vec<f64>> {
    let phi = transport_phi_at(nodes, at)?;
    let (chi, hole) = physical_chi_at(nodes, physical_indices, density, at)?;
    let rho = density.rho_at(at)?;
    let mut values = vec![0.0; layout.component_count];
    for (sample_index, weight) in phi.iter().enumerate() {
        values[sample_index] = *weight;
    }
    for low in 0..layout.sample_count {
        for high in (low + 1)..layout.sample_count {
            values[layout.overlap(low, high)] = phi[low] * phi[high];
        }
    }
    for (physical_position, _) in physical_indices.iter().enumerate() {
        values[layout.vp_offset + physical_position] = chi[physical_position];
        for (site_index, site) in density.sites.iter().enumerate() {
            values[layout.coupling(physical_position, site_index)] =
                chi[physical_position] * site.mu * density.kernel_at(at, site)?;
        }
    }
    values[layout.uncovered_offset] = if hole { rho } else { 0.0 };
    for owner in 0..layout.sample_count {
        for absorber_position in 0..layout.physical_count {
            values[layout.phi_chi(owner, absorber_position)] =
                phi[owner] * chi[absorber_position];
        }
    }
    Ok(values)
}

fn s2_structure_activity_bounds(
    cell: &S2CubatureCell,
    nodes: &[Sample],
    physical_indices: &[usize],
    density: &S2Density,
    layout: &S2StructureLayout,
) -> Result<Vec<f64>> {
    let mut bounds = vec![0.0; layout.component_count];
    let mut phi_bounds = Vec::with_capacity(layout.sample_count);
    for sample in nodes {
        let d_min = cell_d_min(cell, &sample.direction)?;
        phi_bounds.push(partition_component_bound(d_min, sample.transport_radius)?);
    }
    for (sample_index, bound) in phi_bounds.iter().enumerate() {
        bounds[sample_index] = *bound;
    }
    for low in 0..layout.sample_count {
        for high in (low + 1)..layout.sample_count {
            bounds[layout.overlap(low, high)] = phi_bounds[low] * phi_bounds[high];
        }
    }

    let rho_upper = s2_rho_upper_bound(cell, density)?;
    let mut chi_bounds = Vec::with_capacity(layout.physical_count);
    for (physical_position, &sample_index) in physical_indices.iter().enumerate() {
        let chi_bound =
            physical_chi_bound(cell, &nodes[sample_index].direction, density, rho_upper)?;
        chi_bounds.push(chi_bound);
        bounds[layout.vp_offset + physical_position] = chi_bound;
        for (site_index, site) in density.sites.iter().enumerate() {
            let d_min = cell_d_min(cell, &site.direction)?;
            bounds[layout.coupling(physical_position, site_index)] =
                chi_bound * site.mu * s2_kernel_value(density.normalization, d_min)?;
        }
    }
    bounds[layout.uncovered_offset] =
        s2_uncovered_upper_bound(cell, nodes, physical_indices, density, rho_upper)?;
    for owner in 0..layout.sample_count {
        for absorber_position in 0..layout.physical_count {
            bounds[layout.phi_chi(owner, absorber_position)] =
                phi_bounds[owner] * chi_bounds[absorber_position];
        }
    }
    Ok(bounds)
}

fn physical_chi_bound(
    cell: &S2CubatureCell,
    sample_direction: &Direction,
    density: &S2Density,
    rho_upper: f64,
) -> Result<f64> {
    if rho_upper == 0.0 {
        return Ok(0.0);
    }
    let d_min = cell_d_min(cell, sample_direction)?;
    partition_component_bound(d_min, 2.0 * density.ell())
}

fn s2_uncovered_upper_bound(
    cell: &S2CubatureCell,
    nodes: &[Sample],
    physical_indices: &[usize],
    density: &S2Density,
    rho_upper: f64,
) -> Result<f64> {
    if rho_upper == 0.0 {
        return Ok(0.0);
    }
    let center = cell.center()?;
    let radius = cell.conservative_radius();
    let physical_radius = 2.0 * density.ell();
    for &sample_index in physical_indices {
        let d_max = (center.angle(&nodes[sample_index].direction)? + radius).min(std::f64::consts::PI);
        if d_max < physical_radius {
            return Ok(0.0);
        }
    }
    Ok(rho_upper)
}

fn partition_component_bound(d_min: f64, radius: f64) -> Result<f64> {
    if WendlandC2.profile(d_min / radius)? == 0.0 {
        Ok(0.0)
    } else {
        Ok(1.0)
    }
}

fn s2_rho_upper_bound(cell: &S2CubatureCell, density: &S2Density) -> Result<f64> {
    let mut sum = KahanSum::default();
    for site in &density.sites {
        let d_min = cell_d_min(cell, &site.direction)?;
        sum.add(site.mu * s2_kernel_value(density.normalization, d_min)?);
    }
    finite_nonnegative(sum.total(), "S2 density activity bound")
}

fn cell_d_min(cell: &S2CubatureCell, center: &Direction) -> Result<f64> {
    Ok((cell.center()?.angle(center)? - cell.conservative_radius()).max(0.0))
}

fn transport_phi_at(nodes: &[Sample], at: &Direction) -> Result<Vec<f64>> {
    let logs = nodes
        .iter()
        .map(|sample| {
            if !sample.transport_radius.is_finite() || sample.transport_radius <= 0.0 {
                return Err(V2Error::InvalidSampleField(
                    "transport radius must be finite and positive".to_owned(),
                ));
            }
            WendlandC2.log_profile(at.angle(&sample.direction)? / sample.transport_radius)
        })
        .collect::<Result<Vec<_>>>()?;
    normalized_profile_weights(logs)?.ok_or_else(|| {
        V2Error::InvalidSampleField("transport responsibility vanished on S2".to_owned())
    })
}

fn physical_chi_at(
    nodes: &[Sample],
    physical_indices: &[usize],
    density: &S2Density,
    at: &Direction,
) -> Result<(Vec<f64>, bool)> {
    if physical_indices.is_empty() || !density.support_positive(at)? {
        return Ok((vec![0.0; physical_indices.len()], true));
    }
    let logs = physical_indices
        .iter()
        .map(|&sample_index| {
            WendlandC2.log_profile(
                at.angle(&nodes[sample_index].direction)? / (2.0 * density.ell()),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    match normalized_profile_weights(logs)? {
        Some(weights) => Ok((weights, false)),
        None => Ok((vec![0.0; physical_indices.len()], true)),
    }
}

fn build_candidates(sites: &[DensitySite]) -> Result<Vec<Candidate>> {
    let b = sites
        .first()
        .ok_or_else(|| {
            V2Error::InvalidSampleField("cannot build carrier gauge without sites".to_owned())
        })?
        .direction
        .clone();
    let mut tangent = None;
    for site in sites {
        let dot = site.direction.dot(&b)?;
        let projection = site
            .direction
            .as_slice()
            .iter()
            .zip(b.as_slice())
            .map(|(value, base)| value - dot * base)
            .collect::<Vec<_>>();
        let norm = crate::geometry::vector_norm(&projection)?;
        if norm > EPS_ANGLE {
            tangent = Some(Direction::new(projection)?);
            break;
        }
    }
    let tangent = match tangent {
        Some(tangent) => tangent,
        None => {
            let mut axis = vec![0.0; b.dimension()];
            let mut best_index = 0usize;
            let mut best_abs = f64::INFINITY;
            for (index, value) in b.as_slice().iter().enumerate() {
                let absolute = value.abs();
                if absolute < best_abs - EPS_ABS
                    || ((absolute - best_abs).abs() <= EPS_ABS && index < best_index)
                {
                    best_abs = absolute;
                    best_index = index;
                }
            }
            axis[best_index] = 1.0;
            let radial = crate::geometry::dot(&axis, b.as_slice())?;
            let projected: Vec<_> = axis
                .iter()
                .zip(b.as_slice())
                .map(|(value, base)| value - radial * base)
                .collect();
            Direction::new(projected)?
        }
    };
    let c1_raw: Vec<_> = b
        .as_slice()
        .iter()
        .zip(tangent.as_slice())
        .map(|(base, tangent)| {
            -crate::numeric::CARRIER_OFFSET_RAD.cos() * base
                + crate::numeric::CARRIER_OFFSET_RAD.sin() * tangent
        })
        .collect();
    let c1 = Direction::new(c1_raw)?;
    if b.dot(&tangent)?.abs() > EPS_ABS || (c1.norm() - 1.0).abs() > EPS_REL {
        return Err(V2Error::InvalidSampleField(
            "carrier gauge is not orthonormal".to_owned(),
        ));
    }

    let mut candidates = vec![
        Candidate {
            key: (0, 0, 0),
            representative_key: (0, 0, 0),
            representative_direction: b.clone(),
            direction: b,
            broad_carrier: true,
            physical: false,
        },
        Candidate {
            key: (0, 1, 0),
            representative_key: (0, 1, 0),
            representative_direction: c1.clone(),
            direction: c1,
            broad_carrier: true,
            physical: false,
        },
    ];
    for site in sites {
        candidates.push(Candidate {
            key: (1, site.id, 0),
            representative_key: (1, site.id, 0),
            representative_direction: site.direction.clone(),
            direction: site.direction.clone(),
            broad_carrier: false,
            physical: true,
        });
        candidates.push(Candidate {
            key: (1, site.id, 1),
            representative_key: (1, site.id, 1),
            representative_direction: Direction::new(
                site.direction
                    .as_slice()
                    .iter()
                    .map(|value| -value)
                    .collect(),
            )?,
            direction: Direction::new(
                site.direction
                    .as_slice()
                    .iter()
                    .map(|value| -value)
                    .collect(),
            )?,
            broad_carrier: false,
            physical: true,
        });
    }
    candidates.sort_by_key(|candidate| candidate.key);
    let mut representatives: Vec<(CandidateKey, Direction)> = Vec::new();
    for candidate in &mut candidates {
        let mut representative: Option<(CandidateKey, Direction)> = None;
        for (key, direction) in &representatives {
            if candidate.direction.angle(direction)? <= EPS_ANGLE {
                representative = Some((*key, direction.clone()));
                break;
            }
        }
        match representative {
            Some((key, direction)) => {
                candidate.representative_key = key;
                candidate.representative_direction = direction;
            }
            None => {
                representatives.push((candidate.key, candidate.direction.clone()));
                candidate.representative_key = candidate.key;
                candidate.representative_direction = candidate.direction.clone();
            }
        }
    }
    Ok(candidates)
}

fn apply_candidate(
    nodes: &[Sample],
    candidate: &Candidate,
    ell: f64,
) -> Result<Option<Vec<Sample>>> {
    let mut output = nodes.to_vec();
    let mut matching: Option<usize> = None;
    for (index, node) in output.iter().enumerate() {
        let distance = node.direction.angle(&candidate.representative_direction)?;
        let is_better = match matching {
            None => true,
            Some(current) => node.candidate_key < output[current].candidate_key,
        };
        if distance <= EPS_ANGLE && is_better {
            matching = Some(index);
        }
    }
    if let Some(index) = matching {
        if !candidate.physical {
            return Ok(None);
        }
        if output[index].role.is_physical() {
            return Ok(None);
        }
        output[index].role = if output[index].role.has_broad_carrier_profile() {
            SampleRole::CarrierPhysical
        } else {
            SampleRole::Physical
        };
        output[index].physical_candidate_key = Some(candidate.key);
        output[index].transport_radius = if output[index].role.has_broad_carrier_profile() {
            crate::numeric::CARRIER_RADIUS_RAD.max((2.0 * ell).min(std::f64::consts::PI))
        } else {
            (2.0 * ell).min(std::f64::consts::PI)
        };
        return Ok(Some(output));
    }
    let id = u64::try_from(output.len())
        .map_err(|_| V2Error::InvalidSampleField("SampleId exceeds u64".to_owned()))?;
    output.push(Sample {
        id,
        candidate_key: candidate.representative_key,
        physical_candidate_key: candidate.physical.then_some(candidate.key),
        role: SampleRole::Physical,
        direction: candidate.representative_direction.clone(),
        transport_radius: if candidate.broad_carrier {
            crate::numeric::CARRIER_RADIUS_RAD
        } else {
            (2.0 * ell).min(std::f64::consts::PI)
        },
        transport_volume: 0.0,
        physical_volume: None,
        mass: None,
        density: None,
    });
    Ok(Some(output))
}

fn validate_sites(version: &FieldVersion, sites: &[DensitySite]) -> Result<()> {
    for (index, site) in sites.iter().enumerate() {
        if site.id != index as u64 || site.mu != 1.0 {
            return Err(V2Error::InvalidSampleField(
                "DensitySite ids must be contiguous and mu must equal one".to_owned(),
            ));
        }
        if site.direction.dimension() != version.dimension as usize {
            return Err(V2Error::DimensionMismatch {
                expected: version.dimension as usize,
                actual: site.direction.dimension(),
            });
        }
    }
    Ok(())
}

fn compare_score(
    left_uncovered: f64,
    left_error: f64,
    left_key: CandidateKey,
    right_uncovered: f64,
    right_error: f64,
    right_key: CandidateKey,
) -> Ordering {
    left_uncovered
        .total_cmp(&right_uncovered)
        .then_with(|| left_error.total_cmp(&right_error))
        .then_with(|| left_key.cmp(&right_key))
}

fn coupling_marginals_hold(coupling: &[Vec<f64>], density: &SemanticDensity) -> bool {
    for site_index in 0..density.sites.len() {
        let mut sum = KahanSum::default();
        for row in coupling {
            sum.add(row[site_index]);
        }
        if (sum.total() - density.sites[site_index].mu).abs()
            > EPS_ABS + EPS_REL * density.sites[site_index].mu.abs().max(1.0)
        {
            return false;
        }
    }
    for row in coupling {
        let mut sum = KahanSum::default();
        for value in row {
            sum.add(*value);
        }
        if !sum.total().is_finite() || sum.total() < 0.0 {
            return false;
        }
    }
    let total = coupling
        .iter()
        .flat_map(|row| row.iter().copied())
        .fold(KahanSum::default(), |mut sum, value| {
            sum.add(value);
            sum
        })
        .total();
    (total - density.sites.len() as f64).abs()
        <= EPS_ABS + EPS_REL * (density.sites.len() as f64).max(1.0)
}

fn graph_is_connected(adjacency: &[Vec<usize>]) -> bool {
    if adjacency.is_empty() {
        return false;
    }
    let mut visited = vec![false; adjacency.len()];
    let mut stack = vec![0usize];
    visited[0] = true;
    while let Some(node) = stack.pop() {
        for &neighbor in &adjacency[node] {
            if !visited[neighbor] {
                visited[neighbor] = true;
                stack.push(neighbor);
            }
        }
    }
    visited.into_iter().all(|value| value)
}

fn finite_positive(value: f64, label: &'static str) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(V2Error::InvalidSampleField(label.to_owned()));
    }
    Ok(value)
}

/// Stable max/log-domain normalization for compact Wendland profiles.  None
/// means every profile is exactly zero (`-infinity` in log space), which is a
/// valid partial-prefix hole and not a numerical zero fabricated by a floor.
fn normalized_profile_weights(log_profiles: Vec<f64>) -> Result<Option<Vec<f64>>> {
    let Some(log_denominator) = log_sum_exp(log_profiles.iter().copied())? else {
        return Ok(None);
    };
    let weights = log_profiles
        .into_iter()
        .map(|value| {
            if value.is_finite() {
                (value - log_denominator).exp()
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    if weights
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(V2Error::InvalidSampleField(
            "profile normalization is non-finite".to_owned(),
        ));
    }
    validate_partition(&weights, "physical responsibility")?;
    Ok(Some(weights))
}

fn validate_partition(weights: &[f64], label: &'static str) -> Result<()> {
    let mut sum = KahanSum::default();
    for weight in weights {
        if !weight.is_finite() || *weight < 0.0 {
            return Err(V2Error::InvalidSampleField(label.to_owned()));
        }
        sum.add(*weight);
    }
    let total = sum.total();
    if !total.is_finite() || (total - 1.0).abs() > EPS_ABS + EPS_REL {
        return Err(V2Error::InvalidSampleField(format!(
            "{label} does not form a partition"
        )));
    }
    Ok(())
}

fn finite_nonnegative(value: f64, label: &'static str) -> Result<f64> {
    if !value.is_finite() || value < 0.0 {
        return Err(V2Error::InvalidSampleField(label.to_owned()));
    }
    Ok(value)
}

fn kahan_sum_finite(values: &[f64], label: &'static str) -> Result<f64> {
    let mut sum = KahanSum::default();
    for value in values {
        if !value.is_finite() || *value < 0.0 {
            return Err(V2Error::InvalidSampleField(label.to_owned()));
        }
        sum.add(*value);
    }
    finite_nonnegative(sum.total(), label)
}

fn infeasible_outcome(scale: ScaleLevel, failure_code: &str) -> SampleProjectionOutcome {
    SampleProjectionOutcome {
        scale,
        status: ProjectionStatus::Infeasible,
        n_req: None,
        representation_error: None,
        failure_code: Some(failure_code.to_owned()),
        witness_candidate_keys: Vec::new(),
        sample_field: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::FieldVersion;

    fn semantic_direction(axis: usize, sign: f64) -> Direction {
        let mut values = vec![0.0; 384];
        values[axis] = sign;
        Direction::new(values).expect("semantic direction")
    }

    fn sites() -> Vec<DensitySite> {
        vec![
            DensitySite {
                id: 0,
                geometry_gauge_rank: 0,
                direction: semantic_direction(0, 1.0),
                mu: 1.0,
            },
            DensitySite {
                id: 1,
                geometry_gauge_rank: 1,
                direction: semantic_direction(1, 1.0),
                mu: 1.0,
            },
            DensitySite {
                id: 2,
                geometry_gauge_rank: 2,
                direction: semantic_direction(2, 1.0),
                mu: 1.0,
            },
        ]
    }

    fn s2_site(id: u64, coords: [f64; 3]) -> DensitySite {
        DensitySite {
            id,
            geometry_gauge_rank: id,
            direction: Direction::new(coords.to_vec()).expect("S2 direction"),
            mu: 1.0,
        }
    }

    #[test]
    fn physics_empty_sites_are_infeasible() {
        let version = FieldVersion::physics_reference_s2(8).expect("version");
        let scale = ScaleLevel::from_level(2).expect("scale");
        let outcome = project_sample_field(&version, scale, &[]).expect("outcome");
        assert_eq!(outcome.status, ProjectionStatus::Infeasible);
        assert_eq!(
            outcome.failure_code.as_deref(),
            Some("empty_density_sites")
        );
        assert!(outcome.sample_field.is_none());
    }

    #[test]
    fn s2_projection_is_deterministic_and_no_longer_unavailable() {
        let version = FieldVersion::physics_reference_s2(8).expect("version");
        let scale = ScaleLevel::from_level(0).expect("scale");
        let sites = vec![s2_site(0, [0.0, 0.0, 1.0])];
        let first = project_s2_sample_field(&version, scale, &sites).expect("projection");
        let second = project_s2_sample_field(&version, scale, &sites).expect("projection");
        assert_eq!(first.status, second.status);
        assert_eq!(first.n_req, second.n_req);
        assert_eq!(first.representation_error, second.representation_error);
        assert_eq!(first.failure_code, second.failure_code);
        assert_eq!(first.witness_candidate_keys, second.witness_candidate_keys);
        assert_ne!(
            first.failure_code.as_deref(),
            Some("sample_projection_unavailable_s2")
        );
        assert!(matches!(
            first.status,
            ProjectionStatus::Available | ProjectionStatus::Infeasible
        ));
        if first.status == ProjectionStatus::Infeasible {
            assert_eq!(
                first.failure_code.as_deref(),
                Some("projection_candidate_exhausted")
            );
            assert!(first.sample_field.is_none());
            assert!(!first.witness_candidate_keys.is_empty());
        }
    }

    #[test]
    fn s2_promoted_carrier_prefix_has_cubature_leaves_and_coupling() {
        let scale = ScaleLevel::from_level(0).expect("scale");
        let sites = vec![s2_site(0, [0.0, 0.0, 1.0])];
        let density = S2Density::build(scale, &sites).expect("density");
        let candidates = build_candidates(&sites).expect("candidates");
        let mut nodes = vec![candidates[0].as_carrier(0), candidates[1].as_carrier(1)];
        let physical = candidates
            .iter()
            .find(|candidate| candidate.key == (1, 0, 0))
            .expect("site candidate");
        nodes = apply_candidate(&nodes, physical, scale.ell())
            .expect("apply")
            .expect("promotion");
        let evaluation = evaluate_s2_prefix(scale, &density, &mut nodes).expect("prefix");

        assert!(evaluation.structural_pass, "{:?}", evaluation.structural_failure);
        assert_eq!(evaluation.uncovered_physical_mass, 0.0);
        assert!(evaluation.representation_error.is_finite());
        assert!(evaluation.candidate_score_valid);
        assert!(evaluation
            .cubature_leaves
            .as_ref()
            .is_some_and(|leaves| !leaves.is_empty()));

        let mut transport_sum = KahanSum::default();
        for volume in &evaluation.transport_volumes {
            assert!(*volume > 0.0);
            transport_sum.add(*volume);
        }
        assert!((transport_sum.total() - 1.0).abs() <= EPS_ABS + EPS_REL);
        assert_eq!(evaluation.coupling.len(), 1);
        assert!(
            (evaluation.coupling[0][0] - 1.0).abs() <= EPS_ABS + EPS_REL,
            "coupling={}",
            evaluation.coupling[0][0]
        );
        assert!(evaluation.masses[0].is_some_and(|mass| (mass - 1.0).abs() <= EPS_ABS + EPS_REL));
        assert!(evaluation.physical_volumes[0].is_some_and(|volume| volume > 0.0));
        assert!(evaluation.masses[1].is_none());
        assert!(!evaluation.graph_edges.is_empty());
    }

    #[test]
    fn semantic_density_columns_are_stochastic_and_finite() {
        let version = FieldVersion::semantic_production_384(8).expect("version");
        let scale = ScaleLevel::from_level(2).expect("scale");
        let density = SemanticDensity::build(scale, &sites()).expect("density");
        for source in 0..density.sites.len() {
            let mut sum = KahanSum::default();
            for weight in density
                .operational_kernel
                .column(source)
                .expect("kernel column")
            {
                sum.add(weight);
            }
            assert!((sum.total() - 1.0).abs() < 1e-12);
        }
        assert!(density
            .probability_values
            .iter()
            .all(|value| value.is_finite() && *value > 0.0));
        assert_eq!(
            density.sites[0].direction.dimension(),
            version.dimension as usize
        );
    }

    #[test]
    fn semantic_projection_is_deterministic_and_closes_operational_marginals() {
        let version = FieldVersion::semantic_production_384(8).expect("version");
        let scale = ScaleLevel::from_level(4).expect("scale");
        let first = project_semantic_sample_field(&version, scale, &sites()).expect("projection");
        let second = project_semantic_sample_field(&version, scale, &sites()).expect("projection");
        assert_eq!(first.status, second.status);
        assert_eq!(first.n_req, second.n_req);
        assert_eq!(first.representation_error, second.representation_error);
        assert_eq!(first.failure_code, second.failure_code);
        assert_eq!(first.witness_candidate_keys, second.witness_candidate_keys);
        if let (Some(left), Some(right)) = (&first.sample_field, &second.sample_field) {
            assert_eq!(left.candidate_keys, right.candidate_keys);
            assert_eq!(left.phi, right.phi);
            assert_eq!(left.chi, right.chi);
            assert_eq!(left.coupling, right.coupling);
            assert_eq!(left.graph_edges, right.graph_edges);
            assert_eq!(left.absorption, right.absorption);
        }
        assert_eq!(first.status, ProjectionStatus::Available);
        assert!(first.failure_code.is_none());
        let field = first.sample_field.expect("field");
        assert!(field.structural_pass);
        assert!(field.representation_error <= EPS_REPRESENTATION);
        assert_eq!(field.candidate_keys[0], (0, 0, 0));
        assert_eq!(field.candidate_keys[1], (0, 1, 0));

        let physical_rows = field
            .samples
            .iter()
            .filter(|sample| sample.role.is_physical())
            .count();
        assert_eq!(field.coupling.len(), physical_rows);
        for (site_index, site) in field.sites.iter().enumerate() {
            let mut column = KahanSum::default();
            for row in &field.coupling {
                column.add(row[site_index]);
            }
            assert!((column.total() - site.mu).abs() <= EPS_ABS + EPS_REL * site.mu.abs().max(1.0));
        }
        for sample in &field.samples {
            if sample.role == SampleRole::Carrier {
                assert!(sample.physical_volume.is_none());
                assert!(sample.mass.is_none());
                assert!(sample.density.is_none());
            }
        }
    }
}
