//! Continuation de charge pour la convergence sur grands réseaux.

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Result, anyhow};

use crate::graph::GasNetwork;
use crate::solver::config::SteadyStateConfig;
use crate::solver::gas_properties::GasComposition;
use crate::solver::steady_state::{
    SolverControl, SolverProgress, SolverResult, solve_steady_state_with_progress,
};

/// Événement émis au début de chaque palier de continuation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinuationStepEvent {
    pub step: usize,
    pub total_steps: usize,
    pub scale: f64,
}

#[derive(Debug, Clone)]
pub struct ContinuationConfig {
    pub scales: Vec<f64>,
    pub max_seconds: Option<u64>,
    pub auto_bridges: usize,
    pub min_gap: f64,
}

impl ContinuationConfig {
    pub fn from_scales(scales: Vec<f64>) -> Self {
        Self {
            scales,
            max_seconds: None,
            auto_bridges: 0,
            min_gap: 0.02,
        }
    }
}

pub fn continuation_iter_schedule(
    max_iter: usize,
    n_scales: usize,
    node_count: usize,
) -> Vec<usize> {
    if n_scales == 0 {
        return Vec::new();
    }

    if node_count > 2000 && n_scales >= 2 && max_iter >= n_scales {
        let mut schedule = vec![1usize; n_scales];
        let remaining = max_iter.saturating_sub(n_scales - 1).max(1);
        schedule[n_scales - 1] = remaining;
        return schedule;
    }

    vec![max_iter.max(1); n_scales]
}

/// Résout en enchaînant des paliers de demande (ex. 10 % → 30 % → 100 %).
pub fn solve_steady_state_with_continuation<F, G>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    steady_config: SteadyStateConfig,
    continuation: &ContinuationConfig,
    mut on_progress: F,
    mut on_continuation_step: Option<G>,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
    G: FnMut(ContinuationStepEvent),
{
    let mut scales: Vec<f64> = continuation
        .scales
        .iter()
        .copied()
        .filter(|s| *s > 0.0)
        .collect();
    if scales.is_empty() {
        return Err(anyhow!("continuation scales must not be empty"));
    }

    if scales.len() == 1 && (scales[0] - 1.0).abs() < 1e-9 {
        return solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            steady_config,
            on_progress,
        );
    }

    let per_scale_iters = continuation_iter_schedule(
        steady_config.max_iter,
        scales.len(),
        network.node_count(),
    );
    let default_iter_budget = |n_scales: usize| {
        (steady_config.max_iter / n_scales.max(1)).max(1)
    };
    let snapshot_default = if network.node_count() > 2000 {
        1
    } else {
        (steady_config.max_iter / 2).max(1)
    };
    let snapshot_every = if steady_config.snapshot_every > 0 {
        steady_config.snapshot_every
    } else {
        snapshot_default
    };

    let started_at = Instant::now();

    let mut last_err: Option<anyhow::Error> = None;
    let mut warm_start_pressures: Option<HashMap<String, f64>> =
        initial_pressures.map(|p| p.clone());
    let mut last_success: Option<SolverResult> = None;
    let mut last_success_scale: Option<f64> = None;
    let mut bridges_used = 0usize;

    let mut idx = 0usize;
    while idx < scales.len() {
        if continuation
            .max_seconds
            .map(|max_s| started_at.elapsed().as_secs() >= max_s)
            .unwrap_or(false)
        {
            break;
        }

        let scale = scales[idx];
        if let Some(ref mut cb) = on_continuation_step {
            cb(ContinuationStepEvent {
                step: idx + 1,
                total_steps: scales.len(),
                scale,
            });
        }

        let iter_budget = continuation_iter_schedule(
            steady_config.max_iter,
            scales.len(),
            network.node_count(),
        )
        .get(idx)
        .copied()
        .or_else(|| per_scale_iters.get(idx).copied())
        .unwrap_or_else(|| default_iter_budget(scales.len()));
        let scaled_demands: HashMap<String, f64> = demands
            .iter()
            .map(|(node_id, q)| (node_id.clone(), q * scale))
            .collect();

        let mut best_snapshot_pressures: Option<HashMap<String, f64>> = None;
        let mut best_snapshot_residual = f64::INFINITY;

        let step_config = SteadyStateConfig {
            max_iter: iter_budget,
            tolerance: steady_config.tolerance,
            snapshot_every,
            gas_composition: steady_config.gas_composition,
        };

        match solve_steady_state_with_progress(
            network,
            &scaled_demands,
            warm_start_pressures.as_ref(),
            step_config,
            |progress| {
                if let Some(pressures) = progress.pressures.clone() {
                    if progress.residual < best_snapshot_residual {
                        best_snapshot_residual = progress.residual;
                        best_snapshot_pressures = Some(pressures);
                    }
                }
                on_progress(progress)
            },
        ) {
            Ok(result) => {
                warm_start_pressures = Some(result.pressures.clone());
                last_success_scale = Some(scale);
                last_success = Some(result);
                idx += 1;
            }
            Err(err) => {
                if let Some(snapshot) = best_snapshot_pressures.take() {
                    warm_start_pressures = Some(snapshot);
                }
                if bridges_used < continuation.auto_bridges {
                    let low = last_success_scale.unwrap_or(0.0);
                    let gap = scale - low;
                    if gap > continuation.min_gap {
                        let mid = low + 0.5 * gap;
                        if !scales.iter().any(|s| (*s - mid).abs() < 1e-9) {
                            scales.insert(idx, mid);
                            bridges_used += 1;
                            continue;
                        }
                    }
                }
                last_err = Some(err);
                idx += 1;
            }
        }
    }

    if let Some(mut result) = last_success {
        if let Some(scale) = last_success_scale {
            result.demand_scale_achieved = Some(scale);
            if scale < 0.999 {
                result.warnings.push(format!(
                    "Continuation partielle : convergence atteinte à {:.0} % des demandes cibles seulement.",
                    scale * 100.0
                ));
            }
        }
        return Ok(result);
    }

    Err(last_err.unwrap_or_else(|| anyhow!("continuation failed on all scales")))
}

/// Point d'entrée unifié : direct ou continuation selon le preset.
pub fn solve_steady_state_with_preset<F, G>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    preset: &super::presets::SolverPreset,
    gas_composition: GasComposition,
    on_progress: F,
    on_continuation_step: Option<G>,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
    G: FnMut(ContinuationStepEvent),
{
    let steady_config = SteadyStateConfig {
        gas_composition,
        max_iter: preset.max_iter,
        tolerance: preset.tolerance,
        snapshot_every: preset.snapshot_every,
    };

    if preset.uses_continuation() {
        let continuation = ContinuationConfig {
            scales: preset.continuation_scales.clone(),
            max_seconds: preset.continuation_max_seconds,
            auto_bridges: preset.continuation_auto_bridges,
            min_gap: 0.02,
        };
        solve_steady_state_with_continuation(
            network,
            demands,
            initial_pressures,
            steady_config,
            &continuation,
            on_progress,
            on_continuation_step,
        )
    } else {
        solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            steady_config,
            on_progress,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};
    use crate::solver::gas_properties::GasComposition;

    use super::*;

    fn build_chain_network(pipe_count: usize) -> GasNetwork {
        let mut net = GasNetwork::new();
        for i in 0..=pipe_count {
            net.add_node(Node {
                id: format!("N{i}"),
                x: i as f64,
                y: 0.0,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: if i == 0 { Some(70.0) } else { None },
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        for i in 0..pipe_count {
            net.add_pipe(Pipe {
                id: format!("P{i}"),
                from: format!("N{i}"),
                to: format!("N{}", i + 1),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 500.0,
                roughness_mm: 0.012,
                flow_min_m3s: None,
                flow_max_m3s: None,
                compressor_ratio_max: None,
                equipment: Default::default(),
            });
        }
        net
    }

    #[test]
    fn continuation_single_scale_delegates_to_direct() {
        let net = build_chain_network(3);
        let mut demands = HashMap::new();
        demands.insert("N3".to_string(), -1.0);

        let cfg = SteadyStateConfig {
            gas_composition: GasComposition::pure_ch4(),
            max_iter: 500,
            tolerance: 1e-4,
            snapshot_every: 0,
        };
        let cont = ContinuationConfig::from_scales(vec![1.0]);
        let result = solve_steady_state_with_continuation(
            &net,
            &demands,
            None,
            cfg,
            &cont,
            |_| SolverControl::Continue,
            None::<fn(ContinuationStepEvent)>,
        )
        .expect("direct path");
        assert!(result.residual < 1e-3);
    }

    #[test]
    fn continuation_emits_step_events() {
        let net = build_chain_network(5);
        let mut demands = HashMap::new();
        demands.insert("N5".to_string(), -2.0);

        let cfg = SteadyStateConfig {
            gas_composition: GasComposition::pure_ch4(),
            max_iter: 300,
            tolerance: 1e-3,
            snapshot_every: 0,
        };
        let cont = ContinuationConfig::from_scales(vec![0.3, 1.0]);
        let mut steps = Vec::new();
        let _ = solve_steady_state_with_continuation(
            &net,
            &demands,
            None,
            cfg,
            &cont,
            |_| SolverControl::Continue,
            Some(|ev| steps.push(ev)),
        );
        assert_eq!(steps.len(), 2);
        assert!((steps[0].scale - 0.3).abs() < 1e-9);
    }

    #[test]
    fn partial_continuation_scale_adds_warning() {
        let net = build_chain_network(5);
        let mut demands = HashMap::new();
        demands.insert("N5".to_string(), -2.0);

        let cfg = SteadyStateConfig {
            gas_composition: GasComposition::pure_ch4(),
            max_iter: 300,
            tolerance: 1e-3,
            snapshot_every: 0,
        };
        let cont = ContinuationConfig {
            scales: vec![0.3, 1.0],
            max_seconds: Some(1),
            auto_bridges: 0,
            min_gap: 0.02,
        };
        let mut slept = false;
        let result = solve_steady_state_with_continuation(
            &net,
            &demands,
            None,
            cfg,
            &cont,
            |_| {
                if !slept {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    slept = true;
                }
                SolverControl::Continue
            },
            None::<fn(ContinuationStepEvent)>,
        )
        .expect("partial continuation should return best scale");

        let scale = result
            .demand_scale_achieved
            .expect("demand_scale_achieved should be set");
        assert!(scale < 0.999, "expected partial scale, got {scale}");
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Continuation partielle")),
            "expected partial continuation warning, got {:?}",
            result.warnings
        );
    }
}
