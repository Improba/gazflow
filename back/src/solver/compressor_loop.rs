use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};

use crate::compressor::{
    CompressorCatalog, CompressorOperatingContext, effective_ratio_with_nominal_for_mode,
    estimated_map_flow_m3s,
};
use crate::graph::{ConnectionKind, GasNetwork, Pipe};
use crate::solver::gas_properties::DEFAULT_GAS_TEMPERATURE_K;

use super::config::SteadyStateConfig;
use super::steady_state::{
    SolverControl, SolverProgress, SolverResult, network_with_scaled_compressor_lift,
    solve_steady_state_with_progress,
};

const LEGACY_BLEND_STEPS: [f64; 8] = [0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 0.95, 1.0];
const DEFAULT_OUTER_MAX_ITERS: usize = 12;
const DEFAULT_RELAX: f64 = 0.5;
const MIN_COMPRESSOR_RATIO: f64 = 1.0;
const MAX_COMPRESSOR_RATIO: f64 = 5.0;
const RATIO_SETTLE_EPS: f64 = 1e-4;
const FLOW_UPDATE_THRESHOLD_M3S: f64 = 0.01;
const TRANSPORT_NOMINAL_THRESHOLD: f64 = 2.0;
const MAP_CONTINUATION_COUPLING_MIN_SCALE: f64 = 0.5;
const CONTINUATION_HANDOFF_RELAX: f64 = 1.0;
const MAP_REFINE_SCALES: &[f64] = &[0.92, 0.96, 1.0];
const HANDOFF_PREFER_ESTIMATED_RESIDUAL: f64 = 7.0;
const MAX_PLAUSIBLE_COMPRESSOR_FLOW_M3S: f64 = 250.0;
const STUCK_RESIDUAL_RATIO_THRESHOLD: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressorMapMode {
    Legacy,
    Measurement,
    Biquadratic,
}

pub fn compressor_map_mode() -> CompressorMapMode {
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

fn compressor_strict_newton() -> bool {
    env_bool("GAZFLOW_COMPRESSOR_STRICT_NEWTON", false)
}

/// Partial accept dans l'outer loop / continuation finale (désactivé si strict Newton).
pub fn compressor_accept_partial_enabled() -> bool {
    !compressor_strict_newton()
}

fn compressor_accept_partial() -> bool {
    compressor_accept_partial_enabled()
}

fn steady_config_for_outer_iter(
    base: SteadyStateConfig,
    mode: CompressorMapMode,
    _residual: f64,
) -> SteadyStateConfig {
    let mut config = base;
    if matches!(
        mode,
        CompressorMapMode::Measurement | CompressorMapMode::Biquadratic
    ) {
        config.disable_compressor_r2_cap = true;
    }
    config
}

#[derive(Debug, Clone, Copy)]
struct RatioUpdateContext {
    q_m3s_norm: f64,
    residual: f64,
    tolerance: f64,
    nominal_ratio: f64,
    pressure_cap_ratio: f64,
    relax: f64,
    allow_map_target: bool,
}

/// Pure guard for one compressor ratio relaxation step (unit-tested).
fn guarded_compressor_ratio_step(
    current: f64,
    map_target: f64,
    ctx: RatioUpdateContext,
) -> Option<f64> {
    if ctx.q_m3s_norm < FLOW_UPDATE_THRESHOLD_M3S {
        return None;
    }

    let converged = ctx.residual <= ctx.tolerance;
    let is_transport = ctx.pressure_cap_ratio >= TRANSPORT_NOMINAL_THRESHOLD;

    if !ctx.allow_map_target {
        if !converged && is_transport && current < ctx.nominal_ratio {
            let next = (current + ctx.relax * (ctx.nominal_ratio - current))
                .clamp(current, MAX_COMPRESSOR_RATIO);
            return ratio_step_if_changed(current, next);
        }
        return None;
    }

    if !converged {
        if !is_transport {
            return None;
        }
        let cap = ctx.pressure_cap_ratio.min(MAX_COMPRESSOR_RATIO);
        if ctx.allow_map_target && ctx.residual > ctx.tolerance * STUCK_RESIDUAL_RATIO_THRESHOLD {
            let goal = map_target.clamp(ctx.nominal_ratio, cap);
            let next =
                (current + ctx.relax * (goal - current)).clamp(MIN_COMPRESSOR_RATIO, cap);
            return ratio_step_if_changed(current, next);
        }
        // Au nominal (allow_map_target), monter vers max(nominal, carte) sans jamais baisser.
        let upward_goal = map_target.max(ctx.nominal_ratio);
        if current >= upward_goal {
            return None;
        }
        let next = (current + ctx.relax * (upward_goal - current)).clamp(current, cap);
        return ratio_step_if_changed(current, next);
    }

    let next = (current + ctx.relax * (map_target - current))
        .clamp(MIN_COMPRESSOR_RATIO, ctx.pressure_cap_ratio.min(MAX_COMPRESSOR_RATIO));
    ratio_step_if_changed(current, next)
}

fn ratio_step_if_changed(current: f64, next: f64) -> Option<f64> {
    if (next - current).abs() > 1e-6 {
        Some(next)
    } else {
        None
    }
}

/// Débit normal total livré (somme des sinks) au palier de continuation courant.
pub(crate) fn estimate_total_delivery_flow_m3s(
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    demands
        .values()
        .filter(|d| **d < 0.0)
        .map(|d| d.abs())
        .sum::<f64>()
        * demand_scale.max(0.0)
}

/// Estimation de débit normal par station quand le Newton n'a pas encore convergé (Q≈0).
pub fn estimate_station_norm_flow(
    active_compressors: usize,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    if active_compressors == 0 {
        return 0.0;
    }
    estimate_total_delivery_flow_m3s(demands, demand_scale) / active_compressors as f64
}

/// Débit normal estimé pour l'évaluation carte (topologie hub/branche + split distribution).
pub fn estimated_compressor_map_flow_m3s(
    network: &GasNetwork,
    pipe: &Pipe,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
) -> f64 {
    let active = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation && p.hydraulically_active())
        .count();
    let total = estimate_total_delivery_flow_m3s(demands, demand_scale);
    estimated_map_flow_m3s(network, pipe, total, active, Some(demands), demand_scale)
}

fn prefer_estimated_flow_for_map(result: &SolverResult, tolerance: f64) -> bool {
    result.residual > tolerance.max(HANDOFF_PREFER_ESTIMATED_RESIDUAL)
}

fn plausible_solver_flow(q_m3s: f64) -> bool {
    q_m3s >= FLOW_UPDATE_THRESHOLD_M3S && q_m3s <= MAX_PLAUSIBLE_COMPRESSOR_FLOW_M3S
}

fn effective_flow_for_map_update(
    network: &GasNetwork,
    result: &SolverResult,
    active_compressors: usize,
    demands: &HashMap<String, f64>,
    pipe: &Pipe,
    demand_scale: f64,
    prefer_estimated: bool,
) -> f64 {
    let total = estimate_total_delivery_flow_m3s(demands, demand_scale);
    let estimated = estimated_map_flow_m3s(
        network,
        pipe,
        total,
        active_compressors,
        Some(demands),
        demand_scale,
    );
    if prefer_estimated && estimated >= FLOW_UPDATE_THRESHOLD_M3S {
        return estimated;
    }
    let solver_q = result.flows.get(&pipe.id).copied().unwrap_or(0.0).abs();
    if plausible_solver_flow(solver_q) {
        return solver_q;
    }
    if estimated >= FLOW_UPDATE_THRESHOLD_M3S {
        estimated
    } else {
        solver_q
    }
}

fn map_inlet_pressure_bar(pipe: &Pipe, result: &SolverResult) -> f64 {
    let transport = pipe
        .equipment
        .compressor_pressure_cap_ratio
        .unwrap_or(1.0)
        >= TRANSPORT_NOMINAL_THRESHOLD;
    let p_in = result
        .pressures
        .get(&pipe.from)
        .copied()
        .unwrap_or(0.0);
    if p_in > 1.5 {
        p_in
    } else if transport {
        40.0
    } else {
        1.01325
    }
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

fn target_ratio_from_catalog(
    catalog: &CompressorCatalog,
    pipe: &Pipe,
    ctx: &CompressorOperatingContext,
    mode: CompressorMapMode,
) -> Option<f64> {
    let station = catalog.station(&pipe.id)?;
    let operating = pipe
        .equipment
        .compressor_nominal_ratio
        .or(pipe.compressor_ratio_max);
    let cap = pipe.equipment.compressor_pressure_cap_ratio;
    let ratio = effective_ratio_with_nominal_for_mode(
        station,
        ctx,
        operating,
        cap,
        mode == CompressorMapMode::Biquadratic,
    );
    Some(ratio.clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RatioUpdateStats {
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
    tolerance: f64,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
    allow_map_target: bool,
    prefer_estimated_flow: bool,
) -> RatioUpdateStats {
    let active_compressors = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation && p.hydraulically_active())
        .count();
    let map_flow_by_id: HashMap<String, f64> = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation && p.hydraulically_active())
        .map(|pipe| {
            (
                pipe.id.clone(),
                effective_flow_for_map_update(
                    network,
                    result,
                    active_compressors,
                    demands,
                    pipe,
                    demand_scale,
                    prefer_estimated_flow,
                ),
            )
        })
        .collect();
    let mut stats = RatioUpdateStats::default();
    for pipe in network.pipes_mut() {
        if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
            continue;
        }
        let q_m3s_norm = map_flow_by_id.get(&pipe.id).copied().unwrap_or(0.0);
        let ctx_op = CompressorOperatingContext {
            q_m3s_norm,
            p_in_bar: map_inlet_pressure_bar(pipe, result).max(1e-3),
            t_in_k: t_in_k.max(1.0),
        };
        let nominal = pipe
            .equipment
            .compressor_nominal_ratio
            .or(pipe.compressor_ratio_max)
            .unwrap_or(1.08)
            .clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO);
        let current = pipe
            .compressor_ratio_max
            .or(pipe.equipment.compressor_nominal_ratio)
            .unwrap_or(1.08)
            .clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO);
        let map_target = target_ratio_from_catalog(catalog, pipe, &ctx_op, mode).unwrap_or(current);
        let pressure_cap = pipe
            .equipment
            .compressor_pressure_cap_ratio
            .unwrap_or(MAX_COMPRESSOR_RATIO)
            .clamp(MIN_COMPRESSOR_RATIO, MAX_COMPRESSOR_RATIO);
        let update_ctx = RatioUpdateContext {
            q_m3s_norm,
            residual: result.residual,
            tolerance,
            nominal_ratio: nominal,
            pressure_cap_ratio: pressure_cap,
            relax,
            allow_map_target,
        };
        let Some(next) = guarded_compressor_ratio_step(current, map_target.min(pressure_cap), update_ctx) else {
            continue;
        };
        let delta = (next - current).abs();
        pipe.compressor_ratio_max = Some(next);
        stats.updated += 1;
        stats.max_delta = stats.max_delta.max(delta);
    }
    stats
}

/// Couplage Q–ratio après un palier de continuation réussi (scale ≥ 0.5).
pub fn apply_map_ratios_after_continuation_step(
    network: &mut GasNetwork,
    demands: &HashMap<String, f64>,
    demand_scale: f64,
    result: &SolverResult,
    mode: CompressorMapMode,
    tolerance: f64,
) -> RatioUpdateStats {
    if demand_scale < MAP_CONTINUATION_COUPLING_MIN_SCALE {
        return RatioUpdateStats::default();
    }
    let Some(catalog) = network.compressor_catalog.clone() else {
        return RatioUpdateStats::default();
    };
    if catalog.stations.is_empty() {
        return RatioUpdateStats::default();
    }
    let prefer_estimated = prefer_estimated_flow_for_map(result, tolerance);
    apply_compressor_map_updates(
        network,
        result,
        &catalog,
        mode,
        CONTINUATION_HANDOFF_RELAX,
        DEFAULT_GAS_TEMPERATURE_K,
        tolerance,
        demands,
        demand_scale,
        true,
        prefer_estimated,
    )
}

fn refine_map_with_demand_continuation<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    warm_start: Option<&HashMap<String, f64>>,
    config: SteadyStateConfig,
    mode: CompressorMapMode,
    on_progress: &mut F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    let mut warm = warm_start.cloned();
    let mut total_iterations = 0usize;
    let mut nominal_result: Option<SolverResult> = None;
    for &scale in MAP_REFINE_SCALES {
        let scaled_demands: HashMap<String, f64> = demands
            .iter()
            .map(|(id, q)| (id.clone(), q * scale))
            .collect();
        let step_network = network_with_scaled_compressor_lift(network, scale);
        let mut step_config = steady_config_for_outer_iter(config, mode, f64::INFINITY);
        step_config.accept_partial_solution = compressor_accept_partial();
        let mut result = solve_steady_state_with_progress(
            &step_network,
            &scaled_demands,
            warm.as_ref(),
            step_config,
            &mut *on_progress,
        )?;
        total_iterations += result.iterations;
        result.iterations = total_iterations;
        warm = Some(result.pressures.clone());
        if (scale - 1.0).abs() < 1e-9 {
            nominal_result = Some(result);
        }
    }
    nominal_result.ok_or_else(|| anyhow!("map refinement continuation produced no result"))
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

    let relax = compressor_relax();
    let max_iters = compressor_outer_max_iters();
    let t_in_k = DEFAULT_GAS_TEMPERATURE_K;
    let mut warm_start = initial_pressures.cloned();
    let mut adjusted_network = network.clone();
    let mut total_iterations = 0usize;
    let mut last_result: Option<SolverResult> = None;
    let mut best_result: Option<SolverResult> = None;

    for outer in 0..max_iters {
        let mut steady_config = if outer == 0 {
            steady_config_for_outer_iter(config, mode, f64::INFINITY)
        } else {
            steady_config_for_outer_iter(
                config,
                mode,
                last_result
                    .as_ref()
                    .map(|r| r.residual)
                    .unwrap_or(f64::INFINITY),
            )
        };
        steady_config.accept_partial_solution = compressor_accept_partial();
        let result = solve_steady_state_with_progress(
            &adjusted_network,
            demands,
            warm_start.as_ref(),
            steady_config,
            &mut *on_progress,
        )?;
        total_iterations += result.iterations;
        let mut result = result;
        result.iterations = total_iterations;
        warm_start = Some(result.pressures.clone());
        last_result = Some(result.clone());
        if best_result
            .as_ref()
            .is_none_or(|best| result.residual < best.residual)
        {
            best_result = Some(result.clone());
        }

        let updates = apply_compressor_map_updates(
            &mut adjusted_network,
            &result,
            catalog,
            mode,
            relax,
            t_in_k,
            config.tolerance,
            demands,
            1.0,
            true,
            prefer_estimated_flow_for_map(&result, config.tolerance),
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
            tracing::debug!(
                outer = outer + 1,
                residual = result.residual,
                "compressor map outer loop: ratios settled, stopping"
            );
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

    if let Some(result) = best_result {
        if result.residual <= config.tolerance {
            return Ok(result);
        }
        if let Ok(blended) = solve_legacy_blend_sequence(
            &adjusted_network,
            demands,
            warm_start.as_ref(),
            config,
            &mut *on_progress,
        ) && blended.residual < result.residual
        {
            tracing::info!(
                before = result.residual,
                after = blended.residual,
                "compressor map outer loop improved via legacy ratio blend polish"
            );
            return Ok(blended);
        }
        if let Ok(refined) = refine_map_with_demand_continuation(
            &adjusted_network,
            demands,
            warm_start.as_ref(),
            config,
            mode,
            &mut *on_progress,
        ) && refined.residual < result.residual
        {
            tracing::info!(
                before = result.residual,
                after = refined.residual,
                "compressor map outer loop improved via demand continuation refine"
            );
            return Ok(refined);
        }
        tracing::info!(
            residual = result.residual,
            tolerance = config.tolerance,
            "compressor map outer loop returning best partial hydraulic state"
        );
        return Ok(result);
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
    use std::collections::HashMap;

    use super::CompressorMapMode;
    use super::{FLOW_UPDATE_THRESHOLD_M3S, RatioUpdateContext, guarded_compressor_ratio_step};

    fn ctx(q: f64, residual: f64, tolerance: f64, nominal: f64, relax: f64) -> RatioUpdateContext {
        RatioUpdateContext {
            q_m3s_norm: q,
            residual,
            tolerance,
            nominal_ratio: nominal,
            pressure_cap_ratio: 4.09,
            relax,
            allow_map_target: true,
        }
    }

    #[test]
    fn compressor_map_mode_parses_measurement() {
        unsafe { std::env::set_var("GAZFLOW_COMPRESSOR_MAP_MODE", "measurement") };
        assert_eq!(super::compressor_map_mode(), CompressorMapMode::Measurement);
        unsafe { std::env::remove_var("GAZFLOW_COMPRESSOR_MAP_MODE") };
    }

    #[test]
    fn guarded_ratio_skips_low_flow() {
        let update = guarded_compressor_ratio_step(2.0, 1.5, ctx(0.005, 1e-7, 1e-6, 4.0, 0.5));
        assert!(update.is_none());
        assert!(0.005 < FLOW_UPDATE_THRESHOLD_M3S);
    }

    #[test]
    fn guarded_ratio_skips_non_transport_before_convergence() {
        let ctx = RatioUpdateContext {
            q_m3s_norm: 1.0,
            residual: 0.1,
            tolerance: 1e-6,
            nominal_ratio: 1.08,
            pressure_cap_ratio: 1.5,
            relax: 0.5,
            allow_map_target: true,
        };
        let update = guarded_compressor_ratio_step(1.2, 1.5, ctx);
        assert!(update.is_none());
    }

    #[test]
    fn guarded_ratio_transport_moves_upward_toward_nominal_before_convergence() {
        let update = guarded_compressor_ratio_step(2.5, 1.5, ctx(1.0, 0.01, 1e-6, 4.09, 0.5))
            .expect("transport should update upward");
        assert!(update > 2.5);
        assert!(update <= 4.09);
    }

    #[test]
    fn guarded_ratio_transport_blocks_downward_before_convergence() {
        let update =
            guarded_compressor_ratio_step(4.2, 1.5, ctx(1.0, 5e-6, 1e-6, 4.09, 0.5));
        assert!(update.is_none());
    }

    #[test]
    fn guarded_ratio_stuck_allows_bidirectional_toward_map_target() {
        let update = guarded_compressor_ratio_step(1.5, 1.1, ctx(1.0, 0.05, 1e-3, 1.08, 0.5))
            .expect("stuck residual should relax toward map target");
        assert!(update < 1.5);
        assert!((update - 1.3).abs() < 1e-9);
    }

    #[test]
    fn guarded_ratio_uses_map_target_when_converged() {
        let update = guarded_compressor_ratio_step(3.0, 2.0, ctx(1.0, 1e-7, 1e-6, 4.09, 0.5))
            .expect("converged update");
        assert!(update < 3.0);
        assert!((update - 2.5).abs() < 1e-9);
    }

    #[test]
    fn guarded_ratio_blocks_map_target_before_nominal() {
        let mut ctx = ctx(1.0, 1e-7, 1e-6, 4.09, 0.5);
        ctx.allow_map_target = false;
        let update = guarded_compressor_ratio_step(3.0, 2.0, ctx);
        assert!(update.is_none());
    }

    #[test]
    fn guarded_ratio_transport_moves_upward_toward_map_target_before_convergence() {
        let update = guarded_compressor_ratio_step(1.08, 1.11, ctx(18.0, 8.0, 3e-3, 1.08, 0.5))
            .expect("map target above nominal should update upward");
        assert!(update > 1.08);
        assert!(update <= 1.11);
    }

    #[test]
    fn guarded_ratio_stuck_relaxes_downward_toward_map_before_convergence() {
        let update = guarded_compressor_ratio_step(1.2, 1.11, ctx(18.0, 8.0, 3e-3, 1.08, 0.5))
            .expect("stuck transport should relax toward map target");
        assert!(update < 1.2);
        assert!((update - 1.155).abs() < 1e-9);
    }

    #[test]
    fn estimated_flow_splits_total_delivery_across_compressors() {
        let active = 2usize;
        let mut demands = HashMap::new();
        demands.insert("sink".into(), -120.0);
        let per_station = super::estimate_station_norm_flow(active, &demands, 1.0);
        assert!((per_station - 60.0).abs() < 1e-9);
    }
}
