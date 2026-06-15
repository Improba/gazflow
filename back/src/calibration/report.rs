use serde::Serialize;

use super::CalibrationParameter;

#[derive(Debug, Clone, Serialize)]
pub struct CalibrationReport {
    pub params_before: CalibrationParameter,
    pub params_after: CalibrationParameter,
    pub rmse: f64,
    pub r_squared: f64,
    pub residuals: Vec<f64>,
}

pub(crate) fn rmse(residuals: &[f64], uncertainties: Option<&[f64]>) -> f64 {
    if residuals.is_empty() {
        return f64::INFINITY;
    }
    if let Some(sigmas) = uncertainties {
        if sigmas.len() == residuals.len() && sigmas.iter().any(|s| *s > 0.0) {
            let mut weighted_sse = 0.0;
            let mut weight_sum = 0.0;
            for (residual, sigma) in residuals.iter().zip(sigmas) {
                let w = if *sigma > 0.0 {
                    1.0 / (sigma * sigma)
                } else {
                    1.0
                };
                weighted_sse += residual * residual * w;
                weight_sum += w;
            }
            if weight_sum > 0.0 {
                return (weighted_sse / weight_sum).sqrt();
            }
        }
    }
    (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt()
}

pub(crate) fn r_squared(observed: &[f64], predicted: &[f64]) -> f64 {
    if observed.is_empty() || observed.len() != predicted.len() {
        return 0.0;
    }
    let mean_observed = observed.iter().sum::<f64>() / observed.len() as f64;
    let ss_tot = observed
        .iter()
        .map(|value| {
            let diff = value - mean_observed;
            diff * diff
        })
        .sum::<f64>();
    let ss_res = observed
        .iter()
        .zip(predicted)
        .map(|(obs, pred)| {
            let diff = obs - pred;
            diff * diff
        })
        .sum::<f64>();

    if ss_tot <= 1e-12 {
        if ss_res <= 1e-12 { 1.0 } else { 0.0 }
    } else {
        1.0 - ss_res / ss_tot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rmse_weighted_prefers_low_uncertainty_residual() {
        let residuals = vec![1.0, 10.0];
        let unweighted = rmse(&residuals, None);
        let weighted = rmse(&residuals, Some(&[0.1, 10.0]));
        assert!(weighted < unweighted);
    }

    #[test]
    fn r_squared_perfect_fit_is_one() {
        let observed = vec![1.0, 2.0, 3.0];
        let predicted = vec![1.0, 2.0, 3.0];
        assert!((r_squared(&observed, &predicted) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn r_squared_uses_observed_minus_predicted_residuals() {
        let observed = vec![10.0, 20.0];
        let predicted = vec![8.0, 18.0];
        let ss_res: f64 = observed
            .iter()
            .zip(&predicted)
            .map(|(y, y_hat)| {
                let r = y - y_hat;
                r * r
            })
            .sum();
        let mean = observed.iter().sum::<f64>() / observed.len() as f64;
        let ss_tot: f64 = observed.iter().map(|y| (y - mean).powi(2)).sum();
        let expected = 1.0 - ss_res / ss_tot;
        assert!((r_squared(&observed, &predicted) - expected).abs() < 1e-12);
    }
}
