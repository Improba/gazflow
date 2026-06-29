//! Simulation multi-pas steady-state sur une série météo (P9.5).
//!
//! Chaque pas résout un régime permanent (hypothèse quasi-stationnaire : la dynamique
//! transitoire du réseau est ignorée entre deux heures). Le warm-start réutilise la
//! solution convergée du pas précédent ; en cas d'échec Newton, un second essai sans
//! initialisation est tenté avant de marquer le pas en échec.

use std::collections::HashMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::graph::GasNetwork;

use super::config::SteadyStateConfig;
use super::continuation::solve_steady_state_with_preset;
use super::demand::{DemandProfile, resolve_demands};
use super::gas_properties::GasComposition;
use super::presets::{preset_for_node_count, preset_robust};
use super::steady_state::{SolverControl, SolverResult, solve_steady_state_with_progress};

/// Pas météo horaire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherStep {
    pub hour: u8,
    pub t_ext_c: f64,
}

#[derive(Debug, Clone)]
pub struct TimeseriesConfig {
    pub gas_composition: GasComposition,
    pub max_iter: usize,
    pub tolerance: f64,
    pub warm_start: bool,
    /// Variation relative max des demandes (Σ|Δd|/Σ|d|) pour réutiliser le warm-start.
    pub warm_start_max_demand_rel_change: f64,
    /// Active continuation de charge sur grands réseaux (auto si node_count > 199).
    pub robust_solver: bool,
}

impl Default for TimeseriesConfig {
    fn default() -> Self {
        Self {
            gas_composition: GasComposition::default(),
            max_iter: 800,
            tolerance: 1e-3,
            warm_start: true,
            warm_start_max_demand_rel_change: 3.0,
            robust_solver: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeseriesStepResult {
    pub hour: u8,
    pub t_ext_c: f64,
    pub demands: HashMap<String, f64>,
    pub pressures: HashMap<String, f64>,
    pub flows: HashMap<String, f64>,
    pub iterations: usize,
    pub residual: f64,
    pub converged: bool,
    pub min_pressure_bar: f64,
    pub max_pressure_bar: f64,
    /// `true` si la convergence a nécessité un redémarrage à froid après échec du warm-start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub retried_cold: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeseriesResult {
    pub steps: Vec<TimeseriesStepResult>,
    pub total_iterations: usize,
    pub failed_hours: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeseriesControl {
    Continue,
    Cancel,
}

pub fn simulate_timeseries(
    network: &GasNetwork,
    profiles: &HashMap<String, DemandProfile>,
    weather: &[WeatherStep],
    config: &TimeseriesConfig,
) -> Result<TimeseriesResult> {
    simulate_timeseries_with_progress(network, profiles, weather, config, None)
}

pub fn simulate_timeseries_with_progress(
    network: &GasNetwork,
    profiles: &HashMap<String, DemandProfile>,
    weather: &[WeatherStep],
    config: &TimeseriesConfig,
    on_step: Option<&dyn Fn(&TimeseriesStepResult) -> TimeseriesControl>,
) -> Result<TimeseriesResult> {
    if weather.is_empty() {
        bail!("weather series must contain at least one step");
    }
    validate_profiles(profiles)?;
    validate_weather(weather)?;
    if config.max_iter == 0 {
        bail!("max_iter must be positive");
    }
    if !config.tolerance.is_finite() || config.tolerance <= 0.0 {
        bail!("tolerance must be finite and positive");
    }

    let mut steps = Vec::with_capacity(weather.len());
    let mut total_iterations = 0usize;
    let mut failed_hours = Vec::new();
    let mut prev_pressures: Option<HashMap<String, f64>> = None;
    let mut prev_demands: Option<HashMap<String, f64>> = None;

    let steady_template = SteadyStateConfig {
        gas_composition: config.gas_composition,
        max_iter: config.max_iter,
        tolerance: config.tolerance,
        snapshot_every: 0,
    };

    for step in weather {
        let demands = resolve_demands(profiles, step.t_ext_c, step.hour)?;
        let warm_ic = warm_start_pressures(
            config,
            prev_pressures.as_ref(),
            prev_demands.as_ref(),
            &demands,
        );

        let (solve_outcome, retried_cold) = solve_timeseries_step(
            network,
            &demands,
            warm_ic.as_ref(),
            steady_template,
            config.robust_solver,
        );

        let step_result = match solve_outcome {
            Ok(result) => {
                total_iterations += result.iterations;
                let (min_p, max_p) = pressure_range(&result.pressures);
                prev_pressures = Some(result.pressures.clone());
                prev_demands = Some(demands.clone());
                TimeseriesStepResult {
                    hour: step.hour,
                    t_ext_c: step.t_ext_c,
                    demands,
                    pressures: result.pressures,
                    flows: result.flows,
                    iterations: result.iterations,
                    residual: result.residual,
                    converged: true,
                    min_pressure_bar: min_p,
                    max_pressure_bar: max_p,
                    retried_cold,
                }
            }
            Err(err) => {
                failed_hours.push(step.hour);
                prev_pressures = None;
                prev_demands = None;
                tracing::warn!(hour = step.hour, error = %err, "timeseries step failed");
                TimeseriesStepResult {
                    hour: step.hour,
                    t_ext_c: step.t_ext_c,
                    demands,
                    pressures: HashMap::new(),
                    flows: HashMap::new(),
                    iterations: 0,
                    residual: f64::NAN,
                    converged: false,
                    min_pressure_bar: f64::NAN,
                    max_pressure_bar: f64::NAN,
                    retried_cold: false,
                }
            }
        };

        if let Some(cb) = on_step {
            if cb(&step_result) == TimeseriesControl::Cancel {
                bail!("cancelled");
            }
        }
        steps.push(step_result);
    }

    Ok(TimeseriesResult {
        steps,
        total_iterations,
        failed_hours,
    })
}

/// Résout un pas ; tente un redémarrage à froid si le warm-start échoue.
fn solve_timeseries_step(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    warm_ic: Option<&HashMap<String, f64>>,
    steady_config: SteadyStateConfig,
    robust_solver: bool,
) -> (Result<SolverResult>, bool) {
    if let Some(ic) = warm_ic {
        match solve_step(network, demands, Some(ic), steady_config, robust_solver) {
            ok @ Ok(_) => return (ok, false),
            Err(err) => {
                tracing::debug!(error = %err, "warm-start failed, retrying cold");
                let cold = solve_step(network, demands, None, steady_config, robust_solver);
                return (cold, true);
            }
        }
    }
    (
        solve_step(network, demands, None, steady_config, robust_solver),
        false,
    )
}

fn solve_step(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    steady_config: SteadyStateConfig,
    robust_solver: bool,
) -> Result<SolverResult> {
    if network.node_count() > 199 || robust_solver {
        let mut preset = if robust_solver {
            preset_robust(network.node_count())
        } else {
            preset_for_node_count(network.node_count())
        };
        preset.max_iter = steady_config.max_iter;
        preset.tolerance = steady_config.tolerance;
        preset.snapshot_every = steady_config.snapshot_every.max(1);
        return solve_steady_state_with_preset(
            network,
            demands,
            initial_pressures,
            &preset,
            steady_config.gas_composition,
            |_| SolverControl::Continue,
            None::<fn(super::continuation::ContinuationStepEvent)>,
        );
    }
    solve_steady_state_with_progress(network, demands, initial_pressures, steady_config, |_| {
        SolverControl::Continue
    })
}

fn warm_start_pressures(
    config: &TimeseriesConfig,
    prev_pressures: Option<&HashMap<String, f64>>,
    prev_demands: Option<&HashMap<String, f64>>,
    demands: &HashMap<String, f64>,
) -> Option<HashMap<String, f64>> {
    if !config.warm_start {
        return None;
    }
    let prev_p = prev_pressures?;
    let prev_d = prev_demands?;
    if demand_relative_change(prev_d, demands) > config.warm_start_max_demand_rel_change {
        return None;
    }
    Some(prev_p.clone())
}

/// $\sum_i |d_i - d_i^{\mathrm{prev}}| / \max(\sum_i |d_i^{\mathrm{prev}}|, \varepsilon)$
pub(crate) fn demand_relative_change(
    previous: &HashMap<String, f64>,
    current: &HashMap<String, f64>,
) -> f64 {
    let mut keys: Vec<&String> = previous.keys().chain(current.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut delta_sum = 0.0_f64;
    let mut previous_sum = 0.0_f64;
    for key in keys {
        let prev = previous.get(key).copied().unwrap_or(0.0);
        let curr = current.get(key).copied().unwrap_or(0.0);
        delta_sum += (curr - prev).abs();
        previous_sum += prev.abs();
    }
    delta_sum / previous_sum.max(1e-9)
}

pub(crate) fn validate_profiles(profiles: &HashMap<String, DemandProfile>) -> Result<()> {
    if profiles.is_empty() {
        bail!("profiles must not be empty");
    }
    for (node_id, profile) in profiles {
        if !profile.q0_m3h.is_finite() || profile.q0_m3h < 0.0 {
            bail!("invalid q0_m3h for node '{node_id}'");
        }
        if !profile.alpha_m3h_per_c.is_finite() || profile.alpha_m3h_per_c < 0.0 {
            bail!("invalid alpha_m3h_per_c for node '{node_id}'");
        }
        if !profile.t_threshold_c.is_finite() {
            bail!("invalid t_threshold_c for node '{node_id}'");
        }
        if let Some(cap) = profile.max_heating_m3h {
            if !cap.is_finite() || cap < 0.0 {
                bail!("invalid max_heating_m3h for node '{node_id}'");
            }
        }
        if let Some(weights) = profile.daily_weights {
            let sum: f64 = weights.iter().sum();
            if weights.iter().any(|w| !w.is_finite() || *w < 0.0) {
                bail!("daily weights must be finite and non-negative for node '{node_id}'");
            }
            if sum <= 0.0 {
                bail!("daily weights must have positive sum for node '{node_id}'");
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_weather(weather: &[WeatherStep]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for step in weather {
        if step.hour > 23 {
            bail!("invalid hour {} (expected 0–23)", step.hour);
        }
        if !step.t_ext_c.is_finite() {
            bail!("non-finite T_ext at hour {}", step.hour);
        }
        if !seen.insert(step.hour) {
            bail!("duplicate weather step for hour {}", step.hour);
        }
    }
    Ok(())
}

fn pressure_range(pressures: &HashMap<String, f64>) -> (f64, f64) {
    let mut min_p = f64::INFINITY;
    let mut max_p = f64::NEG_INFINITY;
    for &p in pressures.values() {
        if p.is_finite() {
            min_p = min_p.min(p);
            max_p = max_p.max(p);
        }
    }
    if !min_p.is_finite() {
        (0.0, 0.0)
    } else {
        (min_p, max_p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};

    fn two_node_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "SRC".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "SK".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "SRC".into(),
            to: "SK".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn winter_day_weather() -> Vec<WeatherStep> {
        (0u8..24)
            .map(|hour| WeatherStep {
                hour,
                t_ext_c: if (6..=20).contains(&hour) { -2.0 } else { -6.0 },
            })
            .collect()
    }

    fn test_profiles() -> HashMap<String, DemandProfile> {
        let mut profiles = HashMap::new();
        profiles.insert(
            "SK".to_string(),
            DemandProfile::from_category(super::super::demand::ClientCategory::Residential),
        );
        profiles
    }

    #[test]
    fn test_timeseries_24h_converges_all_steps() {
        let net = two_node_network();
        let result = simulate_timeseries(
            &net,
            &test_profiles(),
            &winter_day_weather(),
            &TimeseriesConfig::default(),
        )
        .expect("timeseries");
        assert_eq!(result.steps.len(), 24);
        assert!(
            result.failed_hours.is_empty(),
            "tous les pas doivent converger, échecs: {:?}",
            result.failed_hours
        );
    }

    #[test]
    fn test_timeseries_warm_start_speeds_up() {
        let net = two_node_network();
        let weather = winter_day_weather();
        let profiles = test_profiles();

        let cold = simulate_timeseries(
            &net,
            &profiles,
            &weather,
            &TimeseriesConfig {
                warm_start: false,
                ..Default::default()
            },
        )
        .expect("cold");
        let warm = simulate_timeseries(
            &net,
            &profiles,
            &weather,
            &TimeseriesConfig {
                warm_start: true,
                ..Default::default()
            },
        )
        .expect("warm");

        assert!(warm.total_iterations <= cold.total_iterations);
    }

    #[test]
    fn test_timeseries_progress_callback_cancel() {
        let net = two_node_network();
        let weather = winter_day_weather();
        let profiles = test_profiles();
        let steps_seen = std::sync::atomic::AtomicUsize::new(0);

        let result = simulate_timeseries_with_progress(
            &net,
            &profiles,
            &weather,
            &TimeseriesConfig::default(),
            Some(&|_step| {
                let n = steps_seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if n >= 3 {
                    TimeseriesControl::Cancel
                } else {
                    TimeseriesControl::Continue
                }
            }),
        );

        assert!(result.is_err());
        assert_eq!(steps_seen.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[test]
    fn test_demand_relative_change_zero_for_identical() {
        let mut d = HashMap::new();
        d.insert("A".into(), -5.0);
        assert!(demand_relative_change(&d, &d).abs() < 1e-12);
    }

    #[test]
    fn test_warm_start_skipped_on_large_demand_jump() {
        let mut prev_d = HashMap::new();
        prev_d.insert("A".into(), -1.0);
        let mut curr = HashMap::new();
        curr.insert("A".into(), -10.0);
        let mut prev_p = HashMap::new();
        prev_p.insert("A".into(), 50.0);
        let cfg = TimeseriesConfig::default();
        assert!(demand_relative_change(&prev_d, &curr) > cfg.warm_start_max_demand_rel_change);
        assert!(warm_start_pressures(&cfg, Some(&prev_p), Some(&prev_d), &curr).is_none());
    }

    #[test]
    fn test_validate_weather_rejects_invalid_hour() {
        let weather = vec![WeatherStep {
            hour: 24,
            t_ext_c: 0.0,
        }];
        assert!(validate_weather(&weather).is_err());
    }

    #[test]
    fn test_validate_weather_rejects_duplicate_hour() {
        let weather = vec![
            WeatherStep {
                hour: 0,
                t_ext_c: -5.0,
            },
            WeatherStep {
                hour: 0,
                t_ext_c: -4.0,
            },
        ];
        assert!(validate_weather(&weather).is_err());
    }

    #[test]
    fn test_bad_warm_start_still_converges() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);
        let steady = SteadyStateConfig::default();

        let mut bad_ic = HashMap::new();
        bad_ic.insert("SK".to_string(), 0.5);

        let (result, _) = solve_timeseries_step(&net, &demands, Some(&bad_ic), steady, false);
        assert!(result.is_ok(), "solver should converge: {:?}", result.err());
        let r = result.unwrap();
        assert!(r.residual < steady.tolerance * 10.0);
    }
}
