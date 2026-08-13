//! Contract-defined f64 geometry on the unit sphere.
//!
//! `Direction` owns a validated unit vector.  Inputs arriving from outside the
//! state model use [`Direction::new`], which normalizes with the required
//! index-ascending Kahan order.  Snapshot decoding uses
//! [`Direction::from_normalized`] instead: it validates the unit invariant
//! without changing bytes that have already been canonicalized.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    error::{Result, V2Error},
    numeric::{KahanSum, EPS_ABS, EPS_ANGLE, EPS_REL},
};

/// The open margin before the antipodal cut locus.
pub const CUT_LOCUS_MARGIN_RAD: f64 = 1.0e-6;

/// A finite, normalized direction in an ambient space of dimension at least 2.
#[derive(Clone, Debug, PartialEq)]
pub struct Direction(Vec<f64>);

impl Direction {
    /// Validates and Kahan-normalizes an external coordinate.
    pub fn new(values: Vec<f64>) -> Result<Self> {
        validate_direction_input(&values)?;
        let norm = vector_norm_impl(&values, "coordinate")?;
        if norm <= EPS_ABS {
            return Err(V2Error::ZeroCoordinate);
        }

        let normalized = values.into_iter().map(|value| value / norm).collect();
        Self::from_normalized(normalized)
    }

    /// Validates an already canonical unit coordinate without renormalizing it.
    ///
    /// This is deliberately distinct from [`Self::new`].  Persistent state is
    /// decoded through this path so JSON round-trips do not perturb a direction
    /// before its canonical hash is checked.
    pub fn from_normalized(values: Vec<f64>) -> Result<Self> {
        validate_direction_input(&values)?;
        let norm = vector_norm_impl(&values, "coordinate")?;
        if norm <= EPS_ABS {
            return Err(V2Error::ZeroCoordinate);
        }
        if !approximately_equal(norm, 1.0) {
            return Err(V2Error::InvalidTangent(
                "direction is not normalized".to_owned(),
            ));
        }
        Ok(Self(values))
    }

    /// Returns the canonical coordinate components in ambient index order.
    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }

    /// Consumes the direction and returns its canonical coordinate components.
    pub fn into_inner(self) -> Vec<f64> {
        self.0
    }

    /// Ambient vector dimension.
    pub fn dimension(&self) -> usize {
        self.0.len()
    }

    /// Kahan-computed vector norm.  The constructor invariant keeps this near 1.
    pub fn norm(&self) -> f64 {
        vector_norm_impl(&self.0, "direction").expect("Direction invariant")
    }

    /// Kahan dot product with another direction.
    pub fn dot(&self, other: &Self) -> Result<f64> {
        dot(&self.0, &other.0)
    }

    /// Contract angle `acos(clamp(dot, -1, 1))`, in radians.
    pub fn angle(&self, other: &Self) -> Result<f64> {
        angle(self, other)
    }
}

impl Serialize for Direction {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Direction {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<f64>::deserialize(deserializer)?;
        Self::from_normalized(values).map_err(serde::de::Error::custom)
    }
}

/// Kahan dot product in ascending component-index order.
pub fn dot(left: &[f64], right: &[f64]) -> Result<f64> {
    validate_vector_pair(left, right, "dot product")?;

    let mut sum = KahanSum::default();
    for index in 0..left.len() {
        let product = left[index] * right[index];
        if !product.is_finite() {
            return Err(V2Error::NonFiniteVector("dot product"));
        }
        sum.add(product);
    }
    let total = sum.total();
    if !total.is_finite() {
        return Err(V2Error::NonFiniteVector("dot product"));
    }
    Ok(total)
}

/// Kahan-computed Euclidean norm in ascending component-index order.
pub fn vector_norm(vector: &[f64]) -> Result<f64> {
    validate_vector(vector, "vector norm")?;
    vector_norm_impl(vector, "vector norm")
}

/// Contract angle `acos(clamp(dot, -1, 1))`, in radians.
pub fn angle(x: &Direction, y: &Direction) -> Result<f64> {
    Ok(x.dot(y)?.clamp(-1.0, 1.0).acos())
}

/// Orthogonally projects `vector` into the tangent space at `point`.
pub fn tangent_project(point: &Direction, vector: &[f64]) -> Result<Vec<f64>> {
    validate_vector_dimension(vector, point.dimension(), "tangent projection")?;
    let radial = dot(vector, point.as_slice())?;
    let projected: Vec<_> = vector
        .iter()
        .zip(point.as_slice())
        .map(|(value, coordinate)| value - radial * coordinate)
        .collect();
    validate_vector(&projected, "tangent projection")?;
    Ok(projected)
}

/// Contract exponential map `Exp_x(v)`.
pub fn exp_map(x: &Direction, vector: &[f64]) -> Result<Direction> {
    let length = validate_tangent(x, vector)?;
    if length <= EPS_ABS {
        return Ok(x.clone());
    }

    let cos_length = length.cos();
    let sin_over_length = length.sin() / length;
    let raw: Vec<_> = x
        .as_slice()
        .iter()
        .zip(vector)
        .map(|(coordinate, tangent)| cos_length * coordinate + sin_over_length * tangent)
        .collect();
    validate_vector(&raw, "exponential-map output")?;
    verify_unit_roundoff(&raw, "exponential-map output")?;

    // This is the one output normalization allowed by the contract, and only
    // after verifying that it corrects roundoff rather than a bad formula.
    let output = Direction::new(raw)?;
    let cosine_residual = (output.dot(x)? - cos_length).abs();
    if cosine_residual > EPS_ABS + EPS_REL * cos_length.abs().max(1.0) {
        return Err(V2Error::InvalidTangent(
            "exponential-map angular residual exceeds tolerance".to_owned(),
        ));
    }
    Ok(output)
}

/// Contract logarithm map `Log_x(y)`.
pub fn log_map(x: &Direction, y: &Direction) -> Result<Vec<f64>> {
    let theta = angle(x, y)?;
    if theta <= EPS_ANGLE {
        return Ok(vec![0.0; x.dimension()]);
    }
    if theta >= std::f64::consts::PI - CUT_LOCUS_MARGIN_RAD {
        return Err(V2Error::CutLocus);
    }

    let tangent = tangent_project(x, y.as_slice())?;
    let tangent_norm = vector_norm_impl(&tangent, "logarithm-map tangent")?;
    if tangent_norm <= EPS_ABS {
        return Err(V2Error::InvalidTangent(
            "logarithm-map tangent degenerates outside the zero-angle branch".to_owned(),
        ));
    }
    let output: Vec<_> = tangent
        .into_iter()
        .map(|component| theta * component / tangent_norm)
        .collect();
    verify_tangent(x, &output)?;
    let output_norm = vector_norm_impl(&output, "logarithm-map output")?;
    if !approximately_equal(output_norm, theta) {
        return Err(V2Error::InvalidTangent(
            "logarithm-map norm residual exceeds tolerance".to_owned(),
        ));
    }

    let reconstructed = exp_map(x, &output)?;
    let reconstruction_error = angle(&reconstructed, y)?;
    if reconstruction_error > EPS_ANGLE + EPS_REL * theta {
        return Err(V2Error::InvalidTangent(
            "logarithm-map angular residual exceeds tolerance".to_owned(),
        ));
    }
    Ok(output)
}

/// Parallel-transports a tangent vector along the unique shortest geodesic.
pub fn parallel_transport(x: &Direction, y: &Direction, vector: &[f64]) -> Result<Vec<f64>> {
    if x.dimension() != y.dimension() {
        return Err(V2Error::DimensionMismatch {
            expected: x.dimension(),
            actual: y.dimension(),
        });
    }
    let input_norm = validate_tangent(x, vector)?;
    if input_norm <= EPS_ABS {
        return Ok(vec![0.0; y.dimension()]);
    }

    let theta = angle(x, y)?;
    if theta <= EPS_ANGLE {
        let output = tangent_project(y, vector)?;
        verify_parallel_transport_output(y, input_norm, &output)?;
        return Ok(output);
    }
    if theta >= std::f64::consts::PI - CUT_LOCUS_MARGIN_RAD {
        return Err(V2Error::CutLocus);
    }

    let dot_xy = x.dot(y)?;
    let denominator = 1.0 + dot_xy;
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(V2Error::CutLocus);
    }
    let multiplier = dot(vector, y.as_slice())? / denominator;
    let raw: Vec<_> = vector
        .iter()
        .zip(x.as_slice().iter().zip(y.as_slice()))
        .map(|(value, (x_component, y_component))| value - multiplier * (x_component + y_component))
        .collect();
    validate_vector(&raw, "parallel-transport output")?;

    // The only permitted output adjustment for PT is a tangent projection at y;
    // it must not be renormalized.
    let output = tangent_project(y, &raw)?;
    verify_parallel_transport_output(y, input_norm, &output)?;
    Ok(output)
}

/// Propagation direction `v_q(s)` away from `q`, or `None` at source/antipode.
///
/// `None` intentionally forces callers to take the contract's explicit zero
/// moment branch rather than choosing an ambient-axis fallback.
pub fn v_q(q: &Direction, s: &Direction) -> Result<Option<Vec<f64>>> {
    let theta = angle(q, s)?;
    if theta <= EPS_ANGLE || theta >= std::f64::consts::PI - EPS_ANGLE {
        return Ok(None);
    }

    let cosine = q.dot(s)?.clamp(-1.0, 1.0);
    let denominator_squared = 1.0 - cosine * cosine;
    if denominator_squared <= 0.0 {
        return Ok(None);
    }
    let denominator = denominator_squared.sqrt();
    let output: Vec<_> = s
        .as_slice()
        .iter()
        .zip(q.as_slice())
        .map(|(s_component, q_component)| (cosine * s_component - q_component) / denominator)
        .collect();
    validate_vector(&output, "propagation direction")?;
    verify_tangent(s, &output)?;
    if !approximately_equal(vector_norm_impl(&output, "propagation direction")?, 1.0) {
        return Err(V2Error::InvalidTangent(
            "propagation direction has non-unit norm".to_owned(),
        ));
    }
    Ok(Some(output))
}

/// Convenience for the explicit source/antipode zero-moment branch.
///
/// Unlike an arbitrary ambient fallback, this returns an exact zero vector only
/// when [`v_q`] reports that the contract-defined direction is singular.
pub fn v_q_or_zero(q: &Direction, s: &Direction) -> Result<Vec<f64>> {
    Ok(v_q(q, s)?.unwrap_or_else(|| vec![0.0; s.dimension()]))
}

fn validate_direction_input(values: &[f64]) -> Result<()> {
    if values.len() < 2 {
        return Err(V2Error::InvalidDimension);
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(V2Error::NonFiniteCoordinate);
    }
    Ok(())
}

fn validate_vector_pair(left: &[f64], right: &[f64], context: &'static str) -> Result<()> {
    if left.len() != right.len() {
        return Err(V2Error::DimensionMismatch {
            expected: left.len(),
            actual: right.len(),
        });
    }
    validate_vector(left, context)?;
    validate_vector(right, context)
}

fn validate_vector_dimension(
    vector: &[f64],
    dimension: usize,
    context: &'static str,
) -> Result<()> {
    if vector.len() != dimension {
        return Err(V2Error::DimensionMismatch {
            expected: dimension,
            actual: vector.len(),
        });
    }
    validate_vector(vector, context)
}

fn validate_vector(vector: &[f64], context: &'static str) -> Result<()> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(V2Error::NonFiniteVector(context));
    }
    Ok(())
}

fn vector_norm_impl(vector: &[f64], context: &'static str) -> Result<f64> {
    let mut squared_norm = KahanSum::default();
    for component in vector {
        let square = component * component;
        if !square.is_finite() {
            return Err(V2Error::NonFiniteVector(context));
        }
        squared_norm.add(square);
    }
    let total = squared_norm.total();
    if !total.is_finite() || total < 0.0 {
        return Err(V2Error::NonFiniteVector(context));
    }
    Ok(total.sqrt())
}

fn validate_tangent(base: &Direction, vector: &[f64]) -> Result<f64> {
    validate_vector_dimension(vector, base.dimension(), "tangent vector")?;
    let norm = vector_norm_impl(vector, "tangent vector")?;
    verify_tangent_with_norm(base, vector, norm)?;
    Ok(norm)
}

fn verify_tangent(base: &Direction, vector: &[f64]) -> Result<()> {
    let norm = vector_norm_impl(vector, "tangent vector")?;
    verify_tangent_with_norm(base, vector, norm)
}

fn verify_tangent_with_norm(base: &Direction, vector: &[f64], norm: f64) -> Result<()> {
    let radial = dot(vector, base.as_slice())?.abs();
    if radial > EPS_ABS + EPS_REL * norm {
        return Err(V2Error::InvalidTangent(
            "vector is not tangent to its base direction".to_owned(),
        ));
    }
    Ok(())
}

fn verify_unit_roundoff(vector: &[f64], context: &'static str) -> Result<()> {
    let norm = vector_norm_impl(vector, context)?;
    if !approximately_equal(norm, 1.0) {
        return Err(V2Error::InvalidTangent(format!(
            "{context} is not unit before permitted roundoff normalization"
        )));
    }
    Ok(())
}

fn verify_parallel_transport_output(
    destination: &Direction,
    input_norm: f64,
    output: &[f64],
) -> Result<()> {
    verify_tangent(destination, output)?;
    let output_norm = vector_norm_impl(output, "parallel-transport output")?;
    if !approximately_equal(output_norm, input_norm) {
        return Err(V2Error::InvalidTangent(
            "parallel transport does not preserve vector norm".to_owned(),
        ));
    }
    Ok(())
}

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= EPS_ABS + EPS_REL * left.abs().max(right.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direction(values: &[f64]) -> Direction {
        Direction::new(values.to_vec()).expect("valid test direction")
    }

    fn assert_close(left: f64, right: f64, tolerance: f64) {
        assert!(
            (left - right).abs() <= tolerance,
            "left={left:?}, right={right:?}, tolerance={tolerance:?}"
        );
    }

    #[test]
    fn direction_normalizes_external_values_with_kahan_norm() {
        let value = direction(&[3.0, 4.0, 0.0]);
        assert_close(value.as_slice()[0], 0.6, EPS_ABS);
        assert_close(value.as_slice()[1], 0.8, EPS_ABS);
        assert_close(value.norm(), 1.0, EPS_ABS);
    }

    #[test]
    fn direction_rejects_invalid_external_coordinates() {
        assert!(matches!(
            Direction::new(vec![1.0]),
            Err(V2Error::InvalidDimension)
        ));
        assert!(matches!(
            Direction::new(vec![0.0, 0.0]),
            Err(V2Error::ZeroCoordinate)
        ));
        assert!(matches!(
            Direction::new(vec![f64::NAN, 0.0]),
            Err(V2Error::NonFiniteCoordinate)
        ));
    }

    #[test]
    fn direction_json_round_trip_preserves_canonical_bits() {
        let original = direction(&[1.0, 2.0, 3.0]);
        let encoded = serde_json::to_string(&original).expect("serialize direction");
        let decoded: Direction = serde_json::from_str(&encoded).expect("deserialize direction");
        assert_eq!(original.dimension(), decoded.dimension());
        for (left, right) in original.as_slice().iter().zip(decoded.as_slice()) {
            assert_eq!(left.to_bits(), right.to_bits());
        }
    }

    #[test]
    fn tangent_projection_removes_the_radial_component() {
        let base = direction(&[1.0, 0.0, 0.0]);
        let projected = tangent_project(&base, &[3.0, 4.0, 0.0]).expect("project tangent");
        assert_close(projected[0], 0.0, EPS_ABS);
        assert_close(projected[1], 4.0, EPS_ABS);
        assert_close(dot(&projected, base.as_slice()).expect("dot"), 0.0, EPS_ABS);
    }

    #[test]
    fn exp_and_log_are_a_round_trip_away_from_cut_locus() {
        let x = direction(&[1.0, 0.0, 0.0]);
        let y = direction(&[0.0, 1.0, 0.0]);
        let log = log_map(&x, &y).expect("log map");
        assert_close(
            vector_norm(&log).expect("norm"),
            std::f64::consts::FRAC_PI_2,
            1.0e-8,
        );
        let reconstructed = exp_map(&x, &log).expect("exp map");
        assert_close(angle(&reconstructed, &y).expect("angle"), 0.0, 1.0e-8);
    }

    #[test]
    fn cut_locus_is_explicitly_rejected_for_nonzero_log_and_transport() {
        let x = direction(&[1.0, 0.0, 0.0]);
        let antipode = direction(&[-1.0, 0.0, 0.0]);
        assert!(matches!(log_map(&x, &antipode), Err(V2Error::CutLocus)));
        assert!(matches!(
            parallel_transport(&x, &antipode, &[0.0, 1.0, 0.0]),
            Err(V2Error::CutLocus)
        ));
    }

    #[test]
    fn parallel_transport_preserves_tangency_and_norm_without_renormalizing() {
        let x = direction(&[1.0, 0.0, 0.0]);
        let y = direction(&[0.0, 1.0, 0.0]);
        let transported = parallel_transport(&x, &y, &[0.0, 0.0, 2.0]).expect("transport");
        assert_close(
            dot(&transported, y.as_slice()).expect("tangent"),
            0.0,
            EPS_ABS,
        );
        assert_close(vector_norm(&transported).expect("norm"), 2.0, EPS_ABS);
        assert_close(transported[2], 2.0, EPS_ABS);
    }

    #[test]
    fn zero_parallel_transport_rejects_endpoint_dimension_mismatch() {
        let x = direction(&[1.0, 0.0, 0.0]);
        let y = direction(&[1.0, 0.0]);
        assert!(matches!(
            parallel_transport(&x, &y, &[0.0, 0.0, 0.0]),
            Err(V2Error::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn propagation_direction_uses_an_explicit_singular_zero_branch() {
        let q = direction(&[1.0, 0.0, 0.0]);
        let orthogonal = direction(&[0.0, 1.0, 0.0]);
        let tangent = v_q(&q, &orthogonal)
            .expect("v_q")
            .expect("non-singular direction");
        assert_close(tangent[0], -1.0, EPS_ABS);
        assert_close(tangent[1], 0.0, EPS_ABS);
        assert_close(vector_norm(&tangent).expect("norm"), 1.0, EPS_ABS);
        assert_eq!(v_q(&q, &q).expect("source branch"), None);
        assert_eq!(
            v_q_or_zero(&q, &direction(&[-1.0, 0.0, 0.0])).expect("antipode branch"),
            vec![0.0, 0.0, 0.0]
        );
    }
}
