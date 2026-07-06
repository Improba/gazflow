use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use faer::Mat;
use faer::prelude::Solve;
use faer::sparse::{SparseColMat, Triplet};
use rayon::prelude::*;

use crate::compressor::{
    CompressorCatalog, CompressorOperatingContext, compressor_energy_head_mismatch_kj_per_kg,
    effective_ratio_energy_closure_for_mode, effective_ratio_with_nominal_for_mode,
    head_mismatch_penalty_psq, isentropic_outlet_temperature_k,
};
use crate::gaslib::{
    compressor_decision_variables_enabled, compressor_hard_coupling_enabled,
    detect_shortpipe_boundary_pairs, scenario_boundary_active_envelopes_enabled,
    scenario_boundary_partial_accept_enabled, scenario_pressure_clamp_in_newton_enabled,
    scenario_pressure_envelopes_enabled, scenario_pressure_in_newton_enabled,
    scenario_pressure_penalty_weight, shortpipe_merge_boundaries_enabled,
};
use crate::graph::{ConnectionKind, GasNetwork};

use super::config::SteadyStateConfig;
use super::compressor_loop::{compressor_map_mode, CompressorMapMode};
use super::gas_properties::{
    DEFAULT_GAS_TEMPERATURE_K, GasComposition, gas_density_kg_per_m3_with_composition,
};
use super::iterative::solve_sparse_gmres_ilu0;
use super::steady_state::{
    NondimScaling, PipeElevationContext, SolverControl, SolverProgress, SolverResult,
    compressor_pressure_from_coeff_for_config, compressor_r2_cap_disabled,
    effective_compressor_pressure_from_coeff,
    effective_compressor_pressure_from_coeff_enthalpic, effective_pipe_geometry,
    flow_and_conductance, flow_reference_from_demands, gravity_dp_sq_bar,
    gravity_dp_sq_derivatives_wrt_pressure_sq, pipe_flow_with_gravity,
    pipe_resistance_at_pressure_with_composition, pressure_sq_reference_from_fixed,
};

const MIN_PRESSURE_SQ: f64 = 1.0;
const MIN_ABS_DP: f64 = 1e-10;

#[derive(Debug, Clone)]
struct PressureBoundContext {
    lower_bar: Vec<Option<f64>>,
    upper_bar: Vec<Option<f64>>,
    penalty_weight: f64,
    clamp_in_line_search: bool,
}

impl PressureBoundContext {
    fn from_network(network: &GasNetwork, node_ids: &[String]) -> Self {
        let mut lower_bar = Vec::with_capacity(node_ids.len());
        let mut upper_bar = Vec::with_capacity(node_ids.len());
        for id in node_ids {
            if network.scenario_pressure_envelope_nodes.contains(id) {
                if let Some(n) = network.nodes().find(|node| node.id == *id) {
                    lower_bar.push(n.pressure_lower_bar);
                    upper_bar.push(n.pressure_upper_bar);
                } else {
                    lower_bar.push(None);
                    upper_bar.push(None);
                }
            } else {
                lower_bar.push(None);
                upper_bar.push(None);
            }
        }
        Self {
            lower_bar,
            upper_bar,
            penalty_weight: scenario_pressure_penalty_weight(),
            clamp_in_line_search: scenario_pressure_clamp_in_newton_enabled(),
        }
    }

    fn clamp_idx(&self, idx: usize, pressure_sq: f64) -> f64 {
        if !self.clamp_in_line_search {
            return pressure_sq.max(MIN_PRESSURE_SQ);
        }
        let mut v = pressure_sq.max(MIN_PRESSURE_SQ);
        if let Some(lo) = self.lower_bar.get(idx).and_then(|o| *o) {
            v = v.max(lo * lo);
        }
        if let Some(hi) = self.upper_bar.get(idx).and_then(|o| *o) {
            v = v.min(hi * hi);
        }
        v
    }

    fn violation_m3s(&self, idx: usize, pressure_bar: f64) -> f64 {
        let mut viol = 0.0_f64;
        if let Some(lo) = self.lower_bar.get(idx).and_then(|o| *o) {
            viol = viol.max((lo - pressure_bar).max(0.0) * self.penalty_weight);
        }
        if let Some(hi) = self.upper_bar.get(idx).and_then(|o| *o) {
            viol = viol.max((pressure_bar - hi).max(0.0) * self.penalty_weight);
        }
        viol
    }

    fn max_free_violation_m3s(
        &self,
        free_indices: &[usize],
        pressures_sq: &[f64],
    ) -> f64 {
        free_indices
            .iter()
            .map(|&idx| self.violation_m3s(idx, pressures_sq[idx].sqrt()))
            .fold(0.0_f64, f64::max)
    }
}

/// Alias pression / bilan massique pour couples shortPipe (source esclave → sink maître).
struct ShortPipeAliasContext {
    resolve: Vec<usize>,
    /// (slave, master, ratio_sq) — `P_slave² = ratio_sq · P_master²`.
    /// ratio_sq = 1.0 pour les fusions shortPipe (P_slave = P_master) ;
    /// ratio_sq = r² pour les outlets compresseurs en couplage dur (Phase IV).
    slaves: Vec<(usize, usize, f64)>,
    /// Facteur de ratio par nœud (r² si le nœud est un outlet compresseur couplé,
    /// 1.0 sinon). Sert à calculer le `from/to_pressure_factor` des `IndexedPipe`.
    ratio_factor: Vec<f64>,
}

impl ShortPipeAliasContext {
    fn from_network(network: &GasNetwork, id_pos: &HashMap<String, usize>, n: usize) -> Self {
        let shortpipe_merge = shortpipe_merge_boundaries_enabled();
        let hard_coupling = compressor_hard_coupling_enabled()
            && compressor_decision_variables_enabled();
        // L'alias n'a rien à faire si ni la fusion shortPipe ni le couplage dur
        // compresseur ne sont actifs. NB: le couplage dur doit fonctionner même
        // sans shortpipe_merge (sinon les outlets compresseurs sont déconnectés
        // sans couplage → pressions flottantes sans signification physique).
        if !shortpipe_merge && !hard_coupling {
            return Self {
                resolve: (0..n).collect(),
                slaves: Vec::new(),
                ratio_factor: vec![1.0; n],
            };
        }
        let mut resolve = (0..n).collect::<Vec<_>>();
        let mut slaves = Vec::new();
        let mut ratio_factor = vec![1.0_f64; n];
        if shortpipe_merge {
            for pair in detect_shortpipe_boundary_pairs(network) {
                let Some(&master) = id_pos.get(&pair.sink_id) else {
                    continue;
                };
                let Some(&slave) = id_pos.get(&pair.source_id) else {
                    continue;
                };
                // Garde-fou : une source ancrée (entry transport, pressure_fixed_bar)
                // doit rester maître à pression fixée. L'aliasing en esclave écraserait
                // sa pression transport (70 bar) par celle, basse, du sink partenaire.
                if let Some(src_node) = network.nodes().find(|n| n.id == pair.source_id) {
                    if src_node.pressure_fixed_bar.is_some() {
                        continue;
                    }
                }
                resolve[slave] = master;
                slaves.push((slave, master, 1.0));
            }
        }

        // Phase IV : couplage dur compresseur. L'outlet (to) devient esclave de
        // l'inlet (from) avec P_out² = r²·P_in². L'arc compresseur est retiré du
        // graphe de flow (cf. IndexedPipe build) ; la fusion de mass-balance
        // (remap resolve + merge demand) réutilise la mécanique shortPipe.
        if hard_coupling {
            for pipe in network.pipes() {
                if pipe.kind != ConnectionKind::CompressorStation || !pipe.hydraulically_active() {
                    continue;
                }
                let Some(&outlet) = id_pos.get(&pipe.to) else { continue; };
                let Some(&inlet) = id_pos.get(&pipe.from) else { continue; };
                // Respecte le resolve shortPipe déjà appliqué (inlet/outlet via leur canonical).
                let inlet_c = resolve[inlet];
                let outlet_c = resolve[outlet];
                if outlet_c == inlet_c {
                    continue;
                }
                // Un nœud déjà esclave (shortPipe ou autre compresseur) n'est pas ré-aliasé.
                if resolve[outlet_c] != outlet_c {
                    continue;
                }
                let r = pipe
                    .compressor_ratio_max
                    .or(pipe.equipment.compressor_nominal_ratio)
                    .unwrap_or(1.08)
                    .max(1.0);
                if !r.is_finite() || r <= 1.0 + 1e-9 {
                    continue;
                }
                resolve[outlet_c] = inlet_c;
                let r2 = r * r;
                slaves.push((outlet_c, inlet_c, r2));
                ratio_factor[outlet_c] = r2;
            }
        }

        for i in 0..n {
            let mut r = i;
            while resolve[r] != r {
                r = resolve[r];
            }
            let mut cur = i;
            while resolve[cur] != cur {
                let next = resolve[cur];
                resolve[cur] = r;
                cur = next;
            }
        }
        Self { resolve, slaves, ratio_factor }
    }

    fn is_canonical(&self, idx: usize) -> bool {
        self.resolve.get(idx).is_some_and(|&r| r == idx)
    }

    fn sync_pressures(&self, pressures_sq: &mut [f64]) {
        for &(slave, master, ratio_sq) in &self.slaves {
            pressures_sq[slave] = ratio_sq * pressures_sq[master];
        }
    }

    fn merge_demands_vec(&self, demands_vec: &mut [f64]) {
        for &(slave, master, _) in &self.slaves {
            demands_vec[master] += demands_vec[slave];
            demands_vec[slave] = 0.0;
        }
    }

    fn inherit_fixed(&self, fixed: &mut HashMap<usize, f64>) {
        for &(slave, master, ratio_sq) in &self.slaves {
            if let Some(&master_p_sq) = fixed.get(&master) {
                fixed.insert(slave, master_p_sq * ratio_sq);
                continue;
            }
            if let Some(&slave_p_sq) = fixed.get(&slave) {
                fixed.insert(master, (slave_p_sq / ratio_sq).max(MIN_PRESSURE_SQ));
            }
        }
    }

    /// Facteur de pression (r²) à appliquer sur l'endpoint d'un pipe dont le
    /// nœud brut est `raw_idx`. 1.0 sauf pour les outlets compresseurs couplés.
    fn pressure_factor_for(&self, raw_idx: usize) -> f64 {
        self.ratio_factor.get(raw_idx).copied().unwrap_or(1.0)
    }
}

fn step_free_pressure_sq(
    current: f64,
    delta: f64,
    idx: usize,
    pressure_bounds: Option<&PressureBoundContext>,
) -> f64 {
    let raw = current + delta;
    if let Some(bounds) = pressure_bounds {
        if bounds.clamp_in_line_search {
            bounds.clamp_idx(idx, raw)
        } else {
            raw.max(MIN_PRESSURE_SQ)
        }
    } else {
        raw.max(MIN_PRESSURE_SQ)
    }
}

const JACOBI_RELAX: f64 = 0.8;
const MAX_BACKTRACK_STEPS: usize = 5;
const PIVOT_EPS: f64 = 1e-14;
const PARALLEL_PIPE_THRESHOLD: usize = 50;
const GMRES_RESTART: usize = 30;
const GMRES_MAX_ITERS: usize = 300;
const GMRES_TOL: f64 = 1e-8;
const PHYSICAL_INIT_RELAX: f64 = 0.7;
const DENSE_FALLBACK_MAX_SIZE: usize = 700;
const SPARSE_LU_MAX_SIZE: usize = 2500;
const REGULATOR_ROW_FD_REL_STEP: f64 = 1e-6;
static SPARSE_LU_ENABLED: AtomicBool = AtomicBool::new(true);

fn disable_jacobi_fallback() -> bool {
    std::env::var("GAZFLOW_DISABLE_JACOBI_FALLBACK")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn env_usize_opt(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn physical_init_iters(node_count: usize) -> usize {
    if let Some(v) = env_usize_opt("GAZFLOW_PHYSICAL_INIT_ITERS") {
        return v;
    }
    match node_count {
        n if n > 2000 => 4,
        n if n > 150 => 12,
        _ => 0,
    }
}

#[derive(Debug, Clone)]
struct IndexedPipe {
    id: String,
    from_idx: usize,
    to_idx: usize,
    kind: ConnectionKind,
    length_km: f64,
    diameter_mm: f64,
    roughness_mm: f64,
    pressure_from_coeff: f64,
    operating_ratio: Option<f64>,
    pressure_cap_ratio: Option<f64>,
    height_from_m: f64,
    height_to_m: f64,
    /// Facteur de pression (r²) sur l'endpoint `from` pour le couplage dur
    /// compresseur (P_from²_effectif = from_pressure_factor · P_from²). 1.0 sinon.
    from_pressure_factor: f64,
    /// Idem sur l'endpoint `to`.
    to_pressure_factor: f64,
}

struct NewtonMapContext<'a> {
    catalog: &'a CompressorCatalog,
    prefer_biquadratic: bool,
    disable_r2_cap: bool,
    /// Dérivées implicites ∂(coeff carte)/∂Q et ∂/∂P_amont (v19 head-Jacobian).
    head_jac: bool,
    /// Bilan enthalpique in-Newton (v20) : cap achieved-ratio assoupli (1,08×) + dérivées carte.
    enthalpic: bool,
    /// Fermeture énergétique H_map(Q) ↔ H_req(P_in,P_out) in-Newton (v21).
    energy_closure: bool,
    /// Équation énergétique explicite H_map − H_req dans Δ(P²) (v22).
    energy_equation: bool,
    /// T_sortie isentrope pour ρ aval des compresseurs (v22).
    discharge_t_out: bool,
    energy_penalty_weight: f64,
    /// Nœud aval compresseur → (from_idx, to_idx) de la station.
    compressor_discharge_nodes: HashMap<usize, (usize, usize)>,
}

const DEFAULT_COMPRESSOR_GAMMA: f64 = 1.3;

fn newton_compressor_energy_equation_enabled(_config: &SteadyStateConfig) -> bool {
    env_bool("GAZFLOW_COMPRESSOR_ENERGY_EQUATION", false)
}

fn newton_compressor_discharge_t_out_enabled(config: &SteadyStateConfig) -> bool {
    if newton_compressor_energy_equation_enabled(config) {
        return env_bool("GAZFLOW_COMPRESSOR_DISCHARGE_T_OUT", true);
    }
    env_bool("GAZFLOW_COMPRESSOR_DISCHARGE_T_OUT", false)
}

fn compressor_energy_penalty_weight() -> f64 {
    std::env::var("GAZFLOW_COMPRESSOR_ENERGY_PENALTY_WEIGHT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|w| w.is_finite() && *w > 0.0)
        .unwrap_or(0.35)
}

fn newton_compressor_energy_closure_enabled(_config: &SteadyStateConfig) -> bool {
    if newton_compressor_energy_equation_enabled(_config) {
        return false;
    }
    env_bool("GAZFLOW_COMPRESSOR_ENERGY_CLOSURE", false)
}

fn newton_compressor_enthalpic_enabled(config: &SteadyStateConfig) -> bool {
    newton_compressor_energy_equation_enabled(config)
        || newton_compressor_energy_closure_enabled(config)
        || env_bool("GAZFLOW_COMPRESSOR_ENTHALPIC", false)
}

fn newton_compressor_head_jac_enabled(_config: &SteadyStateConfig) -> bool {
    // Opt-in : bench mild_618 montre une légère régression avec défaut ON (2,045 vs 2,0 m³/s).
    // v20 enthalpique active implicitement les dérivées carte.
    if newton_compressor_enthalpic_enabled(_config) {
        return true;
    }
    env_bool("GAZFLOW_NEWTON_COMPRESSOR_HEAD_JAC", false)
}

fn compressor_coeff_from_map_with_p_in(
    pipe: &IndexedPipe,
    pressures_sq: &[f64],
    q_m3s_norm: f64,
    map_ctx: &NewtonMapContext<'_>,
    p_in_bar: f64,
) -> f64 {
    let Some(station) = map_ctx.catalog.station(&pipe.id) else {
        return pipe.pressure_from_coeff;
    };
    let p_in = p_in_bar.max(1e-3);
    let p_out = pressures_sq[pipe.to_idx].sqrt().max(1e-3);
    let ctx = CompressorOperatingContext {
        q_m3s_norm: q_m3s_norm.abs().max(1e-3),
        p_in_bar: p_in,
        t_in_k: DEFAULT_GAS_TEMPERATURE_K,
    };
    let ratio = if map_ctx.energy_closure {
        effective_ratio_energy_closure_for_mode(
            station,
            &ctx,
            p_out,
            pipe.operating_ratio,
            pipe.pressure_cap_ratio,
            map_ctx.prefer_biquadratic,
        )
    } else {
        effective_ratio_with_nominal_for_mode(
            station,
            &ctx,
            pipe.operating_ratio,
            pipe.pressure_cap_ratio,
            map_ctx.prefer_biquadratic,
        )
    };
    let mut target_coeff = ratio.powi(2);
    if ratio > 3.0 && !map_ctx.disable_r2_cap && !map_ctx.enthalpic && !map_ctx.energy_closure && !map_ctx.energy_equation {
        target_coeff = target_coeff.min(9.0);
    }
    if map_ctx.enthalpic || map_ctx.energy_closure || map_ctx.energy_equation {
        effective_compressor_pressure_from_coeff_enthalpic(
            target_coeff,
            p_in * p_in,
            pressures_sq[pipe.to_idx],
        )
    } else {
        effective_compressor_pressure_from_coeff(
            target_coeff,
            p_in * p_in,
            pressures_sq[pipe.to_idx],
        )
    }
}

fn compressor_coeff_from_map(
    pipe: &IndexedPipe,
    pressures_sq: &[f64],
    q_m3s_norm: f64,
    map_ctx: &NewtonMapContext<'_>,
) -> f64 {
    compressor_coeff_from_map_with_p_in(
        pipe,
        pressures_sq,
        q_m3s_norm,
        map_ctx,
        pressures_sq[pipe.from_idx].sqrt().max(1e-3),
    )
}

/// Sensibilités numériques du coefficient P² issu de la carte (Q normatif, P_amont).
fn compressor_map_coeff_sensitivities(
    pipe: &IndexedPipe,
    pressures_sq: &[f64],
    q_abs: f64,
    map_ctx: &NewtonMapContext<'_>,
) -> (f64, f64, f64, f64) {
    let p_from = pressures_sq[pipe.from_idx].sqrt().max(1e-3);
    let p_to = pressures_sq[pipe.to_idx].sqrt().max(1e-3);
    let c = compressor_coeff_from_map(pipe, pressures_sq, q_abs, map_ctx);
    let hq = (q_abs.abs() * 1e-4).max(1e-5);
    let dc_dq = (compressor_coeff_from_map(pipe, pressures_sq, q_abs + hq, map_ctx) - c) / hq;
    let hp = (p_from * 1e-4).max(1e-5);
    let dc_dp_in = (compressor_coeff_from_map_with_p_in(
        pipe,
        pressures_sq,
        q_abs,
        map_ctx,
        p_from + hp,
    ) - c)
        / hp;
    let mut dc_dp_out = 0.0;
    if map_ctx.energy_closure {
        let hp_out = (p_to * 1e-4).max(1e-5);
        let mut perturbed = pressures_sq.to_vec();
        perturbed[pipe.to_idx] = (p_to + hp_out).powi(2);
        dc_dp_out =
            (compressor_coeff_from_map(pipe, &perturbed, q_abs, map_ctx) - c) / hp_out;
    }
    (c, dc_dq, dc_dp_in, dc_dp_out)
}

fn inlet_temperature_k_for_pipe(
    pipe: &IndexedPipe,
    map_ctx: Option<&NewtonMapContext<'_>>,
    pressures_sq: &[f64],
) -> f64 {
    let Some(ctx) = map_ctx else {
        return DEFAULT_GAS_TEMPERATURE_K;
    };
    if !ctx.discharge_t_out {
        return DEFAULT_GAS_TEMPERATURE_K;
    }
    let Some(&(cs_from, cs_to)) = ctx.compressor_discharge_nodes.get(&pipe.from_idx) else {
        return DEFAULT_GAS_TEMPERATURE_K;
    };
    let p_in = pressures_sq[cs_from].sqrt().max(1e-3);
    let p_out = pressures_sq[cs_to].sqrt().max(1e-3);
    isentropic_outlet_temperature_k(
        DEFAULT_GAS_TEMPERATURE_K,
        p_in,
        p_out,
        DEFAULT_COMPRESSOR_GAMMA,
    )
}

fn compressor_energy_penalty_psq(
    pipe: &IndexedPipe,
    map_ctx: &NewtonMapContext<'_>,
    p_in: f64,
    p_out: f64,
    q_abs: f64,
    p_ref: f64,
) -> f64 {
    if !map_ctx.energy_equation || pipe.kind != ConnectionKind::CompressorStation {
        return 0.0;
    }
    let Some(station) = map_ctx.catalog.station(&pipe.id) else {
        return 0.0;
    };
    let ctx = CompressorOperatingContext {
        q_m3s_norm: q_abs.max(1e-3),
        p_in_bar: p_in.max(1e-3),
        t_in_k: DEFAULT_GAS_TEMPERATURE_K,
    };
    let (_, _, delta_h) = compressor_energy_head_mismatch_kj_per_kg(
        station,
        &ctx,
        p_out.max(1e-3),
        map_ctx.prefer_biquadratic,
    );
    head_mismatch_penalty_psq(delta_h, p_ref.max(1e-3), map_ctx.energy_penalty_weight)
}

fn newton_compressor_map_enabled(config: &SteadyStateConfig) -> bool {
    if newton_compressor_enthalpic_enabled(config)
        || newton_compressor_energy_closure_enabled(config)
        || newton_compressor_energy_equation_enabled(config)
    {
        return true;
    }
    env_bool(
        "GAZFLOW_NEWTON_COMPRESSOR_MAP",
        matches!(
            compressor_map_mode(),
            CompressorMapMode::Measurement | CompressorMapMode::Biquadratic
        ),
    )
}

#[derive(Debug, Clone)]
struct IterationState {
    f_node: Vec<f64>,
    j_diag: Vec<f64>,
    flows: Vec<f64>,
    conductances_from: Vec<f64>,
    conductances_to: Vec<f64>,
    residual: f64,
}

pub(crate) fn solve_steady_state_newton_hybrid<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    config: &SteadyStateConfig,
    mut on_progress: F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    solve_steady_state_newton_hybrid_with_options(
        network,
        demands,
        initial_pressures_bar,
        config,
        &mut on_progress,
        true,
    )
}

fn solve_steady_state_newton_hybrid_with_options<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    config: &SteadyStateConfig,
    mut on_progress: F,
    enable_active_regulator_row_coupling: bool,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    let SteadyStateConfig {
        gas_composition,
        max_iter,
        tolerance,
        snapshot_every,
        enable_compressor_outer_loop: _,
        disable_compressor_r2_cap: _,
        accept_partial_solution: _,
    } = *config;
    let n = network.node_count();
    if n == 0 {
        return Ok(SolverResult::from_core(
            HashMap::new(),
            HashMap::new(),
            0,
            0.0,
        ));
    }

    let node_ids: Vec<String> = network.nodes().map(|n| n.id.clone()).collect();
    let id_pos: HashMap<String, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();

    let shortpipe_alias = ShortPipeAliasContext::from_network(network, &id_pos, n);
    let hard_coupling_active = compressor_hard_coupling_enabled()
        && compressor_decision_variables_enabled();

    let pressure_bounds = if scenario_pressure_envelopes_enabled()
        && scenario_pressure_in_newton_enabled()
    {
        Some(PressureBoundContext::from_network(network, &node_ids))
    } else {
        None
    };
    let pressure_bounds_ref = pressure_bounds.as_ref();

    let mut fixed: HashMap<usize, f64> = network
        .nodes()
        .filter_map(|n| {
            n.pressure_fixed_bar
                .map(|p| (*id_pos.get(&n.id).unwrap(), p * p))
        })
        .collect();

    let mut pressures_sq = vec![70.0_f64.powi(2); n];
    if let Some(init_map) = initial_pressures_bar {
        for (node_id, &pressure_bar) in init_map {
            if !pressure_bar.is_finite() || pressure_bar <= 0.0 {
                continue;
            }
            if let Some(&idx) = id_pos.get(node_id) {
                pressures_sq[idx] = pressure_bar * pressure_bar;
            }
        }
    }
    for (&idx, &p_sq) in &fixed {
        pressures_sq[idx] = p_sq;
    }

    let mut demands_vec = vec![0.0_f64; n];
    for (id, &demand) in demands {
        if !demand.is_finite() {
            bail!("invalid demand value for node '{id}': {demand}");
        }
        let Some(&idx) = id_pos.get(id) else {
            bail!("unknown demand node id: '{id}'");
        };
        demands_vec[idx] += demand;
    }
    shortpipe_alias.merge_demands_vec(&mut demands_vec);

    let node_heights: HashMap<String, f64> = network
        .nodes()
        .map(|node| (node.id.clone(), node.height_m))
        .collect();

    let pipes: Vec<IndexedPipe> = network
        .pipes()
        .filter_map(|pipe| {
            if !pipe.hydraulically_active() {
                return None;
            }
            let raw_from = *id_pos.get(&pipe.from)?;
            let raw_to = *id_pos.get(&pipe.to)?;
            let from_idx = shortpipe_alias.resolve[raw_from];
            let to_idx = shortpipe_alias.resolve[raw_to];
            if from_idx == to_idx {
                return None;
            }
            // Phase IV : en couplage dur, l'arc compresseur est retiré du graphe de
            // flow (remplacé par l'alias ratio P_out² = r²·P_in² sur l'outlet).
            if hard_coupling_active && pipe.kind == ConnectionKind::CompressorStation {
                return None;
            }
            let (length_km, diameter_mm, roughness_mm) = effective_pipe_geometry(pipe);
            Some(IndexedPipe {
                id: pipe.id.clone(),
                from_idx,
                to_idx,
                kind: pipe.kind,
                length_km,
                diameter_mm,
                roughness_mm,
                pressure_from_coeff: compressor_pressure_from_coeff_for_config(pipe, config),
                operating_ratio: pipe
                    .compressor_ratio_max
                    .or(pipe.equipment.compressor_nominal_ratio),
                pressure_cap_ratio: pipe.equipment.compressor_pressure_cap_ratio,
                height_from_m: node_heights.get(&pipe.from).copied().unwrap_or(0.0),
                height_to_m: node_heights.get(&pipe.to).copied().unwrap_or(0.0),
                from_pressure_factor: shortpipe_alias.pressure_factor_for(raw_from),
                to_pressure_factor: shortpipe_alias.pressure_factor_for(raw_to),
            })
        })
        .collect();

    let mut adjacency = vec![Vec::<usize>::new(); n];
    for pipe in &pipes {
        adjacency[pipe.from_idx].push(pipe.to_idx);
        adjacency[pipe.to_idx].push(pipe.from_idx);
    }

    let mut visited = vec![false; n];
    let mut anchored_components = 0usize;
    for start_idx in 0..n {
        if visited[start_idx] {
            continue;
        }
        if !shortpipe_alias.is_canonical(start_idx) {
            visited[start_idx] = true;
            continue;
        }

        let mut stack = vec![start_idx];
        let mut component = Vec::<usize>::new();
        visited[start_idx] = true;

        while let Some(node_idx) = stack.pop() {
            component.push(node_idx);
            for &neighbor_idx in &adjacency[node_idx] {
                if !visited[neighbor_idx] {
                    visited[neighbor_idx] = true;
                    stack.push(neighbor_idx);
                }
            }
        }

        if component.iter().any(|idx| fixed.contains_key(idx)) {
            continue;
        }

        // Stabilisation numérique uniquement : une pression de référence par composante
        // flottante (pas une condition aux limites GasLib ; pressureMin/Max ne sont pas utilisés).
        let anchor = component
            .iter()
            .copied()
            .filter(|&idx| demands_vec[idx].abs() > 0.0)
            .max_by(|&a, &b| {
                demands_vec[a]
                    .abs()
                    .total_cmp(&demands_vec[b].abs())
                    .then_with(|| b.cmp(&a))
            })
            .map(|idx| (idx, pressures_sq[idx].sqrt()))
            .or_else(|| {
                component
                    .iter()
                    .copied()
                    .min()
                    .map(|idx| (idx, pressures_sq[idx].sqrt().max(1.0)))
            })
            .or_else(|| component.iter().copied().min().map(|idx| (idx, 70.0)));

        if let Some((anchor_idx, anchor_pressure_bar)) = anchor {
            let p_sq = anchor_pressure_bar * anchor_pressure_bar;
            fixed.insert(anchor_idx, p_sq);
            pressures_sq[anchor_idx] = p_sq;
            anchored_components += 1;
        }
    }

    tracing::debug!(
        "anchored {} floating components (no pressure reference)",
        anchored_components
    );

    shortpipe_alias.inherit_fixed(&mut fixed);
    shortpipe_alias.sync_pressures(&mut pressures_sq);

    let free_indices: Vec<usize> = (0..n)
        .filter(|&i| !fixed.contains_key(&i) && shortpipe_alias.is_canonical(i))
        .collect();
    let mut free_pos = vec![usize::MAX; n];
    for (pos, &node_idx) in free_indices.iter().enumerate() {
        free_pos[node_idx] = pos;
    }
    let scaling = NondimScaling::new(
        pressure_sq_reference_from_fixed(&fixed),
        flow_reference_from_demands(&demands_vec),
    );
    let active_regulator_nodes = collect_active_regulator_nodes(network, &id_pos, &fixed);
    let guard_jacobi_fallback = env_bool("GAZFLOW_GUARD_JACOBI_FALLBACK", n > 2000);
    let newton_map_ctx = if newton_compressor_map_enabled(config) {
        let mut compressor_discharge_nodes = HashMap::new();
        for pipe in &pipes {
            if pipe.kind == ConnectionKind::CompressorStation {
                compressor_discharge_nodes.insert(pipe.to_idx, (pipe.from_idx, pipe.to_idx));
            }
        }
        network.compressor_catalog.as_ref().map(|catalog| NewtonMapContext {
            catalog,
            prefer_biquadratic: matches!(compressor_map_mode(), CompressorMapMode::Biquadratic),
            disable_r2_cap: compressor_r2_cap_disabled(config),
            head_jac: newton_compressor_head_jac_enabled(config),
            enthalpic: newton_compressor_enthalpic_enabled(config),
            energy_closure: newton_compressor_energy_closure_enabled(config),
            energy_equation: newton_compressor_energy_equation_enabled(config),
            discharge_t_out: newton_compressor_discharge_t_out_enabled(config),
            energy_penalty_weight: compressor_energy_penalty_weight(),
            compressor_discharge_nodes,
        })
    } else {
        None
    };

    if initial_pressures_bar.is_none()
        && !free_indices.is_empty()
        && let Some(candidate) = build_physical_initial_guess(
            n,
            &pipes,
            &demands_vec,
            &fixed,
            scaling.pressure_sq_ref,
            gas_composition,
        )
    {
        let eval = NewtonEvalContext {
            pipes: &pipes,
            demands_vec: &demands_vec,
            free_indices: &free_indices,
            scaling,
            gas_composition,
            map_ctx: newton_map_ctx.as_ref(),
            pressure_bounds: pressure_bounds_ref,
        };
        let baseline_residual = eval.evaluate(&pressures_sq).residual;
        let candidate_residual = eval.evaluate(&candidate).residual;
        if candidate_residual.is_finite() && candidate_residual < baseline_residual {
            pressures_sq = candidate;
        }
    }

    let mut iterations = 0usize;
    let disable_jacobi = disable_jacobi_fallback();
    for _iter in 0..max_iter {
        let state = evaluate_state(
            &pipes,
            &demands_vec,
            &pressures_sq,
            &free_indices,
            scaling,
            gas_composition,
            newton_map_ctx.as_ref(),
            pressure_bounds_ref,
        );
        let residual = state.residual;
        iterations += 1;

        let snapshot_due = snapshot_every > 0 && iterations.is_multiple_of(snapshot_every);
        let progress = if snapshot_due {
            SolverProgress {
                iter: iterations,
                residual,
                pressures: Some(build_pressure_map(&node_ids, &pressures_sq)),
                flows: Some(build_flow_map(&pipes, &state.flows)),
            }
        } else {
            SolverProgress {
                iter: iterations,
                residual,
                pressures: None,
                flows: None,
            }
        };
        if on_progress(progress) == SolverControl::Cancel {
            bail!("simulation cancelled by callback");
        }

        if residual < tolerance || free_indices.is_empty() {
            break;
        }

        let m = free_indices.len();
        let mut rhs: Vec<f64> = free_indices.iter().map(|&idx| -state.f_node[idx]).collect();
        let mut jacobian_triplets: Vec<(usize, usize, f64)> =
            if pipes.len() >= PARALLEL_PIPE_THRESHOLD {
                pipes
                    .par_iter()
                    .enumerate()
                    .fold(
                        Vec::<(usize, usize, f64)>::new,
                        |mut acc, (pipe_idx, pipe)| {
                            let g_from = state.conductances_from[pipe_idx];
                            let g_to = state.conductances_to[pipe_idx];
                            let a_free = free_pos[pipe.from_idx];
                            let b_free = free_pos[pipe.to_idx];
                            if a_free != usize::MAX {
                                acc.push((a_free, a_free, -g_from));
                            }
                            if b_free != usize::MAX {
                                acc.push((b_free, b_free, -g_to));
                            }
                            if a_free != usize::MAX && b_free != usize::MAX {
                                acc.push((a_free, b_free, g_to));
                                acc.push((b_free, a_free, g_from));
                            }
                            acc
                        },
                    )
                    .reduce(Vec::new, |mut a, mut b| {
                        a.append(&mut b);
                        a
                    })
            } else {
                let mut triplets = Vec::<(usize, usize, f64)>::with_capacity(pipes.len() * 4);
                for (pipe_idx, pipe) in pipes.iter().enumerate() {
                    let g_from = state.conductances_from[pipe_idx];
                    let g_to = state.conductances_to[pipe_idx];
                    let a_free = free_pos[pipe.from_idx];
                    let b_free = free_pos[pipe.to_idx];

                    if a_free != usize::MAX {
                        triplets.push((a_free, a_free, -g_from));
                    }
                    if b_free != usize::MAX {
                        triplets.push((b_free, b_free, -g_to));
                    }
                    if a_free != usize::MAX && b_free != usize::MAX {
                        triplets.push((a_free, b_free, g_to));
                        triplets.push((b_free, a_free, g_from));
                    }
                }
                triplets
            };

        if enable_active_regulator_row_coupling {
            append_active_regulator_row_coupling(
                &mut jacobian_triplets,
                &active_regulator_nodes,
                &pipes,
                &free_pos,
                &state,
                &demands_vec,
                &pressures_sq,
                &free_indices,
                &mut rhs,
                scaling,
                gas_composition,
                newton_map_ctx.as_ref(),
            );
        }

        // Dérivées de la pénalité enveloppe P dans le Jacobian sparse (dual Q+P).
        // f_node += w*shortfall (shortfall=lo-p) => d(f)/d(p_sq) = -w/(2p).
        if let Some(bounds) = pressure_bounds_ref {
            for &idx in free_indices.iter() {
                let p = pressures_sq[idx].sqrt().max(1e-3);
                let pos = free_pos[idx];
                if pos == usize::MAX {
                    continue;
                }
                if let Some(lo) = bounds.lower_bar.get(idx).and_then(|o| *o) {
                    if (lo - p).max(0.0) > 0.0 {
                        jacobian_triplets.push((pos, pos, -bounds.penalty_weight / (2.0 * p)));
                    }
                }
                if let Some(hi) = bounds.upper_bar.get(idx).and_then(|o| *o) {
                    if (p - hi).max(0.0) > 0.0 {
                        jacobian_triplets.push((pos, pos, -bounds.penalty_weight / (2.0 * p)));
                    }
                }
            }
        }

        let gmres_max_iters_default = if m > 1200 { 220 } else { GMRES_MAX_ITERS };
        let gmres_max_iters =
            env_usize_opt("GAZFLOW_GMRES_MAX_ITERS").unwrap_or(gmres_max_iters_default);
        let gmres_restart = env_usize_opt("GAZFLOW_GMRES_RESTART").unwrap_or(GMRES_RESTART);
        let Some(delta_free) = solve_sparse_linear(m, &jacobian_triplets, &rhs)
            .or_else(|| {
                solve_sparse_gmres_ilu0(
                    m,
                    &jacobian_triplets,
                    &rhs,
                    GMRES_TOL,
                    gmres_max_iters,
                    gmres_restart,
                )
            })
            .or_else(|| {
                if m <= DENSE_FALLBACK_MAX_SIZE {
                    solve_dense_from_triplets(m, &jacobian_triplets, rhs.clone())
                } else {
                    None
                }
            })
        else {
            if !disable_jacobi {
                if guard_jacobi_fallback {
                    try_apply_jacobi_fallback_if_improves(
                        &mut pressures_sq,
                        &free_indices,
                        &state.f_node,
                        &state.j_diag,
                        residual,
                        NewtonEvalContext {
                            pipes: &pipes,
                            demands_vec: &demands_vec,
                            free_indices: &free_indices,
                            scaling,
                            gas_composition,
                            map_ctx: newton_map_ctx.as_ref(),
                            pressure_bounds: pressure_bounds_ref,
                        },
                    );
                } else {
                    apply_jacobi_fallback(
                        &mut pressures_sq,
                        &free_indices,
                        &state.f_node,
                        &state.j_diag,
                        pressure_bounds_ref,
                    );
                }
            }
            continue;
        };

        let mut accepted = false;
        let mut alpha = 1.0;
        for _ in 0..=MAX_BACKTRACK_STEPS {
            let mut trial_pressures = pressures_sq.clone();
            for (pos, &idx) in free_indices.iter().enumerate() {
                trial_pressures[idx] = step_free_pressure_sq(
                    trial_pressures[idx],
                    alpha * delta_free[pos],
                    idx,
                    pressure_bounds_ref,
                );
            }

            let trial_state = evaluate_state(
                &pipes,
                &demands_vec,
                &trial_pressures,
                &free_indices,
                scaling,
                gas_composition,
                newton_map_ctx.as_ref(),
                pressure_bounds_ref,
            );
            if trial_state.residual < residual {
                pressures_sq = trial_pressures;
                shortpipe_alias.sync_pressures(&mut pressures_sq);
                accepted = true;
                break;
            }
            alpha *= 0.5;
        }

        if !accepted && !disable_jacobi {
            if guard_jacobi_fallback {
                try_apply_jacobi_fallback_if_improves(
                    &mut pressures_sq,
                    &free_indices,
                    &state.f_node,
                    &state.j_diag,
                    residual,
                    NewtonEvalContext {
                        pipes: &pipes,
                        demands_vec: &demands_vec,
                        free_indices: &free_indices,
                        scaling,
                        gas_composition,
                        map_ctx: newton_map_ctx.as_ref(),
                        pressure_bounds: pressure_bounds_ref,
                    },
                );
            } else {
                apply_jacobi_fallback(
                    &mut pressures_sq,
                    &free_indices,
                    &state.f_node,
                    &state.j_diag,
                    pressure_bounds_ref,
                );
            }
            shortpipe_alias.sync_pressures(&mut pressures_sq);
        }
    }

    shortpipe_alias.sync_pressures(&mut pressures_sq);
    let final_state = evaluate_state(
        &pipes,
        &demands_vec,
        &pressures_sq,
        &free_indices,
        scaling,
        gas_composition,
        newton_map_ctx.as_ref(),
        pressure_bounds_ref,
    );

    if final_state.residual >= tolerance && !free_indices.is_empty() {
        if config.accept_partial_solution && final_state.residual.is_finite() {
            if scenario_boundary_active_envelopes_enabled()
                && !scenario_boundary_partial_accept_enabled()
            {
                if let Some(bounds) = pressure_bounds_ref {
                    let env_viol =
                        bounds.max_free_violation_m3s(&free_indices, &pressures_sq);
                    if env_viol > tolerance {
                        let contract_residual = final_state.residual.max(env_viol);
                        let mut result_pressures = HashMap::new();
                        for (i, id) in node_ids.iter().enumerate() {
                            result_pressures.insert(id.clone(), pressures_sq[i].sqrt());
                        }
                        let mut result_flows = HashMap::new();
                        for (pipe_idx, pipe) in pipes.iter().enumerate() {
                            result_flows.insert(pipe.id.clone(), final_state.flows[pipe_idx]);
                        }
                        return Ok(SolverResult::from_core(
                            result_pressures,
                            result_flows,
                            iterations,
                            contract_residual,
                        ));
                    }
                }
            }
            let mut result_pressures = HashMap::new();
            for (i, id) in node_ids.iter().enumerate() {
                result_pressures.insert(id.clone(), pressures_sq[i].sqrt());
            }
            let mut result_flows = HashMap::new();
            for (pipe_idx, pipe) in pipes.iter().enumerate() {
                result_flows.insert(pipe.id.clone(), final_state.flows[pipe_idx]);
            }
            return Ok(SolverResult::from_core(
                result_pressures,
                result_flows,
                iterations,
                final_state.residual,
            ));
        }
        bail!(
            "Newton-hybrid solver did not converge in {} iterations (residual={:.3e}, tolerance={:.3e})",
            iterations,
            final_state.residual,
            tolerance
        );
    }

    let mut result_pressures = HashMap::new();
    for (i, id) in node_ids.iter().enumerate() {
        result_pressures.insert(id.clone(), pressures_sq[i].sqrt());
    }

    let mut result_flows = HashMap::new();
    for (pipe_idx, pipe) in pipes.iter().enumerate() {
        result_flows.insert(pipe.id.clone(), final_state.flows[pipe_idx]);
    }

    Ok(SolverResult::from_core(
        result_pressures,
        result_flows,
        iterations,
        final_state.residual,
    ))
}

fn build_pressure_map(node_ids: &[String], pressures_sq: &[f64]) -> HashMap<String, f64> {
    node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), pressures_sq[i].sqrt()))
        .collect()
}

fn build_flow_map(pipes: &[IndexedPipe], flows: &[f64]) -> HashMap<String, f64> {
    pipes
        .iter()
        .enumerate()
        .map(|(i, pipe)| (pipe.id.clone(), flows[i]))
        .collect()
}

fn collect_active_regulator_nodes(
    network: &GasNetwork,
    id_pos: &HashMap<String, usize>,
    fixed: &HashMap<usize, f64>,
) -> Vec<usize> {
    let mut active = Vec::new();
    for pipe in network.pipes() {
        if !pipe.hydraulically_active() {
            continue;
        }
        if !matches!(
            pipe.kind,
            ConnectionKind::PressureRegulator
                | ConnectionKind::DeliveryStation
                | ConnectionKind::ControlValve
        ) {
            continue;
        }
        if let Some(&to_idx) = id_pos.get(&pipe.to)
            && fixed.contains_key(&to_idx)
        {
            active.push(to_idx);
        }
    }
    active.sort_unstable();
    active.dedup();
    active
}

#[derive(Debug, Clone, Copy)]
struct RegulatorIncident {
    free_node_idx: usize,
    j_free_to_regulated: f64,
}

fn append_active_regulator_row_coupling(
    jacobian_triplets: &mut Vec<(usize, usize, f64)>,
    active_regulator_nodes: &[usize],
    pipes: &[IndexedPipe],
    free_pos: &[usize],
    state: &IterationState,
    demands_vec: &[f64],
    pressures_sq: &[f64],
    free_indices: &[usize],
    rhs: &mut [f64],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: Option<&NewtonMapContext<'_>>,
) {
    if active_regulator_nodes.is_empty() {
        return;
    }

    for &reg_node_idx in active_regulator_nodes {
        let mut incidents = Vec::<RegulatorIncident>::new();
        let mut neighbor_vars = Vec::<usize>::new();

        for (pipe_idx, pipe) in pipes.iter().enumerate() {
            if pipe.from_idx == reg_node_idx {
                let free_node_idx = pipe.to_idx;
                if free_pos[free_node_idx] == usize::MAX {
                    continue;
                }
                incidents.push(RegulatorIncident {
                    free_node_idx,
                    j_free_to_regulated: state.conductances_from[pipe_idx],
                });
                neighbor_vars.push(free_node_idx);
            } else if pipe.to_idx == reg_node_idx {
                let free_node_idx = pipe.from_idx;
                if free_pos[free_node_idx] == usize::MAX {
                    continue;
                }
                incidents.push(RegulatorIncident {
                    free_node_idx,
                    j_free_to_regulated: state.conductances_to[pipe_idx],
                });
                neighbor_vars.push(free_node_idx);
            }
        }

        if incidents.is_empty() {
            continue;
        }
        neighbor_vars.sort_unstable();
        neighbor_vars.dedup();

        // MVP P8.7:
        // On évalue numériquement la ligne de résidu du nœud régulé actif (différences finies)
        // puis on condense sa contribution dans le bloc libre. Cela évite d'imposer une
        // dérivation analytique complète pour les cas régulateurs actifs.
        let mut variable_indices = Vec::with_capacity(neighbor_vars.len() + 1);
        variable_indices.push(reg_node_idx);
        variable_indices.extend(neighbor_vars.iter().copied());
        let row_derivs = finite_difference_node_row_derivatives(
            reg_node_idx,
            &variable_indices,
            state.f_node[reg_node_idx],
            pipes,
            demands_vec,
            pressures_sq,
            free_indices,
            scaling,
            gas_composition,
            map_ctx,
        );
        let Some(&j_rr) = row_derivs.get(&reg_node_idx) else {
            continue;
        };
        if !j_rr.is_finite() || j_rr.abs() < 1e-14 {
            continue;
        }

        for incident_i in &incidents {
            let row = free_pos[incident_i.free_node_idx];
            if row == usize::MAX || !incident_i.j_free_to_regulated.is_finite() {
                continue;
            }
            rhs[row] += (incident_i.j_free_to_regulated * state.f_node[reg_node_idx]) / j_rr;
            for &neighbor_idx in &neighbor_vars {
                let col = free_pos[neighbor_idx];
                if col == usize::MAX {
                    continue;
                }
                let Some(&j_rj) = row_derivs.get(&neighbor_idx) else {
                    continue;
                };
                let correction = -(incident_i.j_free_to_regulated * j_rj) / j_rr;
                if correction.is_finite() {
                    jacobian_triplets.push((row, col, correction));
                }
            }
        }
    }
}

fn finite_difference_node_row_derivatives(
    row_node_idx: usize,
    variable_indices: &[usize],
    base_row_residual: f64,
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    pressures_sq: &[f64],
    free_indices: &[usize],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: Option<&NewtonMapContext<'_>>,
) -> HashMap<usize, f64> {
    let mut derivs = HashMap::new();
    for &var_idx in variable_indices {
        let h = (pressures_sq[var_idx].abs().max(1.0) * REGULATOR_ROW_FD_REL_STEP).max(1e-6);
        let mut perturbed = pressures_sq.to_vec();
        perturbed[var_idx] = (perturbed[var_idx] + h).max(MIN_PRESSURE_SQ);
        let perturbed_state = evaluate_state(
            pipes,
            demands_vec,
            &perturbed,
            free_indices,
            scaling,
            gas_composition,
            map_ctx,
            None,
        );
        let d = (perturbed_state.f_node[row_node_idx] - base_row_residual) / h;
        if d.is_finite() {
            derivs.insert(var_idx, d);
        }
    }
    derivs
}

struct PipeFlowDerivatives {
    q: f64,
    conductance_from: f64,
    conductance_to: f64,
}

fn pipe_flow_derivatives(
    pipe: &IndexedPipe,
    pressures_sq: &[f64],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: Option<&NewtonMapContext<'_>>,
) -> PipeFlowDerivatives {
    if pipe.kind == ConnectionKind::CompressorStation
        && map_ctx.is_some_and(|ctx| {
            ctx.enthalpic || ctx.energy_closure || ctx.energy_equation || ctx.head_jac
        })
    {
        return pipe_flow_derivatives_enthalpic(
            pipe,
            pressures_sq,
            scaling,
            gas_composition,
            map_ctx.unwrap(),
        );
    }

    // Pressions effectives aux endpoints : pour le couplage dur compresseur
    // (Phase IV), un endpoint aliasé comme outlet compresseur a sa pression²
    // multipliée par r² (P_out² = r²·P_in²). Les conductances retournées par
    // `pipe_flow_with_gravity` sont exprimées vs ces pressions effectives ; on
    // les remultiplie par le facteur pour revenir aux pressions nodales.
    let p_from_sq = pipe.from_pressure_factor * pressures_sq[pipe.from_idx];
    let p_to_sq = pipe.to_pressure_factor * pressures_sq[pipe.to_idx];
    let p_from = p_from_sq.sqrt();
    let p_to = p_to_sq.sqrt();
    let avg_p = 0.5 * (p_from + p_to);
    let t_local = inlet_temperature_k_for_pipe(pipe, map_ctx, pressures_sq);
    let mut pressure_from_coeff = if pipe.pressure_from_coeff > 1.0 + 1e-6 {
        effective_compressor_pressure_from_coeff(
            pipe.pressure_from_coeff,
            p_from_sq,
            p_to_sq,
        )
    } else {
        pipe.pressure_from_coeff
    };
    let resistance = pipe_resistance_at_pressure_with_composition(
        pipe.length_km,
        pipe.diameter_mm,
        pipe.roughness_mm,
        avg_p,
        t_local,
        gas_composition,
        0.0,
    )
    .max(MIN_ABS_DP);
    let rho = gas_density_kg_per_m3_with_composition(avg_p, t_local, &gas_composition);
    let elev = PipeElevationContext {
        height_from_m: pipe.height_from_m,
        height_to_m: pipe.height_to_m,
        density_kg_per_m3: rho,
    };
    let (q_boot, _, _) = pipe_flow_with_gravity(
        p_from_sq,
        p_to_sq,
        pressure_from_coeff,
        resistance,
        scaling,
        elev,
    );
    if pipe.kind == ConnectionKind::CompressorStation && map_ctx.is_some() {
        pressure_from_coeff =
            compressor_coeff_from_map(pipe, pressures_sq, q_boot.abs(), map_ctx.unwrap());
    }
    let (q, conductance_from_eff, conductance_to_eff) = pipe_flow_with_gravity(
        p_from_sq,
        p_to_sq,
        pressure_from_coeff,
        resistance,
        scaling,
        elev,
    );
    PipeFlowDerivatives {
        q,
        conductance_from: conductance_from_eff * pipe.from_pressure_factor,
        conductance_to: conductance_to_eff * pipe.to_pressure_factor,
    }
}

/// Jacobian compresseur avec couplage carte(Q, P) sur le coefficient P² — v19/v20/v21.
fn pipe_flow_derivatives_enthalpic(
    pipe: &IndexedPipe,
    pressures_sq: &[f64],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: &NewtonMapContext<'_>,
) -> PipeFlowDerivatives {
    let p_from_sq = pressures_sq[pipe.from_idx];
    let p_to_sq = pressures_sq[pipe.to_idx];
    let p_from = p_from_sq.sqrt();
    let p_to = p_to_sq.sqrt();
    let avg_p = 0.5 * (p_from + p_to);
    let t_local = inlet_temperature_k_for_pipe(pipe, Some(map_ctx), pressures_sq);
    let mut pressure_from_coeff = if pipe.pressure_from_coeff > 1.0 + 1e-6 {
        if map_ctx.enthalpic || map_ctx.energy_closure || map_ctx.energy_equation {
            effective_compressor_pressure_from_coeff_enthalpic(
                pipe.pressure_from_coeff,
                p_from_sq,
                p_to_sq,
            )
        } else {
            effective_compressor_pressure_from_coeff(
                pipe.pressure_from_coeff,
                p_from_sq,
                p_to_sq,
            )
        }
    } else {
        pipe.pressure_from_coeff
    };
    let resistance = pipe_resistance_at_pressure_with_composition(
        pipe.length_km,
        pipe.diameter_mm,
        pipe.roughness_mm,
        avg_p,
        t_local,
        gas_composition,
        0.0,
    )
    .max(MIN_ABS_DP);
    let rho = gas_density_kg_per_m3_with_composition(avg_p, t_local, &gas_composition);
    let elev = PipeElevationContext {
        height_from_m: pipe.height_from_m,
        height_to_m: pipe.height_to_m,
        density_kg_per_m3: rho,
    };
    let (q_boot, _, _) = pipe_flow_with_gravity(
        p_from_sq,
        p_to_sq,
        pressure_from_coeff,
        resistance,
        scaling,
        elev,
    );
    pressure_from_coeff =
        compressor_coeff_from_map(pipe, pressures_sq, q_boot.abs(), map_ctx);
    let head_penalty = compressor_energy_penalty_psq(
        pipe,
        map_ctx,
        p_from,
        p_to,
        q_boot.abs(),
        avg_p,
    );
    let grav = gravity_dp_sq_bar(
        elev.height_from_m,
        elev.height_to_m,
        p_from,
        p_to,
        elev.density_kg_per_m3,
    );
    let dp_sq = pressure_from_coeff * p_from_sq - p_to_sq - grav - head_penalty;
    let (q, g) = flow_and_conductance(dp_sq, resistance, scaling);
    let (dgrav_from, dgrav_to) = gravity_dp_sq_derivatives_wrt_pressure_sq(
        elev.height_from_m,
        elev.height_to_m,
        p_from,
        p_to,
        elev.density_kg_per_m3,
    );
    let (_, dc_dq, dc_dp_in, dc_dp_out) =
        compressor_map_coeff_sensitivities(pipe, pressures_sq, q.abs(), map_ctx);
    let mut d_penalty_d_pin = 0.0;
    let mut d_penalty_d_pout = 0.0;
    let mut d_penalty_d_q = 0.0;
    if map_ctx.energy_equation {
        let hq = (q.abs() * 1e-4).max(1e-5);
        let hp_in = (p_from * 1e-4).max(1e-5);
        let hp_out = (p_to * 1e-4).max(1e-5);
        let base = head_penalty;
        d_penalty_d_q = (compressor_energy_penalty_psq(
            pipe,
            map_ctx,
            p_from,
            p_to,
            q.abs() + hq,
            avg_p,
        ) - base)
            / hq;
        d_penalty_d_pin = (compressor_energy_penalty_psq(
            pipe,
            map_ctx,
            p_from + hp_in,
            p_to,
            q.abs(),
            avg_p,
        ) - base)
            / hp_in;
        d_penalty_d_pout = (compressor_energy_penalty_psq(
            pipe,
            map_ctx,
            p_from,
            p_to + hp_out,
            q.abs(),
            avg_p,
        ) - base)
            / hp_out;
    }
    let dc_d_pi_from = dc_dp_in / (2.0 * p_from.max(1e-3));
    let dc_d_pi_to = dc_dp_out / (2.0 * p_to.max(1e-3));
    let d_penalty_d_pi_from = d_penalty_d_pin / (2.0 * p_from.max(1e-3));
    let d_penalty_d_pi_to = d_penalty_d_pout / (2.0 * p_to.max(1e-3));
    let a = pressure_from_coeff + p_from_sq * dc_d_pi_from - dgrav_from - d_penalty_d_pi_from;
    let b = p_from_sq * (dc_dq + d_penalty_d_q) * g;
    let denom = (1.0 - b).max(0.05);
    let d_dp_d_from = if denom.is_finite() { a / denom } else { a };
    let d_dp_d_to = p_from_sq * dc_d_pi_to - 1.0 - dgrav_to - d_penalty_d_pi_to;
    PipeFlowDerivatives {
        q,
        conductance_from: g * d_dp_d_from,
        conductance_to: g * (-d_dp_d_to),
    }
}

fn evaluate_state(
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    pressures_sq: &[f64],
    free_indices: &[usize],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: Option<&NewtonMapContext<'_>>,
    pressure_bounds: Option<&PressureBoundContext>,
) -> IterationState {
    let n = pressures_sq.len();
    let mut f_node = demands_vec.to_vec();
    let mut j_diag = vec![0.0_f64; n];
    let mut flows = vec![0.0_f64; pipes.len()];
    let mut conductances_from = vec![0.0_f64; pipes.len()];
    let mut conductances_to = vec![0.0_f64; pipes.len()];

    if pipes.len() >= PARALLEL_PIPE_THRESHOLD {
        let (pipe_contrib_f, pipe_contrib_j, qg_entries) = pipes
            .par_iter()
            .enumerate()
            .fold(
                || {
                    (
                        vec![0.0_f64; n],
                        vec![0.0_f64; n],
                        Vec::<(usize, f64, f64, f64)>::new(),
                    )
                },
                |(mut local_f, mut local_j, mut local_qg), (pipe_idx, pipe)| {
                    let deriv =
                        pipe_flow_derivatives(pipe, pressures_sq, scaling, gas_composition, map_ctx);

                    local_f[pipe.from_idx] -= deriv.q;
                    local_f[pipe.to_idx] += deriv.q;
                    local_j[pipe.from_idx] += deriv.conductance_from;
                    local_j[pipe.to_idx] += deriv.conductance_to;
                    local_qg.push((
                        pipe_idx,
                        deriv.q,
                        deriv.conductance_from,
                        deriv.conductance_to,
                    ));
                    (local_f, local_j, local_qg)
                },
            )
            .reduce(
                || {
                    (
                        vec![0.0_f64; n],
                        vec![0.0_f64; n],
                        Vec::<(usize, f64, f64, f64)>::new(),
                    )
                },
                |(mut f_a, mut j_a, mut qg_a), (f_b, j_b, mut qg_b)| {
                    for i in 0..n {
                        f_a[i] += f_b[i];
                        j_a[i] += j_b[i];
                    }
                    qg_a.append(&mut qg_b);
                    (f_a, j_a, qg_a)
                },
            );

        for i in 0..n {
            f_node[i] += pipe_contrib_f[i];
            j_diag[i] += pipe_contrib_j[i];
        }
        for (pipe_idx, q, g_from, g_to) in qg_entries {
            flows[pipe_idx] = q;
            conductances_from[pipe_idx] = g_from;
            conductances_to[pipe_idx] = g_to;
        }
    } else {
        for (pipe_idx, pipe) in pipes.iter().enumerate() {
            let deriv =
                pipe_flow_derivatives(pipe, pressures_sq, scaling, gas_composition, map_ctx);

            f_node[pipe.from_idx] -= deriv.q;
            f_node[pipe.to_idx] += deriv.q;
            j_diag[pipe.from_idx] += deriv.conductance_from;
            j_diag[pipe.to_idx] += deriv.conductance_to;

            flows[pipe_idx] = deriv.q;
            conductances_from[pipe_idx] = deriv.conductance_from;
            conductances_to[pipe_idx] = deriv.conductance_to;
        }
    }

    if let Some(bounds) = pressure_bounds {
        for &idx in free_indices {
            let p = pressures_sq[idx].sqrt().max(1e-3);
            if let Some(lo) = bounds.lower_bar.get(idx).and_then(|o| *o) {
                let shortfall = (lo - p).max(0.0);
                if shortfall > 0.0 {
                    f_node[idx] += bounds.penalty_weight * shortfall;
                    j_diag[idx] += bounds.penalty_weight / (2.0 * p);
                }
            }
            if let Some(hi) = bounds.upper_bar.get(idx).and_then(|o| *o) {
                let excess = (p - hi).max(0.0);
                if excess > 0.0 {
                    f_node[idx] -= bounds.penalty_weight * excess;
                    j_diag[idx] += bounds.penalty_weight / (2.0 * p);
                }
            }
        }
    }

    let mut residual = free_indices
        .iter()
        .map(|&idx| f_node[idx].abs())
        .fold(0.0, f64::max);

    if let Some(bounds) = pressure_bounds {
        for &idx in free_indices {
            let viol = bounds.violation_m3s(idx, pressures_sq[idx].sqrt());
            residual = residual.max(viol);
        }
    }

    IterationState {
        f_node,
        j_diag,
        flows,
        conductances_from,
        conductances_to,
        residual,
    }
}

fn build_physical_initial_guess(
    node_count: usize,
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    fixed: &HashMap<usize, f64>,
    pressure_sq_ref: f64,
    gas_composition: GasComposition,
) -> Option<Vec<f64>> {
    let init_iters = physical_init_iters(node_count);
    if init_iters == 0 || pipes.is_empty() {
        return None;
    }

    let ref_pressure_bar = pressure_sq_ref.sqrt().max(1.0);
    let linear_conductances: Vec<f64> = pipes
        .iter()
        .map(|pipe| {
            let resistance = pipe_resistance_at_pressure_with_composition(
                pipe.length_km,
                pipe.diameter_mm,
                pipe.roughness_mm,
                ref_pressure_bar,
                DEFAULT_GAS_TEMPERATURE_K,
                gas_composition,
                0.0,
            )
            .max(MIN_ABS_DP);
            (1.0 / resistance).min(1e16)
        })
        .collect();

    let mut pressures_sq = vec![pressure_sq_ref.max(MIN_PRESSURE_SQ); node_count];
    for (&idx, &fixed_sq) in fixed {
        pressures_sq[idx] = fixed_sq.max(MIN_PRESSURE_SQ);
    }

    let mut f_node = vec![0.0_f64; node_count];
    let mut j_diag = vec![0.0_f64; node_count];
    for _ in 0..init_iters {
        f_node.copy_from_slice(demands_vec);
        j_diag.fill(0.0);

        for (pipe_idx, pipe) in pipes.iter().enumerate() {
            let c = linear_conductances[pipe_idx];
            let p_from_sq = pipe.from_pressure_factor * pressures_sq[pipe.from_idx];
            let p_to_sq = pipe.to_pressure_factor * pressures_sq[pipe.to_idx];
            let p_from = p_from_sq.sqrt();
            let p_to = p_to_sq.sqrt();
            let rho = gas_density_kg_per_m3_with_composition(
                0.5 * (p_from + p_to),
                DEFAULT_GAS_TEMPERATURE_K,
                &gas_composition,
            );
            let grav = super::steady_state::gravity_dp_sq_bar(
                pipe.height_from_m,
                pipe.height_to_m,
                p_from,
                p_to,
                rho,
            );
            let q_lin = c
                * (pipe.pressure_from_coeff * p_from_sq
                    - p_to_sq
                    - grav);

            f_node[pipe.from_idx] -= q_lin;
            f_node[pipe.to_idx] += q_lin;
            j_diag[pipe.from_idx] += c * pipe.pressure_from_coeff * pipe.from_pressure_factor;
            j_diag[pipe.to_idx] += c * pipe.to_pressure_factor;
        }

        for i in 0..node_count {
            if fixed.contains_key(&i) || j_diag[i] <= 1e-20 {
                continue;
            }
            let delta = PHYSICAL_INIT_RELAX * f_node[i] / j_diag[i];
            pressures_sq[i] = (pressures_sq[i] + delta).max(MIN_PRESSURE_SQ);
        }
    }

    Some(pressures_sq)
}

#[cfg(test)]
mod tests {
    use crate::graph::EquipmentSpec;
    use crate::solver::gas_properties::GasComposition;
    use crate::solver::steady_state::{
        NondimScaling, compressor_pressure_from_coeff, effective_pipe_geometry,
        flow_reference_from_demands, pressure_sq_reference_from_fixed,
    };
    use serial_test::serial;
    use std::collections::HashMap;

    use rayon::ThreadPoolBuilder;

    use crate::{
        graph::{ConnectionKind, GasNetwork, Node, Pipe},
        solver::solve_steady_state,
    };

    fn long_chain_network(pipe_count: usize) -> GasNetwork {
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
                roughness_mm: 0.05,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            });
        }
        net
    }

    #[test]
    fn test_parallel_solver_same_result() {
        let network = long_chain_network(60);
        let mut demands = HashMap::new();
        demands.insert("N60".to_string(), -3.0);

        let pool_one = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("pool(1)");
        let result_one = pool_one
            .install(|| solve_steady_state(&network, &demands, 2000, 5e-4).expect("solve 1t"));

        let pool_many = ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .expect("pool(4)");
        let result_many = pool_many
            .install(|| solve_steady_state(&network, &demands, 2000, 5e-4).expect("solve 4t"));

        for (node_id, p1) in &result_one.pressures {
            let p4 = result_many
                .pressures
                .get(node_id)
                .expect("node should exist in both solves");
            assert!(
                (p1 - p4).abs() < 1e-3,
                "pressure mismatch for {node_id}: 1t={p1}, 4t={p4}"
            );
        }
        for (pipe_id, q1) in &result_one.flows {
            let q4 = result_many
                .flows
                .get(pipe_id)
                .expect("pipe should exist in both solves");
            assert!(
                (q1 - q4).abs() < 1e-6,
                "flow mismatch for {pipe_id}: 1t={q1}, 4t={q4}"
            );
        }
    }

    #[test]
    #[serial]
    fn test_sparse_linear_solver_matches_dense() {
        let m = 3;
        let triplets = vec![
            (0, 0, 4.0),
            (0, 1, -1.0),
            (1, 0, -1.0),
            (1, 1, 4.0),
            (1, 2, -1.0),
            (2, 1, -1.0),
            (2, 2, 3.0),
        ];
        let rhs = vec![15.0, 10.0, 10.0];

        let Some(sparse) = super::solve_sparse_linear(m, &triplets, &rhs) else {
            eprintln!("skip: sparse LU backend unavailable in this run");
            return;
        };
        let dense = super::solve_dense_from_triplets(m, &triplets, rhs).expect("dense solve");

        for (a, b) in sparse.iter().zip(dense.iter()) {
            assert!(
                (a - b).abs() < 1e-10,
                "delta mismatch: sparse={a}, dense={b}"
            );
        }
    }

    fn regulator_fd_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        for (id, p_fix) in [("HP", Some(70.0)), ("MP", None), ("SK", Some(20.0))] {
            net.add_node(Node {
                id: id.into(),
                x: 0.0,
                y: 0.0,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: p_fix,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        net.add_pipe(Pipe {
            id: "P_HP".into(),
            from: "HP".into(),
            to: "MP".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 20.0,
            diameter_mm: 700.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net.add_pipe(Pipe {
            id: "REG".into(),
            from: "MP".into(),
            to: "SK".into(),
            kind: ConnectionKind::PressureRegulator,
            is_open: true,
            length_km: 0.01,
            diameter_mm: 800.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::pressure_regulator(20.0, 0.5),
        });
        net
    }

    #[test]
    fn test_regulator_jacobian_finite_difference_consistent() {
        let network = regulator_fd_network();
        let node_ids: Vec<String> = network.nodes().map(|n| n.id.clone()).collect();
        let id_pos: HashMap<String, usize> = node_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        let fixed: HashMap<usize, f64> = network
            .nodes()
            .filter_map(|n| {
                n.pressure_fixed_bar
                    .map(|p| (*id_pos.get(&n.id).expect("node index"), p * p))
            })
            .collect();
        let active_nodes = super::collect_active_regulator_nodes(&network, &id_pos, &fixed);
        let sk_idx = *id_pos.get("SK").expect("SK index");
        assert_eq!(active_nodes, vec![sk_idx]);

        let free_indices: Vec<usize> = (0..network.node_count())
            .filter(|i| !fixed.contains_key(i))
            .collect();
        let mut pressures_sq = vec![70.0_f64.powi(2); network.node_count()];
        pressures_sq[*id_pos.get("MP").expect("MP index")] = 45.0_f64.powi(2);
        for (&idx, &p_sq) in &fixed {
            pressures_sq[idx] = p_sq;
        }

        let demands_vec = vec![0.0_f64; network.node_count()];
        let scaling = NondimScaling::new(
            pressure_sq_reference_from_fixed(&fixed),
            flow_reference_from_demands(&demands_vec),
        );
        let node_heights: HashMap<String, f64> = network
            .nodes()
            .map(|node| (node.id.clone(), node.height_m))
            .collect();
        let pipes: Vec<super::IndexedPipe> = network
            .pipes()
            .filter_map(|pipe| {
                if !pipe.hydraulically_active() {
                    return None;
                }
                let from_idx = id_pos.get(&pipe.from).copied()?;
                let to_idx = id_pos.get(&pipe.to).copied()?;
                let (length_km, diameter_mm, roughness_mm) = effective_pipe_geometry(pipe);
                Some(super::IndexedPipe {
                    id: pipe.id.clone(),
                    from_idx,
                    to_idx,
                    kind: pipe.kind,
                    length_km,
                    diameter_mm,
                    roughness_mm,
                    pressure_from_coeff: crate::solver::compressor_pressure_from_coeff(pipe),
                    operating_ratio: pipe
                        .compressor_ratio_max
                        .or(pipe.equipment.compressor_nominal_ratio),
                    pressure_cap_ratio: pipe.equipment.compressor_pressure_cap_ratio,
                    height_from_m: node_heights.get(&pipe.from).copied().unwrap_or(0.0),
                    height_to_m: node_heights.get(&pipe.to).copied().unwrap_or(0.0),
                    from_pressure_factor: 1.0,
                    to_pressure_factor: 1.0,
                })
            })
            .collect();
        let state = super::evaluate_state(
            &pipes,
            &demands_vec,
            &pressures_sq,
            &free_indices,
            scaling,
            GasComposition::pure_ch4(),
            None,
            None,
        );
        let mp_idx = *id_pos.get("MP").expect("MP index");
        let fd = super::finite_difference_node_row_derivatives(
            sk_idx,
            &[sk_idx, mp_idx],
            state.f_node[sk_idx],
            &pipes,
            &demands_vec,
            &pressures_sq,
            &free_indices,
            scaling,
            GasComposition::pure_ch4(),
            None,
        );
        let reg_pipe_idx = pipes
            .iter()
            .position(|p| p.id == "REG")
            .expect("REG pipe index");
        let expected_d_fsk_d_pmp = state.conductances_from[reg_pipe_idx];
        let expected_d_fsk_d_psk = -state.conductances_to[reg_pipe_idx];
        let fd_d_fsk_d_pmp = *fd.get(&mp_idx).expect("fd derivative wrt MP");
        let fd_d_fsk_d_psk = *fd.get(&sk_idx).expect("fd derivative wrt SK");
        let tol_mp = expected_d_fsk_d_pmp.abs() * 5e-3 + 1e-10;
        let tol_sk = expected_d_fsk_d_psk.abs() * 5e-3 + 1e-10;
        assert!(
            (fd_d_fsk_d_pmp - expected_d_fsk_d_pmp).abs() <= tol_mp,
            "dF_SK/dπ_MP mismatch: fd={fd_d_fsk_d_pmp}, analytic={expected_d_fsk_d_pmp}"
        );
        assert!(
            (fd_d_fsk_d_psk - expected_d_fsk_d_psk).abs() <= tol_sk,
            "dF_SK/dπ_SK mismatch: fd={fd_d_fsk_d_psk}, analytic={expected_d_fsk_d_psk}"
        );
    }

    /// Réseau trivial : source (50 bar fixés) → compresseur (r=1,5) → tuyau → sink.
    /// En couplage dur, P_outlet doit valoir exactement r·P_inlet = 75 bar.
    fn hard_coupling_trivial_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        for (id, fixed) in [("S", Some(50.0)), ("M", None), ("T", None)] {
            net.add_node(Node {
                id: id.to_string(),
                x: 0.0,
                y: 0.0,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: fixed,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        // Compresseur S → M, ratio 1.5, cap 1.5 (non-"transport" pour éviter
        // l'outer loop de blending ; le ratio reste fixé à 1.5).
        net.add_pipe(Pipe {
            id: "CS".into(),
            from: "S".into(),
            to: "M".into(),
            kind: ConnectionKind::CompressorStation,
            is_open: true,
            length_km: 0.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: Some(1.5),
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec {
                compressor_pressure_cap_ratio: Some(1.5),
                compressor_nominal_ratio: None,
                ..EquipmentSpec::default()
            },
        });
        // Tuyau M → T (conductance finie pour évacuer le débit du sink).
        net.add_pipe(Pipe {
            id: "PM".into(),
            from: "M".into(),
            to: "T".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    #[test]
    #[serial]
    fn test_hard_coupling_enforces_ratio_on_trivial_network() {
        unsafe {
            std::env::set_var("GAZFLOW_SCENARIO_SHORTPIPE_MERGE_BOUNDARIES", "1");
            std::env::set_var("GAZFLOW_COMPRESSOR_DECISION_VARIABLES", "1");
            std::env::set_var("GAZFLOW_COMPRESSOR_HARD_COUPLING", "1");
        }
        let network = hard_coupling_trivial_network();
        let mut demands = HashMap::new();
        demands.insert("T".to_string(), -5.0);
        let result = solve_steady_state(&network, &demands, 2000, 1e-4).expect("solve");
        unsafe {
            std::env::remove_var("GAZFLOW_COMPRESSOR_HARD_COUPLING");
            std::env::remove_var("GAZFLOW_COMPRESSOR_DECISION_VARIABLES");
            std::env::remove_var("GAZFLOW_SCENARIO_SHORTPIPE_MERGE_BOUNDARIES");
        }
        let p_m = result.pressures.get("M").copied().expect("P_M");
        let p_s = result.pressures.get("S").copied().expect("P_S");
        // Couplage dur : P_M = 1.5 · P_S = 75 bar (à la tolérance du solveur).
        let expected = 1.5 * p_s;
        assert!(
            (p_m - expected).abs() <= 0.5,
            "hard coupling violated: P_M={p_m:.3}, expected r·P_S={expected:.3} (P_S={p_s:.3})"
        );
        assert!(
            (p_m - 75.0).abs() <= 0.5,
            "P_M should be ~75 bar, got {p_m:.3}"
        );
    }
}

struct NewtonEvalContext<'a> {
    pipes: &'a [IndexedPipe],
    demands_vec: &'a [f64],
    free_indices: &'a [usize],
    scaling: NondimScaling,
    gas_composition: GasComposition,
    map_ctx: Option<&'a NewtonMapContext<'a>>,
    pressure_bounds: Option<&'a PressureBoundContext>,
}

impl NewtonEvalContext<'_> {
    fn evaluate(&self, pressures_sq: &[f64]) -> IterationState {
        evaluate_state(
            self.pipes,
            self.demands_vec,
            pressures_sq,
            self.free_indices,
            self.scaling,
            self.gas_composition,
            self.map_ctx,
            self.pressure_bounds,
        )
    }
}

fn try_apply_jacobi_fallback_if_improves(
    pressures_sq: &mut Vec<f64>,
    free_indices: &[usize],
    f_node: &[f64],
    j_diag: &[f64],
    current_residual: f64,
    eval: NewtonEvalContext<'_>,
) {
    let mut candidate = pressures_sq.clone();
    for &idx in free_indices {
        if j_diag[idx] > 1e-20 {
            let delta = JACOBI_RELAX * f_node[idx] / j_diag[idx];
            candidate[idx] = step_free_pressure_sq(
                candidate[idx],
                delta,
                idx,
                eval.pressure_bounds,
            );
        }
    }
    let candidate_state = eval.evaluate(&candidate);
    if candidate_state.residual < current_residual {
        *pressures_sq = candidate;
    }
}

fn apply_jacobi_fallback(
    pressures_sq: &mut [f64],
    free_indices: &[usize],
    f_node: &[f64],
    j_diag: &[f64],
    pressure_bounds: Option<&PressureBoundContext>,
) {
    for &idx in free_indices {
        if j_diag[idx] > 1e-20 {
            let delta = JACOBI_RELAX * f_node[idx] / j_diag[idx];
            pressures_sq[idx] =
                step_free_pressure_sq(pressures_sq[idx], delta, idx, pressure_bounds);
        }
    }
}

fn solve_dense_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    if n == 0 {
        return Some(Vec::new());
    }

    for col in 0..n {
        let mut pivot_row = col;
        let mut pivot_abs = a[col][col].abs();
        for (row, row_vals) in a.iter().enumerate().skip(col + 1).take(n - (col + 1)) {
            let value = row_vals[col].abs();
            if value > pivot_abs {
                pivot_abs = value;
                pivot_row = row;
            }
        }
        if pivot_abs < PIVOT_EPS {
            return None;
        }

        if pivot_row != col {
            a.swap(col, pivot_row);
            b.swap(col, pivot_row);
        }

        let pivot = a[col][col];
        for row in (col + 1)..n {
            let factor = a[row][col] / pivot;
            if factor == 0.0 {
                continue;
            }
            let pivot_vals = a[col].to_vec();
            for k in col..n {
                a[row][k] -= factor * pivot_vals[k];
            }
            b[row] -= factor * b[col];
        }
    }

    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut sum = b[i];
        for (j, &a_ij) in a[i].iter().enumerate().skip(i + 1).take(n - (i + 1)) {
            sum -= a_ij * x[j];
        }
        let diag = a[i][i];
        if diag.abs() < PIVOT_EPS {
            return None;
        }
        x[i] = sum / diag;
    }

    Some(x)
}

fn solve_sparse_linear(
    m: usize,
    triplets: &[(usize, usize, f64)],
    rhs: &[f64],
) -> Option<Vec<f64>> {
    if !SPARSE_LU_ENABLED.load(Ordering::Relaxed) || m > SPARSE_LU_MAX_SIZE {
        return None;
    }
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        if m == 0 {
            return Some(Vec::new());
        }
        let sparse_triplets: Vec<Triplet<usize, usize, f64>> = triplets
            .iter()
            .map(|&(row, col, val)| Triplet::new(row, col, val))
            .collect();
        let jacobian =
            SparseColMat::<usize, f64>::try_new_from_triplets(m, m, &sparse_triplets).ok()?;
        let lu = jacobian.sp_lu().ok()?;
        let rhs_mat = Mat::from_fn(m, 1, |i, _| rhs[i]);
        let solution = lu.solve(&rhs_mat);
        let x: Vec<f64> = (0..m).map(|i| solution[(i, 0)]).collect();
        if x.iter().all(|v| v.is_finite()) {
            Some(x)
        } else {
            None
        }
    }))
    .ok()
    .flatten()
}

fn solve_dense_from_triplets(
    m: usize,
    triplets: &[(usize, usize, f64)],
    b: Vec<f64>,
) -> Option<Vec<f64>> {
    let mut dense = vec![vec![0.0_f64; m]; m];
    for &(row, col, val) in triplets {
        dense[row][col] += val;
    }
    solve_dense_linear(dense, b)
}
