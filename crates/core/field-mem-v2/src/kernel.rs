use crate::error::{Result, V2Error};
use crate::{
    geometry::Direction,
    numeric::{log_sum_exp, KahanSum, EPS_ABS, EPS_ANGLE, EPS_KERNEL, EPS_QUADRATURE, EPS_REL},
};

const GL_ORDERS: [usize; 3] = [256, 512, 1024];

/// Contract identity for the only continuous S² cubature implementation.
pub const S2_CUBATURE_ID: &str = "s2_cube_face_adaptive_gl_v1";
pub const S2_CUBATURE_MAX_DEPTH: u8 = 24;
pub const S2_CUBATURE_MAX_LEAVES: usize = 2_000_000;

/// A closed spherical cap registered as a possible non-zero support region.
///
/// A cell is pruned only when it is provably disjoint from every registered
/// cap.  An empty union means the integrand has no declared compact support and
/// therefore disables pruning.
#[derive(Clone, Debug, PartialEq)]
pub struct S2SupportCap {
    pub center: Direction,
    pub radius: f64,
}

impl S2SupportCap {
    pub fn new(center: Direction, radius: f64) -> Result<Self> {
        if !radius.is_finite() || !(0.0..=std::f64::consts::PI).contains(&radius) {
            return Err(cubature_unresolved(
                "support cap radius must be finite and in [0, pi]",
            ));
        }
        Ok(Self { center, radius })
    }
}

/// One cube-face parameter cell, exposed to the caller's activity-bound
/// callback.  `base4_path` uses `00, 01, 10, 11` child digits as `0,1,2,3`.
#[derive(Clone, Debug, PartialEq)]
pub struct S2CubatureCell {
    pub face_id: u8,
    pub base4_path: Vec<u8>,
    pub bounds: [f64; 4],
}

impl S2CubatureCell {
    /// Exact normalized spherical area from the contract's two-triangle rule.
    pub fn normalized_area(&self) -> Result<f64> {
        cell_area(self)
    }

    /// Parameter-midpoint mapped to S².
    pub fn center(&self) -> Result<Direction> {
        let [a0, a1, b0, b1] = self.bounds;
        face_point(self.face_id, 0.5 * (a0 + a1), 0.5 * (b0 + b1))
    }

    /// Conservative angular enclosing radius specified by the contract.
    pub fn conservative_radius(&self) -> f64 {
        let [a0, a1, b0, b1] = self.bounds;
        std::f64::consts::PI.min((0.5 * (a1 - a0)).hypot(0.5 * (b1 - b0)) + EPS_ANGLE)
    }
}

/// A frozen fine leaf suitable for later SampleField persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct S2CubatureLeaf {
    pub face_id: u8,
    pub base4_path: Vec<u8>,
    pub bounds: [f64; 4],
    pub area: f64,
    pub fine_values: Vec<f64>,
    pub error_bounds: Vec<f64>,
}

/// Deterministic result of [`integrate_s2_adaptive`].
#[derive(Clone, Debug, PartialEq)]
pub struct S2CubatureReport {
    pub fine_values: Vec<f64>,
    pub error_bounds: Vec<f64>,
    pub tolerances: Vec<f64>,
    /// Always sorted by `(face_id, base4_path)`.
    pub leaves: Vec<S2CubatureLeaf>,
}

/// Integrates a vector-valued S² function with contract-defined adaptive
/// cube-face GL2/GL4 cubature.
///
/// `evaluate` must return exactly `component_count` finite values at every
/// requested unit direction. `activity_upper_bounds` supplies one finite,
/// nonnegative proof bound per component for the entire cell; it is never
/// inferred from sampled values. The returned leaves retain the final GL4
/// values and error bounds for artifact freezing.
pub fn integrate_s2_adaptive<F, B>(
    component_count: usize,
    support_union: &[S2SupportCap],
    mut evaluate: F,
    mut activity_upper_bounds: B,
) -> Result<S2CubatureReport>
where
    F: FnMut(&Direction) -> Result<Vec<f64>>,
    B: FnMut(&S2CubatureCell) -> Result<Vec<f64>>,
{
    if component_count == 0 {
        return Err(cubature_unresolved("component count must be positive"));
    }
    for cap in support_union {
        S2SupportCap::new(cap.center.clone(), cap.radius)?;
    }

    let mut leaves: Vec<EvaluatedCell> = (0..6)
        .map(|face_id| {
            evaluate_cell(
                S2CubatureCell {
                    face_id,
                    base4_path: Vec::new(),
                    bounds: [-1.0, 1.0, -1.0, 1.0],
                },
                component_count,
                support_union,
                &mut evaluate,
                &mut activity_upper_bounds,
            )
        })
        .collect::<Result<_>>()?;
    sort_evaluated_cells(&mut leaves);

    loop {
        let (fine_values, error_bounds, tolerances) = aggregate_leaves(&leaves, component_count)?;
        if error_bounds
            .iter()
            .zip(&tolerances)
            .all(|(error, tolerance)| error <= tolerance)
        {
            return Ok(S2CubatureReport {
                fine_values,
                error_bounds,
                tolerances,
                leaves: leaves.into_iter().map(EvaluatedCell::into_leaf).collect(),
            });
        }

        let refine_index = select_max_error_cell(&leaves, &tolerances)?;
        let selected = &leaves[refine_index];
        if selected.cell.base4_path.len() >= S2_CUBATURE_MAX_DEPTH as usize {
            return Err(cubature_unresolved(&format!(
                "maximum depth at face {} path {:?} component {}",
                selected.cell.face_id,
                selected.cell.base4_path,
                select_max_component(&selected.error_bounds, &tolerances),
            )));
        }
        if leaves.len() + 3 > S2_CUBATURE_MAX_LEAVES {
            return Err(cubature_unresolved(&format!(
                "maximum leaves at face {} path {:?} component {}",
                selected.cell.face_id,
                selected.cell.base4_path,
                select_max_component(&selected.error_bounds, &tolerances),
            )));
        }

        let children = selected
            .cell
            .children()
            .into_iter()
            .map(|cell| {
                evaluate_cell(
                    cell,
                    component_count,
                    support_union,
                    &mut evaluate,
                    &mut activity_upper_bounds,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        leaves.splice(refine_index..=refine_index, children);
        sort_evaluated_cells(&mut leaves);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WendlandC2;

impl WendlandC2 {
    pub fn profile(self, normalized_angle: f64) -> Result<f64> {
        validate_normalized_angle(normalized_angle)?;
        if normalized_angle >= 1.0 {
            return Ok(0.0);
        }
        let one_minus = 1.0 - normalized_angle;
        Ok(one_minus.powi(4) * (1.0 + 4.0 * normalized_angle))
    }

    pub fn log_profile(self, normalized_angle: f64) -> Result<f64> {
        validate_normalized_angle(normalized_angle)?;
        if normalized_angle >= 1.0 {
            return Ok(f64::NEG_INFINITY);
        }
        Ok(4.0 * (-normalized_angle).ln_1p() + (4.0 * normalized_angle).ln_1p())
    }
}

fn validate_normalized_angle(normalized_angle: f64) -> Result<()> {
    if !normalized_angle.is_finite() {
        return Err(V2Error::NonFiniteVector("Wendland normalized angle"));
    }
    if normalized_angle < 0.0 {
        return Err(V2Error::InvalidSampleField(
            "Wendland normalized angle must be nonnegative".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct S2Normalization {
    pub ell: f64,
    pub log_z: f64,
    pub accepted_order: usize,
    pub relative_delta: f64,
}

impl S2Normalization {
    pub fn evaluate(self, angle: f64) -> Result<f64> {
        Ok(self.log_evaluate(angle)?.exp())
    }

    pub fn log_evaluate(self, angle: f64) -> Result<f64> {
        Ok(WendlandC2.log_profile(angle / self.ell)? - self.log_z)
    }
}

pub fn normalize_s2(ell: f64) -> Result<S2Normalization> {
    if !ell.is_finite() || !(0.0..=std::f64::consts::PI).contains(&ell) || ell == 0.0 {
        return Err(V2Error::InvalidSampleField(
            "kernel scale must be finite and in (0, pi]".into(),
        ));
    }

    let mut previous: Option<f64> = None;
    for order in GL_ORDERS {
        let log_z = log_z_s2_at_order(ell, order)?;
        if let Some(previous_log_z) = previous {
            let relative_delta = (log_z - previous_log_z).exp_m1().abs();
            if relative_delta <= EPS_KERNEL {
                return Ok(S2Normalization {
                    ell,
                    log_z,
                    accepted_order: order,
                    relative_delta,
                });
            }
        }
        previous = Some(log_z);
    }
    Err(V2Error::KernelUnresolved)
}

pub fn semantic_column_weights(log_profiles: impl IntoIterator<Item = f64>) -> Result<Vec<f64>> {
    let log_profiles: Vec<f64> = log_profiles.into_iter().collect();
    if log_profiles
        .iter()
        .any(|value| value.is_nan() || *value == f64::INFINITY)
    {
        return Err(V2Error::NonFiniteVector("semantic kernel column"));
    }
    let denominator = log_sum_exp(log_profiles.iter().copied())?.ok_or_else(|| {
        V2Error::InvalidSampleField("kernel column has no positive self mass".into())
    })?;
    let weights: Vec<f64> = log_profiles
        .into_iter()
        .map(|value| {
            if value.is_finite() {
                (value - denominator).exp()
            } else {
                0.0
            }
        })
        .collect();
    if weights
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(V2Error::InvalidSampleField(
            "kernel column normalization is non-finite".into(),
        ));
    }
    Ok(weights)
}

fn log_z_s2_at_order(ell: f64, order: usize) -> Result<f64> {
    let mut terms = Vec::with_capacity(order);
    for (node, weight) in gauss_legendre(order)? {
        let radius = 0.5 * ell * (node + 1.0);
        let log_profile = WendlandC2.log_profile(radius / ell)?;
        if !log_profile.is_finite() {
            continue;
        }
        let sine = radius.sin();
        if sine <= 0.0 || !sine.is_finite() || weight <= 0.0 || !weight.is_finite() {
            return Err(V2Error::KernelUnresolved);
        }
        // On S2 with normalized sphere measure the radial coefficient is 1/2.
        terms.push((0.25 * ell * weight).ln() + log_profile + sine.ln());
    }
    log_sum_exp(terms)?.ok_or(V2Error::KernelUnresolved)
}

fn gauss_legendre(order: usize) -> Result<Vec<(f64, f64)>> {
    if order == 0 || (order & 1) == 1 {
        return Err(V2Error::KernelUnresolved);
    }
    let mut nodes = Vec::with_capacity(order);
    for k in 1..=(order / 2) {
        let mut root = (std::f64::consts::PI * (k as f64 - 0.25) / (order as f64 + 0.5)).cos();
        for _ in 0..16 {
            let (polynomial, derivative) = legendre_and_derivative(order, root);
            root -= polynomial / derivative;
        }
        let (_, derivative) = legendre_and_derivative(order, root);
        let weight = 2.0 / ((1.0 - root * root) * derivative * derivative);
        if !root.is_finite() || !weight.is_finite() || weight <= 0.0 {
            return Err(V2Error::KernelUnresolved);
        }
        nodes.push((-root, weight));
        nodes.push((root, weight));
    }
    nodes.sort_by(|left, right| left.0.total_cmp(&right.0));
    if nodes.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(V2Error::KernelUnresolved);
    }
    Ok(nodes)
}

fn legendre_and_derivative(order: usize, x: f64) -> (f64, f64) {
    let mut p_nm2 = 1.0;
    let mut p_nm1 = x;
    if order == 0 {
        return (p_nm2, 0.0);
    }
    if order == 1 {
        return (p_nm1, 1.0);
    }
    for n in 2..=order {
        let n_f = n as f64;
        let p_n = ((2.0 * n_f - 1.0) * x * p_nm1 - (n_f - 1.0) * p_nm2) / n_f;
        p_nm2 = p_nm1;
        p_nm1 = p_n;
    }
    let derivative = order as f64 * (x * p_nm1 - p_nm2) / (x * x - 1.0);
    (p_nm1, derivative)
}

impl S2CubatureCell {
    fn children(&self) -> [Self; 4] {
        let [a0, a1, b0, b1] = self.bounds;
        let amid = 0.5 * (a0 + a1);
        let bmid = 0.5 * (b0 + b1);
        std::array::from_fn(|child| {
            let (a_bounds, b_bounds) = match child {
                0 => ((a0, amid), (b0, bmid)),
                1 => ((a0, amid), (bmid, b1)),
                2 => ((amid, a1), (b0, bmid)),
                3 => ((amid, a1), (bmid, b1)),
                _ => unreachable!("array has four children"),
            };
            let mut base4_path = self.base4_path.clone();
            base4_path.push(child as u8);
            Self {
                face_id: self.face_id,
                base4_path,
                bounds: [a_bounds.0, a_bounds.1, b_bounds.0, b_bounds.1],
            }
        })
    }
}

#[derive(Clone, Debug)]
struct EvaluatedCell {
    cell: S2CubatureCell,
    area: f64,
    fine_values: Vec<f64>,
    error_bounds: Vec<f64>,
}

impl EvaluatedCell {
    fn into_leaf(self) -> S2CubatureLeaf {
        S2CubatureLeaf {
            face_id: self.cell.face_id,
            base4_path: self.cell.base4_path,
            bounds: self.cell.bounds,
            area: self.area,
            fine_values: self.fine_values,
            error_bounds: self.error_bounds,
        }
    }
}

fn evaluate_cell<F, B>(
    cell: S2CubatureCell,
    component_count: usize,
    support_union: &[S2SupportCap],
    evaluate: &mut F,
    activity_upper_bounds: &mut B,
) -> Result<EvaluatedCell>
where
    F: FnMut(&Direction) -> Result<Vec<f64>>,
    B: FnMut(&S2CubatureCell) -> Result<Vec<f64>>,
{
    let area = cell.normalized_area()?;
    if !area.is_finite() || area <= 0.0 {
        return Err(cubature_unresolved("cube-face cell has invalid exact area"));
    }

    if !support_union.is_empty() && cell_is_disjoint_from_support_union(&cell, support_union)? {
        return Ok(EvaluatedCell {
            cell,
            area,
            fine_values: vec![0.0; component_count],
            error_bounds: vec![0.0; component_count],
        });
    }

    let upper_bounds = activity_upper_bounds(&cell)?;
    validate_component_vector(&upper_bounds, component_count, "activity upper bounds")?;
    if upper_bounds.iter().any(|bound| *bound < 0.0) {
        return Err(cubature_unresolved(
            "activity upper bounds must be nonnegative",
        ));
    }

    let (coarse_values, coarse_active) =
        evaluate_rule(&cell, &GL2_NODES, evaluate, component_count)?;
    let (fine_values, fine_active) = evaluate_rule(&cell, &GL4_NODES, evaluate, component_count)?;
    let error_bounds = coarse_values
        .iter()
        .zip(&fine_values)
        .zip(coarse_active.iter().zip(&fine_active))
        .zip(&upper_bounds)
        .map(
            |(((coarse, fine), (coarse_active, fine_active)), upper_bound)| {
                let missing_bound = if !coarse_active && !fine_active {
                    area * upper_bound
                } else {
                    0.0
                };
                let error = (fine - coarse).abs() + missing_bound;
                if !error.is_finite() {
                    Err(cubature_unresolved("cell error bound is non-finite"))
                } else {
                    Ok(error)
                }
            },
        )
        .collect::<Result<Vec<_>>>()?;
    for (component, upper_bound) in upper_bounds.iter().enumerate() {
        let maximum_integral = area * upper_bound;
        if coarse_values[component].abs() > maximum_integral + EPS_ABS
            || fine_values[component].abs() > maximum_integral + EPS_ABS
            || error_bounds[component] > maximum_integral + EPS_ABS
        {
            return Err(cubature_unresolved(&format!(
                "activity upper bound is violated for component {component}"
            )));
        }
    }
    Ok(EvaluatedCell {
        cell,
        area,
        fine_values,
        error_bounds,
    })
}

const GL2_NODES: [(f64, f64); 2] = [
    (-0.577_350_269_189_625_8, 1.0),
    (0.577_350_269_189_625_8, 1.0),
];

const GL4_NODES: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_85),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_85),
];

fn evaluate_rule<F>(
    cell: &S2CubatureCell,
    nodes: &[(f64, f64)],
    evaluate: &mut F,
    component_count: usize,
) -> Result<(Vec<f64>, Vec<bool>)>
where
    F: FnMut(&Direction) -> Result<Vec<f64>>,
{
    let [a0, a1, b0, b1] = cell.bounds;
    let amid = 0.5 * (a0 + a1);
    let ahalf = 0.5 * (a1 - a0);
    let bmid = 0.5 * (b0 + b1);
    let bhalf = 0.5 * (b1 - b0);
    let mut raw_nodes = Vec::with_capacity(nodes.len() * nodes.len());
    let mut raw_weight_sum = KahanSum::default();
    for &(node_a, weight_a) in nodes {
        for &(node_b, weight_b) in nodes {
            let a = amid + ahalf * node_a;
            let b = bmid + bhalf * node_b;
            let raw_weight = ahalf * bhalf * weight_a * weight_b * chart_jacobian(a, b)?;
            if !raw_weight.is_finite() || raw_weight <= 0.0 {
                return Err(cubature_unresolved("cubature raw node weight is invalid"));
            }
            raw_weight_sum.add(raw_weight);
            raw_nodes.push((face_point(cell.face_id, a, b)?, raw_weight));
        }
    }
    let raw_weight_sum = raw_weight_sum.total();
    let exact_area = cell.normalized_area()?;
    if !raw_weight_sum.is_finite() || raw_weight_sum <= 0.0 || !exact_area.is_finite() {
        return Err(cubature_unresolved(
            "cubature rule cannot be normalized to exact cell area",
        ));
    }
    let scale = exact_area / raw_weight_sum;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(cubature_unresolved("cubature exact-area scale is invalid"));
    }

    let mut sums = vec![KahanSum::default(); component_count];
    let mut active = vec![false; component_count];
    for (direction, raw_weight) in raw_nodes {
        let values = evaluate(&direction)?;
        validate_component_vector(&values, component_count, "cubature integrand")?;
        let weight = raw_weight * scale;
        if !weight.is_finite() || weight <= 0.0 {
            return Err(cubature_unresolved(
                "cubature scaled node weight is invalid",
            ));
        }
        for (index, value) in values.into_iter().enumerate() {
            if value != 0.0 {
                active[index] = true;
            }
            sums[index].add(weight * value);
        }
    }
    let values = sums.into_iter().map(KahanSum::total).collect::<Vec<_>>();
    validate_component_vector(&values, component_count, "cubature weighted values")?;
    Ok((values, active))
}

fn aggregate_leaves(
    leaves: &[EvaluatedCell],
    component_count: usize,
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let mut values = vec![KahanSum::default(); component_count];
    let mut errors = vec![KahanSum::default(); component_count];
    for leaf in leaves {
        for component in 0..component_count {
            values[component].add(leaf.fine_values[component]);
            errors[component].add(leaf.error_bounds[component]);
        }
    }
    let values = values.into_iter().map(KahanSum::total).collect::<Vec<_>>();
    let errors = errors.into_iter().map(KahanSum::total).collect::<Vec<_>>();
    validate_component_vector(&values, component_count, "cubature fine sum")?;
    validate_component_vector(&errors, component_count, "cubature error sum")?;
    if errors.iter().any(|error| *error < 0.0) {
        return Err(cubature_unresolved("cubature error sum is negative"));
    }
    let tolerances = values
        .iter()
        .map(|value| EPS_QUADRATURE + EPS_REL * value.abs().max(1.0))
        .collect::<Vec<_>>();
    validate_component_vector(&tolerances, component_count, "cubature tolerances")?;
    Ok((values, errors, tolerances))
}

fn sort_evaluated_cells(leaves: &mut [EvaluatedCell]) {
    leaves.sort_by(|left, right| {
        left.cell
            .face_id
            .cmp(&right.cell.face_id)
            .then_with(|| left.cell.base4_path.cmp(&right.cell.base4_path))
    });
}

fn select_max_error_cell(leaves: &[EvaluatedCell], tolerances: &[f64]) -> Result<usize> {
    let mut best_index = None;
    let mut best_error = f64::NEG_INFINITY;
    for (index, leaf) in leaves.iter().enumerate() {
        let normalized_error = leaf
            .error_bounds
            .iter()
            .zip(tolerances)
            .map(|(error, tolerance)| error / tolerance)
            .fold(f64::NEG_INFINITY, f64::max);
        if !normalized_error.is_finite() || normalized_error < 0.0 {
            return Err(cubature_unresolved("cell normalized error is invalid"));
        }
        // Iteration order is canonical `(face_id, base4_path)`, so retaining
        // the first equal maximum implements the contract's tie-break.
        if normalized_error > best_error {
            best_error = normalized_error;
            best_index = Some(index);
        }
    }
    best_index.ok_or_else(|| cubature_unresolved("no cubature leaves"))
}

fn select_max_component(errors: &[f64], tolerances: &[f64]) -> usize {
    let mut best = 0;
    let mut best_error = f64::NEG_INFINITY;
    for (index, (error, tolerance)) in errors.iter().zip(tolerances).enumerate() {
        let normalized = error / tolerance;
        if normalized > best_error {
            best = index;
            best_error = normalized;
        }
    }
    best
}

fn cell_is_disjoint_from_support_union(
    cell: &S2CubatureCell,
    support_union: &[S2SupportCap],
) -> Result<bool> {
    let center = cell.center()?;
    let radius = cell.conservative_radius();
    for cap in support_union {
        if cap.radius >= std::f64::consts::PI - EPS_ANGLE {
            return Ok(false);
        }
        let distance = center.angle(&cap.center)?;
        if distance <= radius + cap.radius + EPS_ANGLE {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_component_vector(
    values: &[f64],
    component_count: usize,
    label: &'static str,
) -> Result<()> {
    if values.len() != component_count {
        return Err(cubature_unresolved(&format!(
            "{label} has {} components; expected {component_count}",
            values.len()
        )));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(cubature_unresolved(&format!(
            "{label} contains non-finite values"
        )));
    }
    Ok(())
}

fn chart_jacobian(a: f64, b: f64) -> Result<f64> {
    let denominator = (1.0 + a * a + b * b).powf(1.5) * (4.0 * std::f64::consts::PI);
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(cubature_unresolved("cube-face chart Jacobian is invalid"));
    }
    Ok(1.0 / denominator)
}

fn cell_area(cell: &S2CubatureCell) -> Result<f64> {
    let [a0, a1, b0, b1] = cell.bounds;
    let v00 = face_point(cell.face_id, a0, b0)?;
    let v10 = face_point(cell.face_id, a1, b0)?;
    let v11 = face_point(cell.face_id, a1, b1)?;
    let v01 = face_point(cell.face_id, a0, b1)?;
    let area = (solid_angle(&v00, &v10, &v11)? + solid_angle(&v00, &v11, &v01)?)
        / (4.0 * std::f64::consts::PI);
    if !area.is_finite() || area <= 0.0 {
        return Err(cubature_unresolved("cube-face exact area is invalid"));
    }
    Ok(area)
}

fn solid_angle(x: &Direction, y: &Direction, z: &Direction) -> Result<f64> {
    let [x0, x1, x2] = direction3(x)?;
    let [y0, y1, y2] = direction3(y)?;
    let [z0, z1, z2] = direction3(z)?;
    let determinant =
        x0 * (y1 * z2 - y2 * z1) - x1 * (y0 * z2 - y2 * z0) + x2 * (y0 * z1 - y1 * z0);
    let denominator = 1.0 + x.dot(y)? + y.dot(z)? + z.dot(x)?;
    let omega = 2.0 * determinant.abs().atan2(denominator);
    if !omega.is_finite() || omega < 0.0 {
        return Err(cubature_unresolved(
            "spherical triangle solid angle is invalid",
        ));
    }
    Ok(omega)
}

fn face_point(face_id: u8, a: f64, b: f64) -> Result<Direction> {
    if !a.is_finite() || !b.is_finite() {
        return Err(cubature_unresolved("cube-face parameter is non-finite"));
    }
    let point = match face_id {
        0 => vec![1.0, a, b],
        1 => vec![-1.0, a, -b],
        2 => vec![-a, 1.0, b],
        3 => vec![a, -1.0, b],
        4 => vec![a, b, 1.0],
        5 => vec![-a, b, -1.0],
        _ => return Err(cubature_unresolved("cube-face id must be in 0..6")),
    };
    Direction::new(point)
}

fn direction3(direction: &Direction) -> Result<[f64; 3]> {
    let values = direction.as_slice();
    if values.len() != 3 {
        return Err(cubature_unresolved("S2 cubature requires 3D directions"));
    }
    Ok([values[0], values[1], values[2]])
}

fn cubature_unresolved(message: &str) -> V2Error {
    V2Error::CubatureUnresolved(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_nonnegative_profile() {
        let kernel = WendlandC2;
        assert_eq!(kernel.profile(0.0).unwrap(), 1.0);
        assert!(kernel.profile(0.5).unwrap() > 0.0);
        assert_eq!(kernel.profile(1.0).unwrap(), 0.0);
        assert_eq!(kernel.profile(2.0).unwrap(), 0.0);
        assert!(kernel.profile(f64::NAN).is_err());
        assert!(kernel.profile(f64::INFINITY).is_err());
        assert!(kernel.profile(-0.5).is_err());
    }

    #[test]
    fn s2_normalization_converges_and_integrates() {
        let normalization = normalize_s2(std::f64::consts::FRAC_PI_2).unwrap();
        assert!(GL_ORDERS.contains(&normalization.accepted_order));
        assert!(normalization.log_z.is_finite());
        assert!(normalization.evaluate(0.0).unwrap().is_finite());
        assert_eq!(normalization.evaluate(normalization.ell).unwrap(), 0.0);

        let integral = gauss_legendre(1024)
            .unwrap()
            .into_iter()
            .map(|(node, weight)| {
                let radius = 0.5 * std::f64::consts::PI * (node + 1.0);
                0.25 * std::f64::consts::PI
                    * weight
                    * normalization.evaluate(radius).unwrap()
                    * radius.sin()
            })
            .sum::<f64>();
        assert!((integral - 1.0).abs() < 1.0e-9, "integral={integral}");
    }

    #[test]
    fn semantic_column_is_stochastic() {
        let weights = semantic_column_weights([0.0, -2.0, f64::NEG_INFINITY]).unwrap();
        assert_eq!(weights[2], 0.0);
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1.0e-14);
        assert!(semantic_column_weights([f64::NAN, 0.0]).is_err());
        assert!(semantic_column_weights([f64::INFINITY, 0.0]).is_err());
    }

    #[test]
    fn cube_faces_have_exact_normalized_total_area_and_constant_integral() {
        let report = integrate_s2_adaptive(2, &[], |_| Ok(vec![1.0, -3.0]), |_| Ok(vec![1.0, 3.0]))
            .expect("constant cubature");

        let area = report.leaves.iter().map(|leaf| leaf.area).sum::<f64>();
        assert!((area - 1.0).abs() < 1.0e-14, "area={area}");
        assert!((report.fine_values[0] - 1.0).abs() < 1.0e-14);
        assert!((report.fine_values[1] + 3.0).abs() < 1.0e-14);
        assert!(report.error_bounds.iter().all(|error| *error < 1.0e-14));
        assert_eq!(report.leaves.len(), 6);
    }

    #[test]
    fn narrow_compact_cap_refines_instead_of_fixed_node_false_zero() {
        let center = Direction::new(vec![1.0, 0.0, 0.0]).expect("S2 center");
        let radius = 0.05;
        let cap = S2SupportCap::new(center.clone(), radius).expect("support cap");
        let report = integrate_s2_adaptive(
            1,
            &[cap],
            |direction| Ok(vec![WendlandC2.profile(direction.angle(&center)? / radius)?]),
            |_| Ok(vec![1.0]),
        )
        .expect("narrow support cubature");

        assert!(report.fine_values[0] > 0.0, "{report:?}");
        assert!(report.leaves.len() > 6, "{report:?}");
        assert!(report.error_bounds[0] <= report.tolerances[0]);
    }

    #[test]
    fn cubature_leaves_and_errors_are_deterministic() {
        let center = Direction::new(vec![1.0, 0.0, 0.0]).expect("S2 center");
        let cap = S2SupportCap::new(center.clone(), 0.4).expect("support cap");
        let run = || {
            integrate_s2_adaptive(
                1,
                std::slice::from_ref(&cap),
                |direction| Ok(vec![WendlandC2.profile(direction.angle(&center)? / 0.4)?]),
                |cell| Ok(vec![1.0 + cell.conservative_radius() * 0.0]),
            )
        };

        let first = run().expect("first cubature run");
        let second = run().expect("second cubature run");
        assert_eq!(first, second);
        assert!(first.leaves.windows(2).all(|pair| {
            (pair[0].face_id, &pair[0].base4_path) < (pair[1].face_id, &pair[1].base4_path)
        }));
    }

    #[test]
    fn cubature_rejects_an_invalid_activity_bound() {
        let result = integrate_s2_adaptive(1, &[], |_| Ok(vec![1.0]), |_| Ok(vec![0.0]));
        assert!(matches!(result, Err(V2Error::CubatureUnresolved(_))));
    }
}
