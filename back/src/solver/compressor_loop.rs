use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};

use crate::compressor::{CompressorCatalog, CompressorOperatingContext, effective_ratio_with_nominal};
use crate::graph::{ConnectionKind, GasNetwork, Pipe};
use crate::solver::gas_properties::DEFAULT_GAS_TEMPERATURE_K;

use super::config::SteadyStateConfig;
use super::steady_state::{
    SolverControl, SolverProgress, SolverResult, solve_steady_state_with_progress,
};

const LEGACY_BLEND_STEPS: [f64; 8] = [0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 0.95, 1.0];
const DEFAULT_OUTER_MAX_ITERS: usize = 12;
const DEFAULT_RELAX: f64 = 0.5;
const MIN_COMPRESSOR_RATIO: f64 = 1.0;
const MAX_COMPRESSOR_RATIO: f64 = 5.0;
const RATIO_SETTLE_EPS: f64 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompressorMapMode {
    Legacy,
    Measurement,
    Biquadratic,
}

pub(crate) fn compressor_map_mode() -> CompressorMapMode {
    let Some(raw) = std::env::var("GAZFLOW_COMPRESSOR_MAP_MODE").ok() else {
        return CompressorMapMode::Legacy;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "legacy" => CompressorMapMode::Legacy,
        "measurement" => CompressorMapMode::Measurement,
        "biquadratic" => CompressorMapMode::Biquadratic,
        other => {
            tracing::warn!(
                mode = other,
                "unknown GAZFLOW_COMPRESSOR_MAP_MODE, falling back to legacy"
            );
            CompressorMapMode::Legacy
        }
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn compressor_outer_max_iters() -> usize {
    env_usize(
        "GAZFLOW_COMPRESSOR_OUTER_MAX_ITERS",
        DEFAULT_OUTER_MAX_ITERS,
    )
    .clamp(1, 128)
}

fn compressor_relax() -> f64 {
    env_f64("GAZFLOW_COMPRESSOR_RELAX", DEFAULT_RELAX).clamp(0.0, 1.0)
}

fn map_steady_config(base: SteadyStateConfig, mode: CompressorMapMode) -> SteadyStateConfig {
    let mut config = base;
    if matches!(
        mode,
        CompressorMapMode::Measurement | CompressorMapMode::Biquadratic
    ) {
        config.disable_compressor_r2_cap = true;
    }
    config
}

fn network_has_transport_compressors(network: &GasNetwork) -> bool {
    network.pipes().any(|pipe| {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            return false;
        }
        pipe.equipment.compressor_nominal_ratio.is_some()
            || pipe.compressor_ratio_max.unwrap_or(1.0) > 1.5
    })
}

fn should_try_compressor_outer_fallback(network: &GasNetwork) -> bool {
    if env_bool("GAZFLOW_SKIP_COMPRESSOR_OUTER", false) {
        return false;
    }
    if !network_has_transport_compressors(network) {
        return false;
    }
    env_bool("GAZFLOW_COMPRESSOR_OUTER", false) || network.node_count() >= 200
}

fn apply_compressor_blend(network: &GasNetwork, blend: f64) -> GasNetwork {
    let mut net = network.clone();
    let blend = blend.clamp(0.0, 1.0);
    for pipe in net.pipes_mut() {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            continue;
        }
        let nominal = pipe
            .equipment
            .compressor_nominal_ratio
            .or(pipe.compressor_ratio_max)
            .unwrap_or(1.08)
            .max(1.0);
        pipe.compressor_ratio_max = Some(1.0 + (nominal - 1.0) * blend);
    }
    net
}

fn legacy_blend_schedule(max_iters: usize) -> Vec<f64> {
    let mut schedule = Vec::with_capacity(max_iters.max(1));
    for i in 0..max_iters.max(1) {
        let blend = LEGACY_BLEND_STEPS.get(i).copied().unwrap_or(1.0);
        schedule.push(blend);
    }
    schedule
}

fn solve_legacy_blend_sequence<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    config: SteadyStateConfig,
    on_progress: &mut F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    if let Ok(result) = solve_steady_state_with_progress(
        network,
        demands,
        initial_pressures,
        config,
        &mut *on_progress,
    ) && result.residual <= config.tolerance
    {
        return Ok(result);
    }

    let mut warm_start = initial_pressures.cloned();
    let mut total_iterations = 0usize;
    let mut last_error: Option<anyhow::Error> = None;
    for blend in legacy_blend_schedule(compressor_outer_max_iters()) {
        let blended = apply_compressor_blend(network, blend);
        match solve_steady_state_with_progress(
            &blended,
            demands,
            warm_start.as_ref(),
            config,
            &mut *on_progress,
        ) {
            Ok(mut result) => {
                total_iterations += result.iterations;
                warm_start = Some(result.pressures.clone());
                result.iterations = total_iterations;
                if result.residual <= config.tolerance {
                    return Ok(result);
                }
                last_error = Some(anyhow!(
                    "compressor outer stage blend={blend:.2} residual={:.3e}",
                    result.residual
                ));
            }
            Err(err) => {
                tracing::warn!(blend, error = %err, "compressor outer stage failed");
                last_error = Some(err);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("compressor outer loop exhausted all blends")))
}

fn operating_context(
    result: &SolverResult,
    pipe: &Pipe,
    t_in_k: f64,
) -> CompressorOperatingContext {
    CompressorOperatingContext {
        q_m3s_norm: result.flows.get(&pipe.id).copied().unwrap_or(0.0).abs(),
        p_in_bar: result
            .pressures
            .get(&pipe.from)
            .copied()
            .unwrap_or(1.01325)
            .max(1e-3),
        t_in_k: t_in_k.max(1.0),
    }
}

fn target_ratio_from_catalog(
    catalog: &CompressorCatalog,
    pipe: &Pipe,
    ctx: &CompressorOperatingContext,
    mode: CompressorMapMode,
) -> Option<f64> {
    if mode == CompressorMapMode::Biquadratic {
        tracing::warn!(
            pipe_id = %pipe.id,
            "GAZFLOW_COMPRESSOR_MAP_MODE=biquadratic not implemented yet, using measurement map"
        );
    }
    let station = catalog.station(&pipe.id)?;
    let nominal = pipe
        .equipment
        .compressor_nominal_ratio
        .or(pipe.compressor_ratio_max);
    let ratio = effective_ratio_with_nominal(station, ctx, nominal);
    Some(ratio.clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO))
}

#[derive(Debug, Clone, Copy, Default)]
struct RatioUpdateStats {
    updated: usize,
    max_delta: f64,
}

fn apply_compressor_map_updates(
    network: &mut GasNetwork,
    result: &SolverResult,
    catalog: &CompressorCatalog,
    mode: CompressorMapMode,
    relax: f64,
    t_in_k: f64,
) -> RatioUpdateStats {
    let mut stats = RatioUpdateStats::default();
    for pipe in network.pipes_mut() {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            continue;
        }
        let ctx = operating_context(result, pipe, t_in_k);
        let Some(target) = target_ratio_from_catalog(catalog, pipe, &ctx, mode) else {
            continue;
        };
        let current = pipe
            .compressor_ratio_max
            .or(pipe.equipment.compressor_nominal_ratio)
            .unwrap_or(1.08)
            .clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO);
        let next = (current + relax * (target - current))
            .clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO);
        let delta = (next - current).abs();
        if delta > 1e-6 {
            pipe.compressor_ratio_max = Some(next);
            stats.updated += 1;
            stats.max_delta = stats.max_delta.max(delta);
        }
    }
    stats
}

fn solve_with_compressor_map<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    config: SteadyStateConfig,
    mode: CompressorMapMode,
    on_progress: &mut F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    let Some(catalog) = network.compressor_catalog.as_ref() else {
        tracing::debug!(
            ?mode,
            "compressor map mode enabled but no .cs catalog on network, running nominal solve"
        );
        return solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            config,
            &mut *on_progress,
        );
    };

    if catalog.stations.is_empty() {
        return solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            config,
            &mut *on_progress,
        );
    }

    let steady_config = map_steady_config(config, mode);
    let relax = compressor_relax();
    let max_iters = compressor_outer_max_iters();
    let t_in_k = DEFAULT_GAS_TEMPERATURE_K;
    let mut warm_start = initial_pressures.cloned();
    let mut adjusted_network = network.clone();
    let mut total_iterations = 0usize;
    let mut last_result: Option<SolverResult> = None;

    for outer in 0..max_iters {
        let mut result = solve_steady_state_with_progress(
            &adjusted_network,
            demands,
            warm_start.as_ref(),
            steady_config,
            &mut *on_progress,
        )?;
        total_iterations += result.iterations;
        result.iterations = total_iterations;
        warm_start = Some(result.pressures.clone());
        last_result = Some(result.clone());

        let updates = apply_compressor_map_updates(
            &mut adjusted_network,
            &result,
            catalog,
            mode,
            relax,
            t_in_k,
        );

        if result.residual <= config.tolerance
            && (updates.updated == 0 || updates.max_delta <= RATIO_SETTLE_EPS)
        {
            return Ok(result);
        }

        if updates.updated == 0 {
            if result.residual <= config.tolerance {
                return Ok(result);
            }
            break;
        }

        tracing::debug!(
            outer = outer + 1,
            updated = updates.updated,
            max_delta = updates.max_delta,
            residual = result.residual,
            "compressor map outer iteration updated ratios"
        );
    }

    if let Some(result) = last_result
        && result.residual <= config.tolerance
    {
        return Ok(result);
    }

    Err(anyhow!(
        "compressor map outer loop exhausted ({max_iters} iterations) without hydraulic convergence"
    ))
}

pub(crate) fn solve_with_compressor_loop<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    config: SteadyStateConfig,
    mut on_progress: F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    if !network_has_transport_compressors(network) {
        return solve_steady_state_with_progress(
            network,
            demands,
            initial_pressures,
            config,
            on_progress,
        );
    }
    let mode = compressor_map_mode();
    match mode {
        CompressorMapMode::Legacy => solve_legacy_blend_sequence(
            network,
            demands,
            initial_pressures,
            config,
            &mut on_progress,
        ),
        CompressorMapMode::Measurement | CompressorMapMode::Biquadratic => {
            solve_with_compressor_map(
                network,
                demands,
                initial_pressures,
                config,
                mode,
                &mut on_progress,
            )
        }
    }
}

pub(crate) fn solve_compressor_outer_fallback<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures: Option<&HashMap<String, f64>>,
    config: SteadyStateConfig,
    mut on_progress: F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    if !should_try_compressor_outer_fallback(network) {
        bail!("compressor outer fallback not applicable");
    }
    tracing::info!("trying compressor outer loop after continuation failure");
    solve_legacy_blend_sequence(
        network,
        demands,
        initial_pressures,
        config,
        &mut on_progress,
    )
}

#[cfg(test)]
mod tests {
    use super::CompressorMapMode;

    #[test]
    fn compressor_map_mode_parses_measurement() {
        unsafe { std::env::set_var("GAZFLOW_COMPRESSOR_MAP_MODE", "measurement") };
        assert_eq!(super::compressor_map_mode(), CompressorMapMode::Measurement);
        unsafe { std::env::remove_var("GAZFLOW_COMPRESSOR_MAP_MODE") };
    }
}
