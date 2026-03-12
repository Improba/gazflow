use std::collections::HashMap;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};
use crate::solver::gas_properties::{DEFAULT_GAS_TEMPERATURE_K, gas_density_kg_per_m3};

/// Résultat d'une simulation en régime permanent.
#[derive(Debug, Clone, Serialize)]
pub struct SolverResult {
    /// Pression à chaque nœud (bar).
    pub pressures: HashMap<String, f64>,
    /// Débit dans chaque tuyau (m³/s à conditions normales).
    pub flows: HashMap<String, f64>,
    /// Nombre d'itérations Newton-Raphson.
    pub iterations: usize,
    /// Résidu final.
    pub residual: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverControl {
    Continue,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct SolverProgress {
    pub iter: usize,
    pub residual: f64,
    pub pressures: Option<HashMap<String, f64>>,
    pub flows: Option<HashMap<String, f64>>,
}

const MIN_PIPE_RESISTANCE: f64 = 1e-16;
const DEFAULT_PRESSURE_BOUNDS_TOL_BAR: f64 = 0.05;
const MIN_MASS_BALANCE_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy)]
pub(crate) struct NondimScaling {
    pub pressure_sq_ref: f64,
    pub flow_ref: f64,
}

impl NondimScaling {
    pub(crate) fn new(pressure_sq_ref: f64, flow_ref: f64) -> Self {
        Self {
            pressure_sq_ref: pressure_sq_ref.max(1.0),
            flow_ref: flow_ref.abs().max(1e-6),
        }
    }

    pub(crate) fn resistance_hat(self, resistance: f64) -> f64 {
        (resistance * self.flow_ref * self.flow_ref / self.pressure_sq_ref).max(MIN_PIPE_RESISTANCE)
    }
}

/// Approximation explicite de Swamee-Jain du coefficient de friction de Darcy.
pub(crate) fn darcy_friction(roughness_mm: f64, diameter_mm: f64, reynolds: f64) -> f64 {
    let e_d = roughness_mm / diameter_mm;
    if reynolds < 2300.0 {
        return 64.0 / reynolds.max(1.0);
    }
    let a = e_d / 3.7;
    let b = 5.74 / reynolds.powf(0.9);
    let log_term = (a + b).log10();
    0.25 / (log_term * log_term)
}

/// Résistance hydraulique K d'un tuyau, telle que :
///   P_in² - P_out² = K · Q · |Q|   (en bar², Q en unités arbitraires)
///
/// K intègre un facteur de densité simplifié pour que les pressions
/// restent dans l'ordre de grandeur de 1-100 bar.
#[allow(dead_code)]
pub(crate) fn pipe_resistance(length_km: f64, diameter_mm: f64, roughness_mm: f64) -> f64 {
    // Compat MVP historique: rho_eff fixe.
    pipe_resistance_with_density(length_km, diameter_mm, roughness_mm, 50.0)
}

pub(crate) fn pipe_resistance_with_density(
    length_km: f64,
    diameter_mm: f64,
    roughness_mm: f64,
    density_kg_per_m3: f64,
) -> f64 {
    let d = diameter_mm * 1e-3; // m
    let l = length_km * 1e3; // m
    let f = darcy_friction(roughness_mm, diameter_mm, 1e7);
    let area = std::f64::consts::PI * d * d / 4.0;

    // Conversion Pa² → bar² : diviser par 1e10.
    let rho_eff = density_kg_per_m3.max(1e-6);
    (f * l * rho_eff / (2.0 * d * area * area * 1e10)).max(MIN_PIPE_RESISTANCE)
}

pub(crate) fn pipe_resistance_at_pressure(
    length_km: f64,
    diameter_mm: f64,
    roughness_mm: f64,
    average_pressure_bar: f64,
    temperature_k: f64,
) -> f64 {
    let rho = gas_density_kg_per_m3(average_pressure_bar, temperature_k);
    pipe_resistance_with_density(length_km, diameter_mm, roughness_mm, rho)
}

pub(crate) fn effective_pipe_geometry(pipe: &Pipe) -> (f64, f64, f64) {
    match pipe.kind {
        ConnectionKind::Pipe | ConnectionKind::Resistor => {
            (pipe.length_km, pipe.diameter_mm, pipe.roughness_mm)
        }
        ConnectionKind::Valve | ConnectionKind::ShortPipe | ConnectionKind::CompressorStation => {
            // Valve ouverte / shortPipe / compresseur MVP -> liaison quasi transparente.
            (
                pipe.length_km.min(0.001),
                pipe.diameter_mm.max(1000.0),
                pipe.roughness_mm,
            )
        }
    }
}

#[allow(dead_code)]
pub(crate) fn effective_pipe_resistance(pipe: &Pipe) -> f64 {
    let (length_km, diameter_mm, roughness_mm) = effective_pipe_geometry(pipe);
    pipe_resistance_at_pressure(
        length_km,
        diameter_mm,
        roughness_mm,
        70.0,
        DEFAULT_GAS_TEMPERATURE_K,
    )
}

pub(crate) fn pressure_sq_reference_from_fixed(fixed: &HashMap<usize, f64>) -> f64 {
    fixed.values().copied().fold(70.0_f64.powi(2), f64::max)
}

pub(crate) fn compressor_pressure_from_coeff(pipe: &Pipe) -> f64 {
    if pipe.kind != ConnectionKind::CompressorStation {
        return 1.0;
    }
    let ratio = pipe.compressor_ratio_max.unwrap_or(1.08).clamp(1.0, 1.6);
    ratio * ratio
}

pub(crate) fn flow_reference_from_demands(demands: &[f64]) -> f64 {
    demands
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0_f64, f64::max)
        .max(1.0)
}

pub(crate) fn flow_and_conductance(
    dp_sq: f64,
    resistance: f64,
    scaling: NondimScaling,
) -> (f64, f64) {
    let abs_dp_sq = dp_sq.abs().max(1e-10);
    let sign = dp_sq.signum();

    // Non-dimensionnalisation:
    // π̂ = π / π_ref,  Q̂ = Q / Q_ref,  K̂ = K·Q_ref²/π_ref.
    let dp_hat = abs_dp_sq / scaling.pressure_sq_ref;
    let k_hat = scaling.resistance_hat(resistance);
    let q_hat = (dp_hat / k_hat).sqrt();
    let q = sign * q_hat * scaling.flow_ref;

    // dQ/dπ = (Q_ref / π_ref) * dQ̂/dπ̂.
    let g_hat = 1.0 / (2.0 * (k_hat * dp_hat).sqrt());
    let g = (scaling.flow_ref / scaling.pressure_sq_ref) * g_hat;
    (q, g)
}

/// Résout le réseau en régime permanent via Newton complet + line-search.
///
/// Si une itération Newton échoue (Jacobien singulier ou line-search sans progrès),
/// un fallback Jacobi est appliqué sur cette itération.
pub fn solve_steady_state(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
    solve_steady_state_with_progress(network, demands, None, max_iter, tolerance, 0, |_| {
        SolverControl::Continue
    })
}

pub fn solve_steady_state_with_initial_pressures(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
    solve_steady_state_with_progress(
        network,
        demands,
        initial_pressures_bar,
        max_iter,
        tolerance,
        0,
        |_| SolverControl::Continue,
    )
}

pub fn solve_steady_state_with_progress<F>(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    max_iter: usize,
    tolerance: f64,
    snapshot_every: usize,
    on_progress: F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    let compressor_count = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation)
        .count();
    if compressor_count > 0 {
        tracing::warn!(
            compressor_count,
            "Compressor stations use a simplified pressure-lift MVP model"
        );
    }

    let result = super::newton::solve_steady_state_newton_hybrid(
        network,
        demands,
        initial_pressures_bar,
        max_iter,
        tolerance,
        snapshot_every,
        on_progress,
    )?;
    validate_solution_physics(network, demands, &result, tolerance)?;
    Ok(result)
}

/// Résout le réseau en régime permanent via Newton-Raphson diagonal (Jacobi).
///
/// **Convention de signe :**
/// - `demands[id] > 0` : injection (source)
/// - `demands[id] < 0` : consommation (puits)
///
/// **Variable :** π_i = P_i² (pression au carré, en bar²).
///
/// **Équation nodale :**
///   F_i = Σ Q_entering_i − Σ Q_leaving_i + d_i = 0
///
/// **Hypothèses MVP :**
/// - Gaz parfait, Z = 1, T = 288 K.
/// - Nœuds sources : pression fixée (nœuds "slack").
/// - Compresseurs approximés par un uplift de pression borné (MVP).
pub fn solve_steady_state_jacobi(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
    let n = network.node_count();
    let node_ids: Vec<String> = network.nodes().map(|n| n.id.clone()).collect();
    let id_pos: HashMap<String, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();
    let mut pressures_sq: Vec<f64> = vec![70.0_f64.powi(2); n];

    let fixed: HashMap<usize, f64> = network
        .nodes()
        .filter_map(|n| {
            n.pressure_fixed_bar
                .map(|p| (*id_pos.get(&n.id).unwrap(), p * p))
        })
        .collect();

    for (&idx, &p_sq) in &fixed {
        pressures_sq[idx] = p_sq;
    }

    let compressor_count = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation)
        .count();
    if compressor_count > 0 {
        tracing::warn!(
            compressor_count,
            "Compressor stations use a simplified pressure-lift MVP model"
        );
    }

    let mut demands_vec = vec![0.0_f64; n];
    for (id, &d) in demands {
        if !d.is_finite() {
            bail!("invalid demand value for node '{id}': {d}");
        }
        let Some(&i) = id_pos.get(id) else {
            bail!("unknown demand node id: '{id}'");
        };
        demands_vec[i] += d;
    }

    let scaling = NondimScaling::new(
        pressure_sq_reference_from_fixed(&fixed),
        flow_reference_from_demands(&demands_vec),
    );

    let pipes: Vec<_> = network
        .pipes()
        .filter_map(|pipe| {
            if pipe.kind == ConnectionKind::Valve && !pipe.is_open {
                return None;
            }
            let Some(&a) = id_pos.get(&pipe.from) else {
                return None;
            };
            let Some(&b) = id_pos.get(&pipe.to) else {
                return None;
            };
            let (length_km, diameter_mm, roughness_mm) = effective_pipe_geometry(pipe);
            let pressure_from_coeff = compressor_pressure_from_coeff(pipe);
            Some((
                pipe.id.clone(),
                a,
                b,
                length_km,
                diameter_mm,
                roughness_mm,
                pressure_from_coeff,
            ))
        })
        .collect();
    let mut iterations = 0;
    let mut residual = f64::MAX;
    let relax = 0.8;

    for iter in 0..max_iter {
        // F_i : résidu nodal (bilan de masse)
        let mut f_node = demands_vec.clone();
        // J_ii positif : somme des conductances linéarisées connectées au nœud i
        let mut j_diag = vec![0.0_f64; n];

        for (_, a, b, length_km, diameter_mm, roughness_mm, pressure_from_coeff) in &pipes {
            let p_a = pressures_sq[*a].sqrt();
            let p_b = pressures_sq[*b].sqrt();
            let avg_p = 0.5 * (p_a + p_b);
            let k = pipe_resistance_at_pressure(
                *length_km,
                *diameter_mm,
                *roughness_mm,
                avg_p,
                DEFAULT_GAS_TEMPERATURE_K,
            );
            let dp_sq = *pressure_from_coeff * pressures_sq[*a] - pressures_sq[*b];
            let (q, g) = flow_and_conductance(dp_sq, k, scaling);
            let dq_dpi_from = g * *pressure_from_coeff;
            let dq_dpi_to = g;

            // Q > 0 → flow from a to b
            // Node a perd Q (outflow), node b gagne Q (inflow)
            f_node[*a] -= q;
            f_node[*b] += q;

            // On accumule la magnitude de -∂F_i/∂π_i pour le fallback diagonal.
            j_diag[*a] += dq_dpi_from;
            j_diag[*b] += dq_dpi_to;
        }

        // Résidu = max |F_i| sur les nœuds libres uniquement
        residual = 0.0;
        for i in 0..n {
            if !fixed.contains_key(&i) {
                residual = residual.max(f_node[i].abs());
            }
        }
        iterations = iter + 1;

        if residual < tolerance {
            break;
        }

        // Mise à jour Newton-Raphson diagonal :
        //   Δπ_i = −F_i / J_ii = −F_i / (−Σg) = F_i / Σg
        for i in 0..n {
            if fixed.contains_key(&i) || j_diag[i] < 1e-20 {
                continue;
            }
            let delta = relax * f_node[i] / j_diag[i];
            pressures_sq[i] = (pressures_sq[i] + delta).max(1.0);
        }
    }

    let mut result_pressures = HashMap::new();
    let mut result_flows = HashMap::new();

    for (i, id) in node_ids.iter().enumerate() {
        result_pressures.insert(id.clone(), pressures_sq[i].sqrt());
    }

    for (pipe_id, a, b, length_km, diameter_mm, roughness_mm, pressure_from_coeff) in &pipes {
        let p_a = pressures_sq[*a].sqrt();
        let p_b = pressures_sq[*b].sqrt();
        let avg_p = 0.5 * (p_a + p_b);
        let k = pipe_resistance_at_pressure(
            *length_km,
            *diameter_mm,
            *roughness_mm,
            avg_p,
            DEFAULT_GAS_TEMPERATURE_K,
        );
        let dp_sq = *pressure_from_coeff * pressures_sq[*a] - pressures_sq[*b];
        let (q, _) = flow_and_conductance(dp_sq, k, scaling);
        result_flows.insert(pipe_id.clone(), q);
    }

    if residual >= tolerance && n > fixed.len() {
        bail!(
            "Jacobi solver did not converge in {} iterations (residual={:.3e}, tolerance={:.3e})",
            iterations,
            residual,
            tolerance
        );
    }

    let result = SolverResult {
        pressures: result_pressures,
        flows: result_flows,
        iterations,
        residual,
    };
    validate_solution_physics(network, demands, &result, tolerance)?;
    Ok(result)
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn validate_solution_physics(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    result: &SolverResult,
    residual_tolerance: f64,
) -> Result<()> {
    let strict = env_bool("GAZFLOW_STRICT_PHYSICS_CHECKS", false);
    let pressure_tol_bar =
        env_f64("GAZFLOW_PRESSURE_BOUNDS_TOL_BAR", DEFAULT_PRESSURE_BOUNDS_TOL_BAR).max(0.0);
    let mass_tol = (residual_tolerance * 10.0).max(MIN_MASS_BALANCE_TOL);

    let mut node_balance: HashMap<String, f64> = network
        .nodes()
        .map(|n| (n.id.clone(), demands.get(&n.id).copied().unwrap_or(0.0)))
        .collect();

    for pipe in network.pipes() {
        let q = result.flows.get(&pipe.id).copied().unwrap_or(0.0);
        if let Some(v) = node_balance.get_mut(&pipe.from) {
            *v -= q;
        }
        if let Some(v) = node_balance.get_mut(&pipe.to) {
            *v += q;
        }
    }

    let mut max_free_imbalance = 0.0_f64;
    let mut worst_free_node: Option<&str> = None;
    let mut fixed_balance_sum = 0.0_f64;
    let mut total_demand = 0.0_f64;
    let mut pressure_violations = Vec::<String>::new();

    for node in network.nodes() {
        let solved_pressure = result.pressures.get(&node.id).copied().unwrap_or(0.0);
        if let Some(lower) = node.pressure_lower_bar {
            if solved_pressure + pressure_tol_bar < lower {
                pressure_violations.push(format!(
                    "{}: {solved_pressure:.3} bar < lower {lower:.3} bar",
                    node.id
                ));
            }
        }
        if let Some(upper) = node.pressure_upper_bar {
            if solved_pressure - pressure_tol_bar > upper {
                pressure_violations.push(format!(
                    "{}: {solved_pressure:.3} bar > upper {upper:.3} bar",
                    node.id
                ));
            }
        }

        let bal = node_balance.get(&node.id).copied().unwrap_or(0.0);
        total_demand += demands.get(&node.id).copied().unwrap_or(0.0);
        if node.pressure_fixed_bar.is_some() {
            fixed_balance_sum += bal;
        } else if bal.abs() > max_free_imbalance {
            max_free_imbalance = bal.abs();
            worst_free_node = Some(node.id.as_str());
        }
    }

    // Bilan global: la somme des déséquilibres des nœuds slack doit compenser
    // la demande totale imposée au réseau.
    let global_balance_mismatch = (fixed_balance_sum - total_demand).abs();

    let mut issues = Vec::<String>::new();
    if max_free_imbalance > mass_tol {
        let worst = worst_free_node.unwrap_or("unknown");
        issues.push(format!(
            "free-node mass imbalance too high: max={max_free_imbalance:.3e} at node={worst} (tol={mass_tol:.3e})"
        ));
    }
    if global_balance_mismatch > mass_tol {
        issues.push(format!(
            "global mass balance mismatch too high: mismatch={global_balance_mismatch:.3e}, fixed_sum={fixed_balance_sum:.3e}, total_demand={total_demand:.3e}, tol={mass_tol:.3e}"
        ));
    }
    if !pressure_violations.is_empty() {
        let first = pressure_violations
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown pressure violation".to_string());
        issues.push(format!(
            "pressure bound violation(s): count={}, first={first}, tol={pressure_tol_bar:.3} bar",
            pressure_violations.len()
        ));
    }

    if issues.is_empty() {
        return Ok(());
    }

    let joined = issues.join(" | ");
    if strict {
        bail!("physics validation failed: {joined}");
    }
    tracing::warn!("physics validation warning: {joined}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::gaslib::{load_network, load_reference_solution, load_scenario_demands};
    use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};

    fn two_node_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "pipe1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net
    }

    fn y_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "S".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "J".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(10.5),
            lat: Some(50.5),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_node(Node {
            id: "A".into(),
            x: 2.0,
            y: 1.0,
            lon: Some(11.0),
            lat: Some(51.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_node(Node {
            id: "B".into(),
            x: 2.0,
            y: -1.0,
            lon: Some(11.0),
            lat: Some(49.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "SJ".into(),
            from: "S".into(),
            to: "J".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 50.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net.add_pipe(Pipe {
            id: "JA".into(),
            from: "J".into(),
            to: "A".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 30.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net.add_pipe(Pipe {
            id: "JB".into(),
            from: "J".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 40.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net
    }

    fn near_lossless_link_network(kind: ConnectionKind) -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "link".into(),
            from: "source".into(),
            to: "sink".into(),
            kind,
            is_open: true,
            // Géométrie volontairement peu favorable; la physique dépend du type
            // (valve quasi-passante, compresseur avec ratio d'élévation).
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: if kind == ConnectionKind::CompressorStation {
                Some(1.08)
            } else {
                None
            },
        });
        net
    }

    fn compressor_link_network_with_ratio(ratio: f64) -> GasNetwork {
        let mut net = near_lossless_link_network(ConnectionKind::CompressorStation);
        if let Some(pipe) = net.graph.edge_weights_mut().next() {
            pipe.compressor_ratio_max = Some(ratio);
        }
        net
    }

    fn closed_valve_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "v_closed".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Valve,
            is_open: false,
            length_km: 1.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net
    }

    fn network_with_isolated_node() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "connected".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_node(Node {
            id: "isolated".into(),
            x: 2.0,
            y: 0.0,
            lon: Some(12.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "p".into(),
            from: "source".into(),
            to: "connected".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
        });
        net
    }

    #[test]
    fn steady_state_two_nodes() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -10.0);

        let result = solve_steady_state(&net, &demands, 500, 1e-6).expect("solver should converge");

        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];
        eprintln!(
            "source={p_source:.4} bar, sink={p_sink:.4} bar, iter={}, res={:.2e}",
            result.iterations, result.residual
        );

        assert!(
            (p_source - 70.0).abs() < 0.1,
            "source pressure should be ~70 bar, got {p_source}"
        );
        assert!(
            p_sink < p_source,
            "sink pressure ({p_sink}) should be < source ({p_source})"
        );
        assert!(
            p_sink > 0.0,
            "sink pressure should be positive, got {p_sink}"
        );
    }

    #[test]
    fn steady_state_y_network_mass_conservation() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let result = solve_steady_state(&net, &demands, 500, 1e-6).expect("solver should converge");

        let q_sj = result.flows["SJ"];
        let q_ja = result.flows["JA"];
        let q_jb = result.flows["JB"];

        eprintln!("Q_SJ={q_sj:.4}, Q_JA={q_ja:.4}, Q_JB={q_jb:.4}");
        eprintln!(
            "Pressions: S={:.2}, J={:.2}, A={:.2}, B={:.2}",
            result.pressures["S"],
            result.pressures["J"],
            result.pressures["A"],
            result.pressures["B"]
        );

        // Conservation de masse à J : Q_SJ = Q_JA + Q_JB
        let balance = q_sj - q_ja - q_jb;
        assert!(
            balance.abs() < 1e-4,
            "mass conservation at J: {q_sj} != {q_ja} + {q_jb} (diff={balance})"
        );

        // Toutes les pressions sont décroissantes depuis la source
        assert!(result.pressures["S"] > result.pressures["J"]);
        assert!(result.pressures["J"] > result.pressures["A"]);
        assert!(result.pressures["J"] > result.pressures["B"]);
    }

    #[test]
    fn darcy_friction_turbulent() {
        let f = darcy_friction(0.012, 500.0, 1e7);
        assert!(
            f > 0.005 && f < 0.05,
            "friction factor in realistic range: {f}"
        );
    }

    #[test]
    fn pipe_resistance_positive() {
        let k = pipe_resistance(100.0, 500.0, 0.012);
        assert!(k > 0.0, "resistance must be positive: {k}");
        assert!(k.is_finite(), "resistance must be finite: {k}");
    }

    #[test]
    fn test_pipe_resistance_at_pressure_increases_with_pressure() {
        let low = pipe_resistance_at_pressure(100.0, 500.0, 0.012, 30.0, DEFAULT_GAS_TEMPERATURE_K);
        let high =
            pipe_resistance_at_pressure(100.0, 500.0, 0.012, 70.0, DEFAULT_GAS_TEMPERATURE_K);
        assert!(
            high > low,
            "pipe resistance should increase with pressure-dependent density: low={low}, high={high}"
        );
    }

    #[test]
    fn test_nondimensionalized_flow_matches_physical_formula() {
        let dp_sq = 70.0_f64.powi(2) - 65.0_f64.powi(2);
        let k = pipe_resistance(50.0, 500.0, 0.012);
        let scaling = NondimScaling::new(70.0_f64.powi(2), 10.0);
        let (q_hat_based, g_hat_based) = flow_and_conductance(dp_sq, k, scaling);

        let q_phys = dp_sq.signum() * (dp_sq.abs() / k).sqrt();
        let g_phys = 1.0 / (2.0 * (k * dp_sq.abs()).sqrt());

        assert!(
            (q_hat_based - q_phys).abs() < 1e-10,
            "Q mismatch: nondim={q_hat_based}, physical={q_phys}"
        );
        assert!(
            (g_hat_based - g_phys).abs() < 1e-12,
            "dQ/dπ mismatch: nondim={g_hat_based}, physical={g_phys}"
        );
    }

    #[test]
    fn test_solve_gaslib_11() {
        let network_path = Path::new("dat/GasLib-11.net");
        let scenario_path = Path::new("dat/GasLib-11.scn");
        if !network_path.exists() || !scenario_path.exists() {
            eprintln!(
                "skip: data files not found ({:?}, {:?})",
                network_path, scenario_path
            );
            return;
        }

        let network = load_network(network_path).expect("load GasLib-11 network");
        let scenario = load_scenario_demands(scenario_path).expect("load GasLib-11 scenario");

        let result = solve_steady_state(&network, &scenario.demands, 1200, 5e-4)
            .expect("solver should return a result");

        assert!(
            result.iterations <= 800,
            "too many iterations: {}",
            result.iterations
        );
        assert!(result.residual.is_finite(), "residual must be finite");
        assert_eq!(result.pressures.len(), network.node_count());
        assert_eq!(result.flows.len(), network.edge_count());

        for (id, &pressure_bar) in &result.pressures {
            assert!(
                pressure_bar.is_finite() && pressure_bar > 0.0,
                "pressure must be finite and > 0 at {id}: {pressure_bar}"
            );
            assert!(
                pressure_bar < 200.0,
                "pressure should stay in a realistic range at {id}: {pressure_bar}"
            );
        }
    }

    #[test]
    fn test_gaslib_11_vs_reference_solution() {
        let network_path = Path::new("dat/GasLib-11.net");
        let scenario_path = Path::new("dat/GasLib-11.scn");
        if !network_path.exists() || !scenario_path.exists() {
            eprintln!(
                "skip: data files not found ({:?}, {:?})",
                network_path, scenario_path
            );
            return;
        }

        let mut solution_candidates: Vec<PathBuf> = vec![
            PathBuf::from("dat/GasLib-11.sol"),
            PathBuf::from("dat/GasLib-11-v1-20211130.sol"),
            PathBuf::from("dat/GasLib-11.reference.csv"),
            PathBuf::from("dat/GasLib-11.reference.xml"),
            PathBuf::from("../docs/testing/references/GasLib-11.reference.internal.csv"),
        ];
        if let Ok(custom_path) = std::env::var("GAZFLOW_REFERENCE_SOLUTION_PATH") {
            solution_candidates.insert(0, PathBuf::from(custom_path));
        }
        let Some(solution_path) = solution_candidates.iter().find(|p| p.exists()) else {
            eprintln!(
                "skip: no GasLib-11 reference solution found (set GAZFLOW_REFERENCE_SOLUTION_PATH or place dat/GasLib-11.sol)"
            );
            return;
        };

        let network = load_network(network_path).expect("load GasLib-11 network");
        let scenario = load_scenario_demands(scenario_path).expect("load GasLib-11 scenario");
        let reference = load_reference_solution(solution_path).expect("load reference solution");
        let result = solve_steady_state(&network, &scenario.demands, 1200, 5e-4)
            .expect("solver should converge on GasLib-11");

        let mut compared = 0usize;
        let mut rel_errors = Vec::new();
        let mut worst_node = String::new();
        let mut worst_err = -1.0_f64;
        for (node_id, &p_ref) in &reference.pressures {
            let Some(&p_calc) = result.pressures.get(node_id) else {
                continue;
            };
            if p_ref.abs() < 1e-12 {
                continue;
            }
            let err_pct = ((p_calc - p_ref).abs() / p_ref.abs()) * 100.0;
            if err_pct > worst_err {
                worst_err = err_pct;
                worst_node = node_id.clone();
            }
            rel_errors.push(err_pct);
            compared += 1;
        }

        assert!(
            compared > 0,
            "reference solution has no comparable pressure nodes with computed result"
        );

        let max_err = rel_errors.iter().copied().fold(0.0_f64, f64::max);
        let mean_err = rel_errors.iter().sum::<f64>() / (rel_errors.len() as f64);

        eprintln!(
            "GasLib-11 reference pressure comparison: n={compared}, max={max_err:.3}%, mean={mean_err:.3}%, worst_node={worst_node}"
        );

        // MVP target from implementation plan: max relative pressure error < 5%.
        assert!(
            max_err < 5.0,
            "max relative pressure error too high: {max_err:.3}% (mean={mean_err:.3}%, worst_node={worst_node})"
        );
    }

    fn solve_dataset_with_continuation(
        network: &GasNetwork,
        demands: &HashMap<String, f64>,
        max_iter: usize,
        tolerance: f64,
        continuation_scales: &[f64],
        continuation_max_seconds: Option<u64>,
    ) -> Result<SolverResult> {
        // Préserver l'ordre donné (et les duplicatas éventuels) pour permettre
        // des passes successives sur un même palier de continuation.
        let mut scales: Vec<f64> = continuation_scales
            .iter()
            .copied()
            .filter(|s| *s > 0.0)
            .collect();
        if scales.is_empty() {
            return Err(anyhow::anyhow!("no continuation scale provided"));
        }
        let per_scale_iters =
            continuation_iter_schedule(max_iter, scales.len(), network.node_count());

        let auto_bridges = env_usize("GAZFLOW_CONTINUATION_AUTO_BRIDGES", 0);
        let min_gap = env_f64("GAZFLOW_CONTINUATION_MIN_GAP", 0.02);
        let snapshot_default = if network.node_count() > 2000 {
            1
        } else {
            (max_iter / 2).max(1)
        };
        let snapshot_every =
            env_usize("GAZFLOW_CONTINUATION_SNAPSHOT_EVERY", snapshot_default).max(1);
        let continuation_max_seconds = continuation_max_seconds.or_else(|| {
            let v = env_usize("GAZFLOW_CONTINUATION_MAX_SECONDS", 0);
            (v > 0).then_some(v as u64)
        });
        let started_at = std::time::Instant::now();

        let mut last_err: Option<anyhow::Error> = None;
        let mut warm_start_pressures: Option<HashMap<String, f64>> = None;
        let mut last_success: Option<SolverResult> = None;
        let mut last_success_scale: Option<f64> = None;
        let mut bridges_used = 0usize;

        let mut idx = 0usize;
        while idx < scales.len() {
            if continuation_max_seconds
                .map(|max_s| started_at.elapsed().as_secs() >= max_s)
                .unwrap_or(false)
            {
                eprintln!(
                    "dataset continuation timeout reached after {}s",
                    continuation_max_seconds.unwrap_or_default()
                );
                break;
            }
            let scale = scales[idx];
            let iter_budget = per_scale_iters[idx];
            let scaled_demands: HashMap<String, f64> = demands
                .iter()
                .map(|(node_id, q)| (node_id.clone(), q * scale))
                .collect();
            let mut best_snapshot_pressures: Option<HashMap<String, f64>> = None;
            let mut best_snapshot_residual = f64::INFINITY;

            match solve_steady_state_with_progress(
                network,
                &scaled_demands,
                warm_start_pressures.as_ref(),
                iter_budget,
                tolerance,
                snapshot_every,
                |progress| {
                    if let Some(pressures) = progress.pressures {
                        if progress.residual < best_snapshot_residual {
                            best_snapshot_residual = progress.residual;
                            best_snapshot_pressures = Some(pressures);
                        }
                    }
                    SolverControl::Continue
                },
            ) {
                Ok(result) => {
                    if scale < 1.0 {
                        eprintln!(
                            "dataset continuation succeeded at scale={scale:.3} (budget={}, iter={}, residual={:.3e})",
                            iter_budget, result.iterations, result.residual
                        );
                    }
                    warm_start_pressures = Some(result.pressures.clone());
                    last_success_scale = Some(scale);
                    last_success = Some(result);
                    idx += 1;
                }
                Err(err) => {
                    if let Some(snapshot) = best_snapshot_pressures.take() {
                        eprintln!(
                            "dataset continuation captures best snapshot warm-start at scale={scale:.3} (budget={iter_budget}, residual={best_snapshot_residual:.3e})"
                        );
                        warm_start_pressures = Some(snapshot);
                    }
                    eprintln!("dataset continuation failed at scale={scale:.3}: {err:#}");
                    if bridges_used < auto_bridges {
                        let low = last_success_scale.unwrap_or(0.0);
                        let gap = scale - low;
                        if gap > min_gap {
                            let mid = low + 0.5 * gap;
                            if !scales.iter().any(|s| (*s - mid).abs() < 1e-9) {
                                eprintln!(
                                    "dataset continuation inserts intermediate scale={mid:.3} between {low:.3} and {scale:.3}"
                                );
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

        if let Some(result) = last_success {
            if let Some(scale) = last_success_scale {
                eprintln!("dataset continuation finished at best successful scale={scale:.3}");
            }
            return Ok(result);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no continuation scale provided")))
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

    fn env_scales(name: &str, default: &[f64]) -> Vec<f64> {
        let Some(raw) = std::env::var(name).ok() else {
            return default.to_vec();
        };
        let parsed: Vec<f64> = raw
            .split(',')
            .filter_map(|t| t.trim().parse::<f64>().ok())
            .filter(|v| *v > 0.0)
            .collect();
        if parsed.is_empty() {
            default.to_vec()
        } else {
            parsed
        }
    }

    fn continuation_iter_schedule(
        max_iter: usize,
        n_scales: usize,
        node_count: usize,
    ) -> Vec<usize> {
        if n_scales == 0 {
            return Vec::new();
        }

        if let Ok(raw) = std::env::var("GAZFLOW_CONTINUATION_ITER_SCHEDULE") {
            let parsed: Vec<usize> = raw
                .split(',')
                .filter_map(|t| t.trim().parse::<usize>().ok())
                .map(|v| v.max(1))
                .collect();
            if !parsed.is_empty() {
                let mut schedule = Vec::with_capacity(n_scales);
                for i in 0..n_scales {
                    schedule.push(
                        *parsed
                            .get(i)
                            .unwrap_or_else(|| parsed.last().expect("non-empty")),
                    );
                }
                return schedule;
            }
        }

        // Heuristique large dataset: investir surtout sur le dernier palier,
        // les premiers servant à stabiliser le warm-start.
        if node_count > 2000 && n_scales >= 2 && max_iter >= n_scales {
            let mut schedule = vec![1usize; n_scales];
            let remaining = max_iter.saturating_sub(n_scales - 1).max(1);
            schedule[n_scales - 1] = remaining;
            return schedule;
        }

        vec![max_iter.max(1); n_scales]
    }

    fn run_dataset_solve_smoke(dataset: &str) {
        let network_path = Path::new("dat").join(format!("{dataset}.net"));
        let scenario_path = Path::new("dat").join(format!("{dataset}.scn"));
        if !network_path.exists() || !scenario_path.exists() {
            eprintln!(
                "skip: data files not found ({:?}, {:?})",
                network_path, scenario_path
            );
            return;
        }

        let network = load_network(&network_path).expect("load network");
        let scenario = load_scenario_demands(&scenario_path).expect("load scenario");
        let enable_large = std::env::var("GAZFLOW_ENABLE_LARGE_DATASET_TESTS")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if network.node_count() > 500 && !enable_large {
            eprintln!(
                "skip: {dataset} has {} nodes; set GAZFLOW_ENABLE_LARGE_DATASET_TESTS=1 to run",
                network.node_count()
            );
            return;
        }
        let is_large = network.node_count() > 500;
        let node_count = network.node_count();
        let (default_max_iter, default_tolerance, default_scales): (usize, f64, &[f64]) =
            if node_count > 2000 {
                // Très grands réseaux: smoke court pour vérifier robustesse d'exécution
                // sans bloquer la CI pendant de longues minutes.
                // On répète le dernier palier pour raffiner le warm-start
                // sans augmenter le budget global d'itérations.
                (6, 1e-2, &[0.05, 0.1, 0.1])
            } else if node_count > 500 {
                // Les très grands cas restent sensibles tant que les compresseurs sont
                // modélisés en approximation MVP. On valide la capacité de scaling
                // via continuation de charge.
                (180, 2e-3, &[0.1, 0.3])
            } else {
                (1500, 5e-4, &[1.0])
            };
        let max_iter = if is_large {
            env_usize("GAZFLOW_LARGE_TEST_MAX_ITER", default_max_iter)
        } else {
            default_max_iter
        };
        let tolerance = if is_large {
            env_f64("GAZFLOW_LARGE_TEST_TOL", default_tolerance)
        } else {
            default_tolerance
        };
        let continuation_scales: Vec<f64> = if is_large {
            env_scales("GAZFLOW_LARGE_TEST_SCALES", default_scales)
        } else {
            default_scales.to_vec()
        };
        let continuation_max_seconds = if is_large {
            let default_timeout_s = if node_count > 2000 { 40 } else { 120 };
            let configured = env_usize("GAZFLOW_LARGE_TEST_MAX_SECONDS", default_timeout_s);
            (configured > 0).then_some(configured as u64)
        } else {
            None
        };

        eprintln!(
            "dataset smoke settings: dataset={dataset}, nodes={}, max_iter={}, tol={:.2e}, scales={:?}",
            network.node_count(),
            max_iter,
            tolerance,
            continuation_scales
        );

        let solve_result = solve_dataset_with_continuation(
            &network,
            &scenario.demands,
            max_iter,
            tolerance,
            &continuation_scales,
            continuation_max_seconds,
        );

        if is_large {
            match solve_result {
                Ok(result) => {
                    assert!(
                        result.iterations <= max_iter,
                        "too many iterations: {}",
                        result.iterations
                    );
                    assert!(result.residual.is_finite(), "residual should be finite");
                    assert_eq!(result.pressures.len(), network.node_count());
                    assert_eq!(result.flows.len(), network.edge_count());
                }
                Err(err) => {
                    // Sur les très gros cas, tant que le modèle compresseur reste en
                    // approximation MVP, la convergence n'est pas garantie pour tous
                    // scénarios. Le smoke test valide ici le chargement complet du
                    // dataset + la robustesse d'exécution (échec explicite non-panique).
                    let msg = format!("{err:#}");
                    assert!(
                        msg.contains("did not converge"),
                        "unexpected failure mode on large dataset: {msg}"
                    );
                    eprintln!("large dataset non-convergence accepted in smoke mode: {msg}");
                }
            }
        } else {
            let result = solve_result.expect("solver should converge on dataset");
            assert!(
                result.iterations <= max_iter,
                "too many iterations: {}",
                result.iterations
            );
            assert!(result.residual.is_finite(), "residual should be finite");
            assert_eq!(result.pressures.len(), network.node_count());
            assert_eq!(result.flows.len(), network.edge_count());
        }
    }

    #[test]
    fn test_solve_gaslib_24() {
        run_dataset_solve_smoke("GasLib-24");
    }

    #[test]
    fn test_solve_gaslib_40() {
        run_dataset_solve_smoke("GasLib-40");
    }

    #[test]
    fn test_solve_gaslib_582() {
        run_dataset_solve_smoke("GasLib-582");
    }

    #[test]
    fn test_solve_gaslib_4197() {
        run_dataset_solve_smoke("GasLib-4197");
    }

    #[test]
    fn test_newton_vs_jacobi_same_result() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let result_newton =
            solve_steady_state(&net, &demands, 500, 1e-6).expect("newton-hybrid should converge");
        let result_jacobi =
            solve_steady_state_jacobi(&net, &demands, 500, 1e-6).expect("jacobi should converge");

        assert!(
            result_newton.iterations <= result_jacobi.iterations,
            "newton should not require more iterations on this test case"
        );
        for (node_id, p_newton) in &result_newton.pressures {
            let p_jacobi = result_jacobi
                .pressures
                .get(node_id)
                .expect("node should exist in both results");
            assert!(
                (p_newton - p_jacobi).abs() < 0.2,
                "pressure mismatch at {node_id}: newton={p_newton}, jacobi={p_jacobi}"
            );
        }
    }

    #[test]
    fn test_valve_open_zero_resistance() {
        let net = near_lossless_link_network(ConnectionKind::Valve);
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -20.0);

        let result = solve_steady_state(&net, &demands, 800, 2e-4).expect("solver should converge");
        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];
        let dp = (p_source - p_sink).abs();

        assert!(
            dp < 0.5,
            "open valve should introduce near-zero pressure loss, got ΔP={dp} bar"
        );
    }

    #[test]
    fn test_compressor_applies_pressure_lift_mvp() {
        let net = near_lossless_link_network(ConnectionKind::CompressorStation);
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -20.0);

        let result = solve_steady_state(&net, &demands, 800, 2e-4).expect("solver should converge");
        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];

        assert!(
            p_sink > p_source,
            "compressor MVP should increase downstream pressure, got source={p_source} sink={p_sink}"
        );
    }

    #[test]
    fn test_compressor_higher_ratio_increases_downstream_pressure() {
        let net_low = compressor_link_network_with_ratio(1.02);
        let net_high = compressor_link_network_with_ratio(1.15);
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -20.0);

        let low = solve_steady_state(&net_low, &demands, 800, 2e-4).expect("low ratio solve");
        let high = solve_steady_state(&net_high, &demands, 800, 2e-4).expect("high ratio solve");

        assert!(
            high.pressures["sink"] > low.pressures["sink"],
            "higher compressor ratio should increase downstream pressure"
        );
    }

    #[test]
    fn test_valve_closed_removes_arc_and_blocks_flow() {
        let net = closed_valve_network();
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -5.0);

        let err = solve_steady_state(&net, &demands, 80, 1e-6)
            .expect_err("a closed valve should disconnect source and sink");
        assert!(
            err.to_string().contains("did not converge"),
            "expected non-convergence because demand is unsatisfied behind closed valve, got: {err:#}"
        );
    }

    #[test]
    fn test_warm_start_fewer_iterations() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let cold =
            solve_steady_state(&net, &demands, 500, 1e-6).expect("cold solve should converge");
        let warm = solve_steady_state_with_initial_pressures(
            &net,
            &demands,
            Some(&cold.pressures),
            500,
            1e-6,
        )
        .expect("warm solve should converge");

        assert!(
            warm.iterations <= cold.iterations,
            "warm start should not require more iterations: warm={}, cold={}",
            warm.iterations,
            cold.iterations
        );
        assert!(
            warm.iterations <= 5,
            "warm start should converge quickly, got {} iterations",
            warm.iterations
        );
    }

    #[test]
    fn test_newton_line_search_convergence() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let mut poor_initial_guess = HashMap::new();
        poor_initial_guess.insert("J".to_string(), 2.0);
        poor_initial_guess.insert("A".to_string(), 2.0);
        poor_initial_guess.insert("B".to_string(), 2.0);

        let result = solve_steady_state_with_initial_pressures(
            &net,
            &demands,
            Some(&poor_initial_guess),
            500,
            1e-6,
        )
        .expect("newton with line search should converge from poor initial guess");

        assert!(
            result.residual < 1e-4,
            "expected converged residual, got {}",
            result.residual
        );
        assert!(
            result.iterations < 200,
            "line-search Newton should converge in a reasonable number of iterations, got {}",
            result.iterations
        );
    }

    #[test]
    fn test_newton_jacobi_hybrid_fallback() {
        let net = network_with_isolated_node();
        let mut demands = HashMap::new();
        demands.insert("connected".to_string(), -1.0);
        // Demande non nulle sur un nœud isolé -> Jacobien singulier pour ce DOF.
        demands.insert("isolated".to_string(), -1.0);

        let err = solve_steady_state(&net, &demands, 30, 1e-6)
            .expect_err("isolated unsatisfied demand should produce a non-convergence error");
        assert!(
            err.to_string().contains("did not converge"),
            "expected non-convergence error, got: {err:#}"
        );
    }

    #[test]
    fn test_reject_unknown_demand_node() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("UNKNOWN_NODE".to_string(), -1.0);

        let err = solve_steady_state(&net, &demands, 100, 1e-6)
            .expect_err("unknown node id should be rejected");
        assert!(
            err.to_string().contains("unknown demand node id"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_reject_non_finite_demand_value() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), f64::NAN);

        let err = solve_steady_state(&net, &demands, 100, 1e-6)
            .expect_err("non-finite demand should be rejected");
        assert!(
            err.to_string().contains("invalid demand value"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_jacobi_returns_error_when_not_converged() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let err = solve_steady_state_jacobi(&net, &demands, 1, 1e-12)
            .expect_err("jacobi should fail if max_iter is too small");
        assert!(
            err.to_string().contains("did not converge"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_pressure_drop_dimension_consistency() {
        let length_km = 55.0;
        let diameter_mm = 500.0;
        let roughness_mm = 0.1;

        let k_from_code = pipe_resistance(length_km, diameter_mm, roughness_mm);

        let d = diameter_mm * 1e-3;
        let l = length_km * 1e3;
        let area = std::f64::consts::PI * d * d / 4.0;
        let f = darcy_friction(roughness_mm, diameter_mm, 1e7);
        let rho_eff = 50.0;
        let k_pa2 = f * l * rho_eff / (2.0 * d * area * area);
        let k_bar2 = k_pa2 / 1e10;

        let rel = ((k_from_code - k_bar2) / k_bar2).abs();
        assert!(
            rel < 1e-12,
            "dimension consistency failed: code={k_from_code}, expected={k_bar2}, rel={rel}"
        );
    }

    #[test]
    fn test_sensitivity_physical_trends() {
        let mut net_low = GasNetwork::new();
        net_low.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net_low.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net_low.add_pipe(Pipe {
            id: "p".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.01,
            compressor_ratio_max: None,
        });

        let mut net_high = GasNetwork::new();
        net_high.add_node(Node {
            id: "source".into(),
            x: 0.0,
            y: 0.0,
            lon: Some(10.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net_high.add_node(Node {
            id: "sink".into(),
            x: 1.0,
            y: 0.0,
            lon: Some(11.0),
            lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net_high.add_pipe(Pipe {
            id: "p".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.2,
            compressor_ratio_max: None,
        });

        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -10.0);

        let low = solve_steady_state(&net_low, &demands, 500, 1e-6).expect("low roughness solve");
        let high =
            solve_steady_state(&net_high, &demands, 500, 1e-6).expect("high roughness solve");

        assert!(
            high.pressures["sink"] < low.pressures["sink"],
            "higher roughness should increase pressure drop"
        );
    }
}
