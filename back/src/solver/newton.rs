use std::collections::HashMap;

use anyhow::Result;

use crate::graph::GasNetwork;

use super::steady_state::{SolverResult, effective_pipe_resistance};

const MIN_PRESSURE_SQ: f64 = 1.0;
const MIN_ABS_DP: f64 = 1e-10;
const JACOBI_RELAX: f64 = 0.8;
const MAX_BACKTRACK_STEPS: usize = 5;
const PIVOT_EPS: f64 = 1e-14;

#[derive(Debug, Clone)]
struct IndexedPipe {
    id: String,
    from_idx: usize,
    to_idx: usize,
    resistance: f64,
}

#[derive(Debug, Clone)]
struct IterationState {
    f_node: Vec<f64>,
    j_diag: Vec<f64>,
    flows: Vec<f64>,
    conductances: Vec<f64>,
    residual: f64,
}

pub(crate) fn solve_steady_state_newton_hybrid(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
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
        if let Some(&idx) = id_pos.get(id) {
            demands_vec[idx] += demand;
        }
    }

    let pipes: Vec<IndexedPipe> = network
        .pipes()
        .filter_map(|pipe| {
            let from_idx = id_pos.get(&pipe.from).copied()?;
            let to_idx = id_pos.get(&pipe.to).copied()?;
            Some(IndexedPipe {
                id: pipe.id.clone(),
                from_idx,
                to_idx,
                resistance: effective_pipe_resistance(pipe),
            })
        })
        .collect();

    let free_indices: Vec<usize> = (0..n).filter(|i| !fixed.contains_key(i)).collect();
    let mut free_pos = vec![usize::MAX; n];
    for (pos, &node_idx) in free_indices.iter().enumerate() {
        free_pos[node_idx] = pos;
    }

    let mut iterations = 0usize;
    for iter in 0..max_iter {
        let state = evaluate_state(&pipes, &demands_vec, &pressures_sq, &free_indices);
        let residual = state.residual;
        iterations = iter + 1;

        if residual < tolerance || free_indices.is_empty() {
            break;
        }

        let m = free_indices.len();
        let mut jacobian = vec![vec![0.0_f64; m]; m];
        for (pipe_idx, pipe) in pipes.iter().enumerate() {
            let g = state.conductances[pipe_idx];
            let a_free = free_pos[pipe.from_idx];
            let b_free = free_pos[pipe.to_idx];

            if a_free != usize::MAX {
                jacobian[a_free][a_free] -= g;
            }
            if b_free != usize::MAX {
                jacobian[b_free][b_free] -= g;
            }
            if a_free != usize::MAX && b_free != usize::MAX {
                jacobian[a_free][b_free] += g;
                jacobian[b_free][a_free] += g;
            }
        }

        let rhs: Vec<f64> = free_indices.iter().map(|&idx| -state.f_node[idx]).collect();

        let Some(delta_free) = solve_dense_linear(jacobian, rhs) else {
            apply_jacobi_fallback(
                &mut pressures_sq,
                &free_indices,
                &state.f_node,
                &state.j_diag,
            );
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

            let trial_state = evaluate_state(&pipes, &demands_vec, &trial_pressures, &free_indices);
            if trial_state.residual < residual {
                pressures_sq = trial_pressures;
                accepted = true;
                break;
            }
            alpha *= 0.5;
        }

        if !accepted {
            apply_jacobi_fallback(
                &mut pressures_sq,
                &free_indices,
                &state.f_node,
                &state.j_diag,
            );
        }
    }

    let final_state = evaluate_state(&pipes, &demands_vec, &pressures_sq, &free_indices);

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

fn evaluate_state(
    pipes: &[IndexedPipe],
    demands_vec: &[f64],
    pressures_sq: &[f64],
    free_indices: &[usize],
) -> IterationState {
    let n = pressures_sq.len();
    let mut f_node = demands_vec.to_vec();
    let mut j_diag = vec![0.0_f64; n];
    let mut flows = vec![0.0_f64; pipes.len()];
    let mut conductances = vec![0.0_f64; pipes.len()];

    for (pipe_idx, pipe) in pipes.iter().enumerate() {
        let dp_sq = pressures_sq[pipe.from_idx] - pressures_sq[pipe.to_idx];
        let abs_dp = dp_sq.abs().max(MIN_ABS_DP);
        let q = dp_sq.signum() * (abs_dp / pipe.resistance).sqrt();
        let g = 1.0 / (2.0 * (pipe.resistance * abs_dp).sqrt());

        f_node[pipe.from_idx] -= q;
        f_node[pipe.to_idx] += q;
        j_diag[pipe.from_idx] += g;
        j_diag[pipe.to_idx] += g;

        flows[pipe_idx] = q;
        conductances[pipe_idx] = g;
    }

    let residual = free_indices
        .iter()
        .map(|&idx| f_node[idx].abs())
        .fold(0.0, f64::max);

    IterationState {
        f_node,
        j_diag,
        flows,
        conductances,
        residual,
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
