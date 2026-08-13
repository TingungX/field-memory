use crate::error::{Result, V2Error};
use crate::numeric::{log_sum_exp, EPS_KERNEL};

const GL_ORDERS: [usize; 3] = [256, 512, 1024];

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
}
