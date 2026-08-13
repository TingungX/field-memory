use crate::error::{Result, V2Error};

pub const EPS_ABS: f64 = 1.0e-10;
pub const EPS_REL: f64 = 1.0e-8;
pub const EPS_ANGLE: f64 = 1.0e-9;
pub const EPS_LEDGER: f64 = 1.0e-9;
pub const EPS_KERNEL: f64 = 1.0e-11;
pub const EPS_QUADRATURE: f64 = 5.0e-5;
pub const EPS_REPRESENTATION: f64 = 0.03125;
pub const EPS_TRANSPORT_DISTRIBUTION: f64 = 1.0e-5;
pub const CUT_LOCUS_MARGIN_RAD: f64 = 1.0e-6;
pub const SCALE_LEVEL_MAX: u32 = 20;
pub const PHYSICAL_COVERAGE_FACTOR: f64 = 2.0;
pub const CARRIER_OFFSET_RAD: f64 = 0.10;
pub const CARRIER_RADIUS_RAD: f64 = std::f64::consts::FRAC_PI_2 + CARRIER_OFFSET_RAD;
pub const SIGMA: f64 = 1.0 / std::f64::consts::PI;
pub const FVM_CFL_COARSE: f64 = 0.45;
pub const FVM_CFL_FINE: f64 = 0.225;
pub const FVM_HMAX_COARSE: f64 = std::f64::consts::PI / 32.0;
pub const FVM_HMAX_FINE: f64 = std::f64::consts::PI / 64.0;
pub const FVM_MAX_STEPS: usize = 65_536;

#[derive(Clone, Copy, Debug, Default)]
pub struct KahanSum {
    sum: f64,
    correction: f64,
}

impl KahanSum {
    pub fn add(&mut self, value: f64) {
        let adjusted = value - self.correction;
        let next = self.sum + adjusted;
        self.correction = (next - self.sum) - adjusted;
        self.sum = next;
    }

    pub fn total(self) -> f64 {
        self.sum
    }
}

pub fn kahan_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut acc = KahanSum::default();
    for value in values {
        acc.add(value);
    }
    acc.total()
}

pub fn one_minus_exp_neg(value: f64) -> f64 {
    -(-value).exp_m1()
}

pub fn log_sum_exp(values: impl IntoIterator<Item = f64>) -> Result<Option<f64>> {
    let values: Vec<f64> = values.into_iter().collect();
    if values
        .iter()
        .any(|value| value.is_nan() || *value == f64::INFINITY)
    {
        return Err(V2Error::NonFiniteVector("log-sum-exp"));
    }
    let maximum = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    if !maximum.is_finite() {
        return Ok(None);
    }
    let scaled = kahan_sum(
        values
            .into_iter()
            .filter(|value| value.is_finite())
            .map(|value| (value - maximum).exp()),
    );
    if !scaled.is_finite() || scaled <= 0.0 {
        return Err(V2Error::NonFiniteVector("log-sum-exp"));
    }
    Ok(Some(maximum + scaled.ln()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kahan_retains_small_tail() {
        assert_eq!(kahan_sum([1.0e16, 1.0, -1.0e16]), 0.0);
        assert!((kahan_sum([1.0, 1.0e-16, 1.0e-16]) - 1.0000000000000002).abs() < EPS_ABS);
    }

    #[test]
    fn stable_small_absorption() {
        let x = 1.0e-12;
        assert!((one_minus_exp_neg(x) - x).abs() < 1.0e-20);
    }

    #[test]
    fn log_sum_exp_ignores_zero_mass_terms() {
        let actual = log_sum_exp([f64::NEG_INFINITY, 0.0, 0.0]).unwrap().unwrap();
        assert!((actual - 2.0_f64.ln()).abs() < EPS_ABS);
        assert_eq!(log_sum_exp([f64::NEG_INFINITY]).unwrap(), None);
        assert!(log_sum_exp([f64::NAN, 0.0]).is_err());
        assert!(log_sum_exp([f64::INFINITY, 0.0]).is_err());
    }
}
