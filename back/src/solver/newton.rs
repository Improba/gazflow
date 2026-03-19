use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use faer::Mat;
use faer::prelude::Solve;
use faer::sparse::{SparseColMat, Triplet};
use rayon::prelude::*;

use crate::graph::GasNetwork;

use super::gas_properties::DEFAULT_GAS_TEMPERATURE_K;
use super::iterative::solve_sparse_gmres_ilu0;
use super::steady_state::{
    NondimScaling, SolverControl, SolverProgress, SolverResult, compressor_pressure_from_coeff,
    effective_pipe_geometry, flow_and_conductance, flow_reference_from_demands,
    pipe_resistance_at_pressure, pressure_sq_reference_from_fixed,
};

const MIN_PRESSURE_SQ: f64 = 1.0;
const MIN_ABS_DP: f64 = 1e-10;
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
    if node_count > 2000 { 2 } else { 0 }
}

#[derive(Debug, Clone)]
struct IndexedPipe {
    id: String,
    from_idx: usize,
    to_idx: usize,
    length_km: f64,
    diameter_mm: f64,
    roughness_mm: f64,
    pressure_from_coeff: f64,
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
    max_iter: usize,
    tolerance: f64,
    snapshot_every: usize,
    mut on_progress: F,
) -> Result<SolverResult>
where
    F: FnMut(SolverProgress) -> SolverControl,
{
    let n = network.node_count();
    if n == 0 {
        return Ok(SolverResult {
            pressures: HashMap::new(),
            flows: HashMap::new(),
            iterations: 0,
            residual: 0.0,
        });
    }

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

    let scaling = NondimScaling::new(
        pressure_sq_reference_from_fixed(&fixed),
        flow_reference_from_demands(&demands_vec),
    );

    let pipes: Vec<IndexedPipe> = network
        .pipes()
        .filter_map(|pipe| {
            if pipe.kind == crate::graph::ConnectionKind::Valve && !pipe.is_open {
                return None;
            }
            let from_idx = id_pos.get(&pipe.from).copied()?;
            let to_idx = id_pos.get(&pipe.to).copied()?;
            let (length_km, diameter_mm, roughness_mm) = effective_pipe_geometry(pipe);
            Some(IndexedPipe {
                id: pipe.id.clone(),
                from_idx,
                to_idx,
                length_km,
                diameter_mm,
                roughness_mm,
                pressure_from_coeff: compressor_pressure_from_coeff(pipe),
            })
        })
        .collect();

    let free_indices: Vec<usize> = (0..n).filter(|i| !fixed.contains_key(i)).collect();
    let mut free_pos = vec![usize::MAX; n];
    for (pos, &node_idx) in free_indices.iter().enumerate() {
        free_pos[node_idx] = pos;
    }
    let guard_jacobi_fallback = env_bool("GAZFLOW_GUARD_JACOBI_FALLBACK", n > 2000);

    if initial_pressures_bar.is_none() && !free_indices.is_empty() {
        if let Some(candidate) =
            build_physical_initial_guess(n, &pipes, &demands_vec, &fixed, scaling.pressure_sq_ref)
        {
            let baseline_residual =
                evaluate_state(&pipes, &demands_vec, &pressures_sq, &free_indices, scaling)
                    .residual;
            let candidate_residual =
                evaluate_state(&pipes, &demands_vec, &candidate, &free_indices, scaling).residual;
            if candidate_residual.is_finite() && candidate_residual < baseline_residual {
                pressures_sq = candidate;
            }
        }
    }

    let mut iterations = 0usize;
    let disable_jacobi = disable_jacobi_fallback();
    for iter in 0..max_iter {
        let state = evaluate_state(&pipes, &demands_vec, &pressures_sq, &free_indices, scaling);
        let residual = state.residual;
        iterations = iter + 1;

        let snapshot_due = snapshot_every > 0 && iterations % snapshot_every == 0;
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
        let jacobian_triplets: Vec<(usize, usize, f64)> = if pipes.len() >= PARALLEL_PIPE_THRESHOLD
        {
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

        let rhs: Vec<f64> = free_indices.iter().map(|&idx| -state.f_node[idx]).collect();

        let gmres_max_iters_default = if m > 1200 {
            220
        } else {
            GMRES_MAX_ITERS
        };
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
                        &pipes,
                        &demands_vec,
                        scaling,
                    );
                } else {
                    apply_jacobi_fallback(
                        &mut pressures_sq,
                        &free_indices,
                        &state.f_node,
                        &state.j_diag,
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
                trial_pressures[idx] =
                    (trial_pressures[idx] + alpha * delta_free[pos]).max(MIN_PRESSURE_SQ);
            }

            let trial_state = evaluate_state(
                &pipes,
                &demands_vec,
                &trial_pressures,
                &free_indices,
                scaling,
            );
            if trial_state.residual < residual {
                pressures_sq = trial_pressures;
                accepted = true;
                break;
            }
            alpha *= 0.5;
        }

        if !accepted {
            if !disable_jacobi {
                if guard_jacobi_fallback {
                    try_apply_jacobi_fallback_if_improves(
                        &mut pressures_sq,
                        &free_indices,
                        &state.f_node,
                        &state.j_diag,
                        residual,
                        &pipes,
                        &demands_vec,
                        scaling,
                    );
                } else {
                    apply_jacobi_fallback(
                        &mut pressures_sq,
                        &free_indices,
                        &state.f_node,
                        &state.j_diag,
                    );
                }
            }
        }
    }

    let final_state = evaluate_state(&pipes, &demands_vec, &pressures_sq, &free_indices, scaling);

    if final_state.residual >= tolerance && !free_indices.is_empty() {
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

    Ok(SolverResult {
        pressures: result_pressures,
        flows: result_flows,
        iterations,
        residual: final_state.residual,
    })
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

fn evaluate_state(
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    pressures_sq: &[f64],
    free_indices: &[usize],
    scaling: NondimScaling,
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
                    let dp_sq = pipe.pressure_from_coeff * pressures_sq[pipe.from_idx]
                        - pressures_sq[pipe.to_idx];
                    let p_from = pressures_sq[pipe.from_idx].sqrt();
                    let p_to = pressures_sq[pipe.to_idx].sqrt();
                    let avg_p = 0.5 * (p_from + p_to);
                    let resistance = pipe_resistance_at_pressure(
                        pipe.length_km,
                        pipe.diameter_mm,
                        pipe.roughness_mm,
                        avg_p,
                        DEFAULT_GAS_TEMPERATURE_K,
                    )
                    .max(MIN_ABS_DP);
                    let (q, g) = flow_and_conductance(dp_sq, resistance, scaling);
                    let dq_dpi_from = g * pipe.pressure_from_coeff;
                    let dq_dpi_to = g;

                    local_f[pipe.from_idx] -= q;
                    local_f[pipe.to_idx] += q;
                    local_j[pipe.from_idx] += dq_dpi_from;
                    local_j[pipe.to_idx] += dq_dpi_to;
                    local_qg.push((pipe_idx, q, dq_dpi_from, dq_dpi_to));
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
            let dp_sq =
                pipe.pressure_from_coeff * pressures_sq[pipe.from_idx] - pressures_sq[pipe.to_idx];
            let p_from = pressures_sq[pipe.from_idx].sqrt();
            let p_to = pressures_sq[pipe.to_idx].sqrt();
            let avg_p = 0.5 * (p_from + p_to);
            let resistance = pipe_resistance_at_pressure(
                pipe.length_km,
                pipe.diameter_mm,
                pipe.roughness_mm,
                avg_p,
                DEFAULT_GAS_TEMPERATURE_K,
            )
            .max(MIN_ABS_DP);
            let (q, g) = flow_and_conductance(dp_sq, resistance, scaling);
            let dq_dpi_from = g * pipe.pressure_from_coeff;
            let dq_dpi_to = g;

            f_node[pipe.from_idx] -= q;
            f_node[pipe.to_idx] += q;
            j_diag[pipe.from_idx] += dq_dpi_from;
            j_diag[pipe.to_idx] += dq_dpi_to;

            flows[pipe_idx] = q;
            conductances_from[pipe_idx] = dq_dpi_from;
            conductances_to[pipe_idx] = dq_dpi_to;
        }
    }

    let residual = free_indices
        .iter()
        .map(|&idx| f_node[idx].abs())
        .fold(0.0, f64::max);

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
) -> Option<Vec<f64>> {
    let init_iters = physical_init_iters(node_count);
    if init_iters == 0 || pipes.is_empty() {
        return None;
    }

    let ref_pressure_bar = pressure_sq_ref.sqrt().max(1.0);
    let linear_conductances: Vec<f64> = pipes
        .iter()
        .map(|pipe| {
            let resistance = pipe_resistance_at_pressure(
                pipe.length_km,
                pipe.diameter_mm,
                pipe.roughness_mm,
                ref_pressure_bar,
                DEFAULT_GAS_TEMPERATURE_K,
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
            let q_lin = c
                * (pipe.pressure_from_coeff * pressures_sq[pipe.from_idx]
                    - pressures_sq[pipe.to_idx]);

            f_node[pipe.from_idx] -= q_lin;
            f_node[pipe.to_idx] += q_lin;
            j_diag[pipe.from_idx] += c * pipe.pressure_from_coeff;
            j_diag[pipe.to_idx] += c;
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

        let sparse = super::solve_sparse_linear(m, &triplets, &rhs).expect("sparse solve");
        let dense = super::solve_dense_from_triplets(m, &triplets, rhs).expect("dense solve");

        for (a, b) in sparse.iter().zip(dense.iter()) {
            assert!(
                (a - b).abs() < 1e-10,
                "delta mismatch: sparse={a}, dense={b}"
            );
        }
    }
}

fn try_apply_jacobi_fallback_if_improves(
    pressures_sq: &mut Vec<f64>,
    free_indices: &[usize],
    f_node: &[f64],
    j_diag: &[f64],
    current_residual: f64,
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    scaling: NondimScaling,
) {
    let mut candidate = pressures_sq.clone();
    for &idx in free_indices {
        if j_diag[idx] > 1e-20 {
            let delta = JACOBI_RELAX * f_node[idx] / j_diag[idx];
            candidate[idx] = (candidate[idx] + delta).max(MIN_PRESSURE_SQ);
        }
    }
    let candidate_state = evaluate_state(pipes, demands_vec, &candidate, free_indices, scaling);
    if candidate_state.residual < current_residual {
        *pressures_sq = candidate;
    }
}

fn apply_jacobi_fallback(
    pressures_sq: &mut [f64],
    free_indices: &[usize],
    f_node: &[f64],
    j_diag: &[f64],
) {
    for &idx in free_indices {
        if j_diag[idx] > 1e-20 {
            let delta = JACOBI_RELAX * f_node[idx] / j_diag[idx];
            pressures_sq[idx] = (pressures_sq[idx] + delta).max(MIN_PRESSURE_SQ);
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
            for k in col..n {
                a[row][k] -= factor * a[col][k];
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
    .map_err(|_| {
        SPARSE_LU_ENABLED.store(false, Ordering::Relaxed);
    })
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
