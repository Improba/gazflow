use std::collections::HashMap;

use crate::graph::GasNetwork;

use super::{
    CalibrationParameter, CalibrationStrategy, ScadaMeasurement, objective::compute_residuals,
};

#[derive(Debug, Clone, Copy)]
pub struct CalibrationOptimizationConfig {
    pub min_factor: f64,
    pub max_factor: f64,
    pub step: f64,
    pub max_iterations: usize,
}

impl CalibrationOptimizationConfig {
    pub fn for_strategy(strategy: CalibrationStrategy) -> Self {
        match strategy {
            CalibrationStrategy::Global => Self {
                min_factor: 0.5,
                max_factor: 2.0,
                step: 0.05,
                max_iterations: 1,
            },
            CalibrationStrategy::PerPipe => Self {
                min_factor: 0.5,
                max_factor: 2.0,
                step: 0.1,
                max_iterations: 4,
            },
        }
    }

    fn candidate_factors(self) -> Vec<f64> {
        let mut candidates = Vec::new();
        let mut factor = self.min_factor.max(1e-6);
        while factor <= self.max_factor + 1e-12 {
            candidates.push(factor);
            factor += self.step.max(1e-6);
        }
        candidates
    }
}

pub fn optimize_parameters(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    initial: &CalibrationParameter,
    config: CalibrationOptimizationConfig,
) -> CalibrationParameter {
    match initial {
        CalibrationParameter::GlobalRoughnessFactor { factor } => optimize_global(
            network,
            demands,
            measurements,
            (*factor).max(1e-6),
            config.candidate_factors(),
        ),
        CalibrationParameter::PerPipeRoughnessMultiplier { multipliers } => {
            optimize_per_pipe(network, demands, measurements, multipliers.clone(), config)
        }
        CalibrationParameter::DemandScale { .. } => initial.clone(),
    }
}

fn optimize_global(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    initial_factor: f64,
    candidate_factors: Vec<f64>,
) -> CalibrationParameter {
    let mut best_factor = initial_factor;
    let mut best_rmse = score_rmse(
        network,
        demands,
        measurements,
        &CalibrationParameter::GlobalRoughnessFactor {
            factor: initial_factor,
        },
    );

    for factor in candidate_factors {
        let candidate = CalibrationParameter::GlobalRoughnessFactor { factor };
        let rmse = score_rmse(network, demands, measurements, &candidate);
        if rmse < best_rmse {
            best_rmse = rmse;
            best_factor = factor;
        }
    }

    CalibrationParameter::GlobalRoughnessFactor {
        factor: best_factor,
    }
}

fn optimize_per_pipe(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    mut multipliers: HashMap<String, f64>,
    config: CalibrationOptimizationConfig,
) -> CalibrationParameter {
    let factors = config.candidate_factors();
    let pipe_ids: Vec<String> = network.pipes().map(|p| p.id.clone()).collect();
    let mut best_overall = score_rmse(
        network,
        demands,
        measurements,
        &CalibrationParameter::PerPipeRoughnessMultiplier {
            multipliers: multipliers.clone(),
        },
    );

    for _ in 0..config.max_iterations {
        let mut improved = false;
        for pipe_id in &pipe_ids {
            let mut best_for_pipe = *multipliers.get(pipe_id).unwrap_or(&1.0);
            let mut best_for_pipe_rmse = best_overall;

            for factor in &factors {
                multipliers.insert(pipe_id.clone(), *factor);
                let candidate = CalibrationParameter::PerPipeRoughnessMultiplier {
                    multipliers: multipliers.clone(),
                };
                let rmse = score_rmse(network, demands, measurements, &candidate);
                if rmse < best_for_pipe_rmse {
                    best_for_pipe_rmse = rmse;
                    best_for_pipe = *factor;
                }
            }

            multipliers.insert(pipe_id.clone(), best_for_pipe);
            if best_for_pipe_rmse < best_overall {
                best_overall = best_for_pipe_rmse;
                improved = true;
            }
        }

        if !improved {
            break;
        }
    }

    CalibrationParameter::PerPipeRoughnessMultiplier { multipliers }
}

fn score_rmse(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    measurements: &[ScadaMeasurement],
    params: &CalibrationParameter,
) -> f64 {
    let residuals = compute_residuals(network, demands, measurements, params);
    if residuals.is_empty() {
        return f64::INFINITY;
    }
    (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt()
}
