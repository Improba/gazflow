use crate::graph::Pipe;
use crate::solver::gas_properties::{DEFAULT_GAS_TEMPERATURE_K, GasComposition};
use crate::solver::steady_state::pipe_resistance_at_pressure_with_composition;

use super::boundary::{SinkBoundary, SourceBoundary};
use super::mesh::PipeMesh;
use super::state::TransientPipeState;

/// Système tridiagonal implicite pour un pas de temps (Thomas algorithm).
#[derive(Debug, Clone)]
pub struct TridiagonalStep {
    pub lower: Vec<f64>,
    pub diag: Vec<f64>,
    pub upper: Vec<f64>,
    pub rhs: Vec<f64>,
}

/// Conductance linéarisée [Nm³/(s·bar)] entre deux cellules adjacentes.
pub fn segment_conductance(
    pipe: &Pipe,
    pressure_bar: f64,
    segment_length_km: f64,
    composition: &GasComposition,
) -> f64 {
    let resistance = pipe_resistance_at_pressure_with_composition(
        segment_length_km,
        pipe.diameter_mm,
        pipe.roughness_mm,
        pressure_bar.max(1.0),
        DEFAULT_GAS_TEMPERATURE_K,
        *composition,
        0.0,
    )
    .max(1e-18);
    // Linéarisation de Q = sign*sqrt(Δπ/R) autour de P_ref : G ≈ 1/(2 P_ref R).
    let p_ref = pressure_bar.max(1.0);
    (1.0 / (2.0 * p_ref * resistance)).max(1e-18)
}

/// Assemble le pas implicite Euler pour la continuité isotherme avec inventaire ρ(P)AL.
///
/// Par cellule i : ∂(ρAL)/∂t + ∂Q/∂x = 0, couplage quasi-stationnaire Q = G(P_{i-1}-P_i).
pub fn build_tridiagonal_step(
    mesh: &PipeMesh,
    state: &TransientPipeState,
    pipe: &Pipe,
    dt_s: f64,
    source: &SourceBoundary,
    sink: &SinkBoundary,
    composition: &GasComposition,
) -> TridiagonalStep {
    let n = mesh.n_cells;
    let mut lower = vec![0.0; n.saturating_sub(1)];
    let mut diag = vec![0.0; n];
    let mut upper = vec![0.0; n.saturating_sub(1)];
    let mut rhs = vec![0.0; n];

    let temperature_k = DEFAULT_GAS_TEMPERATURE_K;
    let inv_storage = |p: f64| {
        let rho = composition.density_kg_per_m3(p, temperature_k);
        let drho_dp = density_derivative_kg_per_m3_bar(composition, p, temperature_k);
        let capacitance = (rho + p * drho_dp) * mesh.area_m2 * mesh.dx;
        if capacitance > 1e-18 {
            1.0 / capacitance
        } else {
            1e18
        }
    };

    // Dirichlet amont : P_0 = P_source.
    diag[0] = 1.0;
    if n > 1 {
        upper[0] = 0.0;
    }
    rhs[0] = source.pressure_bar;

    if n == 1 {
        return TridiagonalStep {
            lower,
            diag,
            upper,
            rhs,
        };
    }

    let mean_p = |i: usize| {
        if i == 0 {
            source.pressure_bar
        } else {
            state.pressures[i - 1]
        }
    };

    for i in 1..n {
        let p_i = state.pressures[i];
        let inv_c_i = inv_storage(p_i);
        let g_left = segment_conductance(
            pipe,
            0.5 * (mean_p(i) + p_i),
            mesh.dx * 1e-3,
            composition,
        );
        let is_last = i == n - 1;
        let g_right = if is_last {
            segment_conductance(
                pipe,
                0.5 * (p_i + state.pressures[i.saturating_sub(0)]),
                mesh.dx * 1e-3,
                composition,
            )
        } else {
            segment_conductance(
                pipe,
                0.5 * (p_i + state.pressures[i + 1]),
                mesh.dx * 1e-3,
                composition,
            )
        };

        let alpha_left = dt_s * inv_c_i * g_left;
        let alpha_right = dt_s * inv_c_i * g_right;

        if is_last {
            // Aval : Q_sortant = Q_sink fixe.
            if i > 1 {
                lower[i - 1] = -alpha_left;
            }
            diag[i] = 1.0 + alpha_left;
            rhs[i] = p_i + dt_s * inv_c_i * sink.flow_m3s;
        } else {
            if i > 1 {
                lower[i - 1] = -alpha_left;
            }
            upper[i] = -alpha_right;
            diag[i] = 1.0 + alpha_left + alpha_right;
            rhs[i] = p_i;
        }
    }

    TridiagonalStep {
        lower,
        diag,
        upper,
        rhs,
    }
}

/// Résout le système tridiagonal (Thomas).
pub fn solve_tridiagonal(step: &TridiagonalStep) -> Vec<f64> {
    let n = step.diag.len();
    if n == 0 {
        return Vec::new();
    }
    let mut c_prime = vec![0.0; n.saturating_sub(1)];
    let mut d_prime = vec![0.0; n];
    let mut x = vec![0.0; n];

    d_prime[0] = step.rhs[0] / step.diag[0];
    if n > 1 {
        c_prime[0] = step.upper[0] / step.diag[0];
    }

    for i in 1..n {
        let denom = step.diag[i] - step.lower[i - 1] * c_prime[i - 1];
        let denom = if denom.abs() < 1e-30 {
            1e-30
        } else {
            denom
        };
        if i < n - 1 {
            c_prime[i] = step.upper[i] / denom;
        }
        d_prime[i] = (step.rhs[i] - step.lower[i - 1] * d_prime[i - 1]) / denom;
    }

    x[n - 1] = d_prime[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = d_prime[i] - c_prime[i] * x[i + 1];
    }
    x
}

fn density_derivative_kg_per_m3_bar(
    composition: &GasComposition,
    pressure_bar: f64,
    temperature_k: f64,
) -> f64 {
    let eps = 1e-4;
    let p_lo = (pressure_bar - eps).max(0.0);
    let p_hi = pressure_bar + eps;
    let rho_lo = composition.density_kg_per_m3(p_lo, temperature_k);
    let rho_hi = composition.density_kg_per_m3(p_hi, temperature_k);
    (rho_hi - rho_lo) / (p_hi - p_lo).max(eps)
}

/// Met à jour les débits aux interfaces après résolution des pressions.
pub fn update_flows(
    mesh: &PipeMesh,
    state: &mut TransientPipeState,
    pipe: &Pipe,
    source: &SourceBoundary,
    sink: &SinkBoundary,
    composition: &GasComposition,
) {
    let n = mesh.n_cells;
    state.flows[0] = if n > 1 {
        let g = segment_conductance(
            pipe,
            0.5 * (source.pressure_bar + state.pressures[0]),
            mesh.dx * 1e-3,
            composition,
        );
        g * (source.pressure_bar - state.pressures[0])
    } else {
        sink.flow_m3s
    };

    for i in 1..n {
        let g = segment_conductance(
            pipe,
            0.5 * (state.pressures[i - 1] + state.pressures[i]),
            mesh.dx * 1e-3,
            composition,
        );
        state.flows[i] = g * (state.pressures[i - 1] - state.pressures[i]);
    }
    state.flows[n] = sink.flow_m3s;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tridiagonal_identity_solve() {
        let step = TridiagonalStep {
            lower: vec![0.0, 0.0],
            diag: vec![2.0, 3.0, 4.0],
            upper: vec![0.0, 0.0],
            rhs: vec![4.0, 9.0, 16.0],
        };
        let x = solve_tridiagonal(&step);
        assert!((x[0] - 2.0).abs() < 1e-12);
        assert!((x[1] - 3.0).abs() < 1e-12);
        assert!((x[2] - 4.0).abs() < 1e-12);
    }
}
