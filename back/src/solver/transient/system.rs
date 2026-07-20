use crate::graph::Pipe;
use crate::solver::gas_properties::{
    DEFAULT_GAS_TEMPERATURE_K, GasComposition, STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K,
};
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
///
/// Corde (quasi-Newton) cohérente avec `P₁² − P₂² = R Q|Q|` et `Δπ ≈ 2 P_ref ΔP` :
/// `ΔP = R Q² / (2 P_ref)` ⇒ `G = Q/ΔP = 2 P_ref / (R |Q|)`, régularisée pour `Q → 0` :
/// `G = 2 P_ref / (R · √(Q_prev² + ε²))`.
///
/// `q_prev` est le débit **lagged** (pas de l'itération courante) : couplage quasi-Newton
/// sur la corde `P²`. Le facteur **2** dans `2 P_ref` est intentionnel : il récupère le
/// régime permanent `ΔP = R Q² / (2 P_ref)` et donne `G ΔP = Q` à débit fixé.
pub(crate) fn segment_conductance(
    pipe: &Pipe,
    pressure_bar: f64,
    segment_length_km: f64,
    composition: &GasComposition,
    q_prev: f64,
) -> f64 {
    let resistance = pipe_resistance_at_pressure_with_composition(
        segment_length_km,
        pipe.diameter_mm,
        pipe.roughness_mm,
        pressure_bar.max(1.0),
        DEFAULT_GAS_TEMPERATURE_K,
        *composition,
        q_prev,
    )
    .max(1e-18);
    const EPS_FLOW: f64 = 1e-3;
    let p_ref = pressure_bar.max(1.0);
    let q_reg = (q_prev * q_prev + EPS_FLOW * EPS_FLOW).sqrt();
    (2.0 * p_ref / (resistance * q_reg)).max(1e-18)
}

/// Assemble le pas implicite Euler pour la continuité isotherme.
///
/// Par cellule i (Q en Nm³/s) :
/// $A\,\Delta x\,(\partial\rho/\partial P)\,dP/dt + \rho_n\,\partial Q/\partial x = 0$,
/// avec capacité $C = A\,\Delta x\,(\partial\rho/\partial P)/\rho_n$ [Nm³/bar]
/// et couplage quasi-stationnaire $Q = G(P_{i-1}-P_i)$.
///
/// Les conductances `G` sont évaluées avec `state.flows[i]` (débit **lagged** au pas
/// précédent), pas le $Q$ implicite du pas courant : schéma quasi-Newton sur la corde.
pub(crate) fn build_tridiagonal_step(
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

    if n == 0 {
        return TridiagonalStep {
            lower,
            diag,
            upper,
            rhs,
        };
    }

    let temperature_k = DEFAULT_GAS_TEMPERATURE_K;
    let inv_storage = |p: f64| {
        let capacitance = storage_capacitance_nm3_per_bar(
            composition,
            p,
            temperature_k,
            mesh.area_m2,
            mesh.dx,
        );
        if capacitance > 1e-18 {
            1.0 / capacitance
        } else {
            1e18
        }
    };

    // BC amont : pression Dirichlet sur le bord (pas une cellule).
    // Cellule 0 : C₀ dP₀/dt = G₀(P_source − P₀) − G₁(P₀ − P₁).
    let p0 = state.pressures[0];
    let inv_c0 = inv_storage(p0);
    let g0 = segment_conductance(
        pipe,
        0.5 * (source.pressure_bar + p0),
        mesh.dx * 1e-3,
        composition,
        state.flows[0],
    );
    let alpha0 = dt_s * inv_c0 * g0;

    if n == 1 {
        diag[0] = 1.0 + alpha0;
        rhs[0] = p0 + alpha0 * source.pressure_bar + dt_s * inv_c0 * sink.flow_m3s;
        return TridiagonalStep {
            lower,
            diag,
            upper,
            rhs,
        };
    }

    let g1 = segment_conductance(
        pipe,
        0.5 * (p0 + state.pressures[1]),
        mesh.dx * 1e-3,
        composition,
        state.flows[1],
    );
    let alpha1 = dt_s * inv_c0 * g1;
    diag[0] = 1.0 + alpha0 + alpha1;
    upper[0] = -alpha1;
    rhs[0] = p0 + alpha0 * source.pressure_bar;

    for i in 1..n - 1 {
        let p_i = state.pressures[i];
        let inv_c_i = inv_storage(p_i);
        let g_left = segment_conductance(
            pipe,
            0.5 * (state.pressures[i - 1] + p_i),
            mesh.dx * 1e-3,
            composition,
            state.flows[i],
        );
        let g_right = segment_conductance(
            pipe,
            0.5 * (p_i + state.pressures[i + 1]),
            mesh.dx * 1e-3,
            composition,
            state.flows[i + 1],
        );
        let alpha_left = dt_s * inv_c_i * g_left;
        let alpha_right = dt_s * inv_c_i * g_right;
        lower[i - 1] = -alpha_left;
        upper[i] = -alpha_right;
        diag[i] = 1.0 + alpha_left + alpha_right;
        rhs[i] = p_i;
    }

    // Dernière cellule : Q_out = −sink.flow_m3s (fixe).
    let i = n - 1;
    let p_i = state.pressures[i];
    let inv_c_i = inv_storage(p_i);
    let g_left = segment_conductance(
        pipe,
        0.5 * (state.pressures[i - 1] + p_i),
        mesh.dx * 1e-3,
        composition,
        state.flows[i],
    );
    let alpha_left = dt_s * inv_c_i * g_left;
    lower[i - 1] = -alpha_left;
    diag[i] = 1.0 + alpha_left;
    rhs[i] = p_i + dt_s * inv_c_i * sink.flow_m3s;

    TridiagonalStep {
        lower,
        diag,
        upper,
        rhs,
    }
}

/// Résout le système tridiagonal (Thomas).
pub(crate) fn solve_tridiagonal(step: &TridiagonalStep) -> Vec<f64> {
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

/// Capacité de stockage isotherme d'une cellule [Nm³/bar].
///
/// $C = A \Delta x\, (\partial\rho/\partial P) / \rho_n$ avec $\rho_n$ à conditions normales.
pub(crate) fn storage_capacitance_nm3_per_bar(
    composition: &GasComposition,
    pressure_bar: f64,
    temperature_k: f64,
    area_m2: f64,
    dx_m: f64,
) -> f64 {
    let drho_dp = density_derivative_kg_per_m3_bar(composition, pressure_bar, temperature_k);
    let rho_n = composition
        .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
        .max(1e-6);
    area_m2 * dx_m * drho_dp / rho_n
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
pub(crate) fn update_flows(
    mesh: &PipeMesh,
    state: &mut TransientPipeState,
    pipe: &Pipe,
    source: &SourceBoundary,
    sink: &SinkBoundary,
    composition: &GasComposition,
) {
    let n = mesh.n_cells;
    if n == 0 {
        return;
    }

    let q_prev = state.flows[0];
    let g0 = segment_conductance(
        pipe,
        0.5 * (source.pressure_bar + state.pressures[0]),
        mesh.dx * 1e-3,
        composition,
        q_prev,
    );
    state.flows[0] = g0 * (source.pressure_bar - state.pressures[0]);

    for i in 1..n {
        let q_prev = state.flows[i];
        let g = segment_conductance(
            pipe,
            0.5 * (state.pressures[i - 1] + state.pressures[i]),
            mesh.dx * 1e-3,
            composition,
            q_prev,
        );
        state.flows[i] = g * (state.pressures[i - 1] - state.pressures[i]);
    }
    state.flows[n] = -sink.flow_m3s;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_wrong_storage_capacitance_kg_per_bar(
        composition: &GasComposition,
        pressure_bar: f64,
        temperature_k: f64,
        area_m2: f64,
        dx_m: f64,
    ) -> f64 {
        let rho = composition.density_kg_per_m3(pressure_bar, temperature_k);
        let drho_dp = density_derivative_kg_per_m3_bar(composition, pressure_bar, temperature_k);
        (rho + pressure_bar * drho_dp) * area_m2 * dx_m
    }

    #[test]
    fn storage_capacitance_matches_nm3_per_bar_formula() {
        // Référence indépendante : gaz idéal isotherme C ≈ (A·dx / ρ_n) · (ρ/P).
        let composition = GasComposition::pure_ch4();
        let p_bar = 70.0;
        let area = 0.5;
        let dx = 1000.0;
        let c = storage_capacitance_nm3_per_bar(
            &composition,
            p_bar,
            DEFAULT_GAS_TEMPERATURE_K,
            area,
            dx,
        );
        let rho = composition.density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
        let rho_n = composition
            .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
            .max(1e-6);
        let c_ideal = area * dx * (rho / p_bar) / rho_n;
        assert!(
            (c - c_ideal).abs() / c_ideal < 0.20,
            "C={c} should be within 20% of ideal-gas C={c_ideal} for CH4 at 70 bar"
        );
        assert!(c > 0.0 && c.is_finite());
    }

    #[test]
    fn storage_capacitance_differs_from_legacy_wrong_formula() {
        let composition = GasComposition::pure_ch4();
        let p_bar = 70.0;
        let area = 0.5;
        let dx = 1000.0;
        let c_correct = storage_capacitance_nm3_per_bar(
            &composition,
            p_bar,
            DEFAULT_GAS_TEMPERATURE_K,
            area,
            dx,
        );
        let c_wrong = legacy_wrong_storage_capacitance_kg_per_bar(
            &composition,
            p_bar,
            DEFAULT_GAS_TEMPERATURE_K,
            area,
            dx,
        );
        assert!(
            (c_correct / c_wrong - 1.0).abs() > 0.5,
            "legacy and corrected capacitance should differ materially"
        );
        let inv_ratio = (1.0 / c_correct) / (1.0 / c_wrong);
        assert!(
            inv_ratio > 10.0,
            "inv_c_new / inv_c_old should be ~O(100), got {inv_ratio}"
        );
    }

    #[test]
    fn storage_capacitance_ideal_gas_derivative_check() {
        let composition = GasComposition::pure_ch4();
        let p_bar = 70.0;
        let area = 0.5;
        let dx = 1000.0;
        let rho = composition.density_kg_per_m3(p_bar, DEFAULT_GAS_TEMPERATURE_K);
        let drho_dp =
            density_derivative_kg_per_m3_bar(&composition, p_bar, DEFAULT_GAS_TEMPERATURE_K);
        let drho_dp_ideal = rho / p_bar;
        assert!(
            (drho_dp - drho_dp_ideal).abs() / drho_dp_ideal < 0.2,
            "drho/dP should be close to rho/P for CH4 at 70 bar"
        );
        let c = storage_capacitance_nm3_per_bar(
            &composition,
            p_bar,
            DEFAULT_GAS_TEMPERATURE_K,
            area,
            dx,
        );
        let rho_n = composition
            .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
            .max(1e-6);
        let expected = area * dx * drho_dp / rho_n;
        assert!((c - expected).abs() / expected < 0.05);
    }

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

    fn sample_pipe() -> Pipe {
        Pipe {
            id: "P1".into(),
            from: "SRC".into(),
            to: "SK".into(),
            kind: crate::graph::ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: crate::graph::EquipmentSpec::default(),
        }
    }

    #[test]
    fn segment_conductance_matches_p_squared_chord() {
        let pipe = sample_pipe();
        let composition = GasComposition::pure_ch4();
        let q = 1.0;
        let p_ref = 65.0;
        let segment_km = 10.0 / 12.0;
        let resistance = pipe_resistance_at_pressure_with_composition(
            segment_km,
            pipe.diameter_mm,
            pipe.roughness_mm,
            p_ref,
            DEFAULT_GAS_TEMPERATURE_K,
            composition,
            q,
        );
        let g = segment_conductance(&pipe, p_ref, segment_km, &composition, q);
        let g_expected = 2.0 * p_ref / (resistance * q.abs());
        assert!(
            (g - g_expected).abs() / g_expected < 0.01,
            "G={g} should match 2*P_ref/(R|Q|)={g_expected} within 1%"
        );
    }

    #[test]
    fn segment_conductance_chord_recovers_flow() {
        let pipe = sample_pipe();
        let composition = GasComposition::pure_ch4();
        let q = 2.5;
        let p_ref = 65.0;
        let segment_km = 10.0 / 12.0;
        let resistance = pipe_resistance_at_pressure_with_composition(
            segment_km,
            pipe.diameter_mm,
            pipe.roughness_mm,
            p_ref,
            DEFAULT_GAS_TEMPERATURE_K,
            composition,
            q,
        );
        let g = segment_conductance(&pipe, p_ref, segment_km, &composition, q);
        let delta_p = resistance * q * q / (2.0 * p_ref);
        let q_from_chord = g * delta_p;
        assert!(
            (q_from_chord - q.abs()).abs() / q.abs() < 0.01,
            "G*ΔP should recover |Q|: got {q_from_chord}, expected {}",
            q.abs()
        );
    }

    #[test]
    fn segment_conductance_finite_at_zero_flow() {
        let pipe = sample_pipe();
        let composition = GasComposition::pure_ch4();
        let p_ref = 65.0;
        let segment_km = 10.0 / 12.0;
        let resistance = pipe_resistance_at_pressure_with_composition(
            segment_km,
            pipe.diameter_mm,
            pipe.roughness_mm,
            p_ref,
            DEFAULT_GAS_TEMPERATURE_K,
            composition,
            0.0,
        )
        .max(1e-18);
        const EPS_FLOW: f64 = 1e-3;
        let g = segment_conductance(&pipe, p_ref, segment_km, &composition, 0.0);
        let g_reg = 2.0 * p_ref / (resistance * EPS_FLOW);
        assert!(g.is_finite() && g > 0.0, "G at Q=0 must be finite and positive, got {g}");
        assert!(
            (g - g_reg).abs() / g_reg < 0.01,
            "G(Q=0) should match regularized chord 2P/(R·ε): got {g}, expected {g_reg}"
        );
        assert!(
            g < g_reg * 1.01,
            "G(Q=0) must stay bounded by regularization, got {g} vs cap {g_reg}"
        );
    }

    #[test]
    fn source_bc_raises_first_cell_pressure() {
        let pipe = sample_pipe();
        let mesh = PipeMesh::from_pipe(&pipe, Some(4));
        let composition = GasComposition::pure_ch4();
        let state = TransientPipeState::from_endpoint_pressures(&mesh, 70.0, 60.0, 5.0);
        let sink = SinkBoundary::fixed_flow(-5.0);

        let step_lo = build_tridiagonal_step(
            &mesh,
            &state,
            &pipe,
            60.0,
            &SourceBoundary::fixed_pressure(70.0),
            &sink,
            &composition,
        );
        assert!(
            step_lo.diag[0] > 1.0,
            "cell 0 must be storage-coupled, not Dirichlet (diag[0]={})",
            step_lo.diag[0]
        );
        assert!(
            step_lo.rhs[0] > state.pressures[0],
            "source pressure must appear in cell-0 RHS"
        );

        let p_lo = solve_tridiagonal(&step_lo);
        let step_hi = build_tridiagonal_step(
            &mesh,
            &state,
            &pipe,
            60.0,
            &SourceBoundary::fixed_pressure(80.0),
            &sink,
            &composition,
        );
        let p_hi = solve_tridiagonal(&step_hi);

        assert!(
            p_hi[0] > p_lo[0] + 0.1,
            "raising P_source must raise first cell: lo={} hi={}",
            p_lo[0],
            p_hi[0]
        );
        assert!(
            p_hi[mesh.n_cells - 1] > p_lo[mesh.n_cells - 1] + 0.05,
            "raising P_source must raise sink cell: lo={} hi={}",
            p_lo[mesh.n_cells - 1],
            p_hi[mesh.n_cells - 1]
        );
    }

    /// T3 (validation.md) : dM/dP (linepack maillé) ≈ ρ_n Σ C_i (capacitance PDE).
    #[test]
    fn test_linepack_capacitance_cross_module() {
        let pipe = sample_pipe();
        let mesh = PipeMesh::from_pipe(&pipe, Some(12));
        let composition = GasComposition::pure_ch4();
        let rho_n = composition
            .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
            .max(1e-6);

        for p_bar in [70.0, 30.0] {
            let dp = 0.01;
            let state_lo =
                TransientPipeState::uniform_pressure(&mesh, p_bar - dp, 0.0);
            let state_hi =
                TransientPipeState::uniform_pressure(&mesh, p_bar + dp, 0.0);
            let lp_lo = state_lo.linepack_kg(&mesh, &composition, DEFAULT_GAS_TEMPERATURE_K);
            let lp_hi = state_hi.linepack_kg(&mesh, &composition, DEFAULT_GAS_TEMPERATURE_K);
            let dm_dp_fd = (lp_hi - lp_lo) / (2.0 * dp);

            let capacitance_sum_nm3_per_bar: f64 = (0..mesh.n_cells)
                .map(|_| {
                    storage_capacitance_nm3_per_bar(
                        &composition,
                        p_bar,
                        DEFAULT_GAS_TEMPERATURE_K,
                        mesh.area_m2,
                        mesh.dx,
                    )
                })
                .sum();
            let dm_dp_cap = rho_n * capacitance_sum_nm3_per_bar;

            let rel_err = (dm_dp_fd - dm_dp_cap).abs() / dm_dp_cap.max(1e-18);
            eprintln!(
                "linepack↔capacitance P={p_bar} bar: dM/dP_fd={dm_dp_fd:.6e}, dM/dP_cap={dm_dp_cap:.6e}, rel_err={rel_err:.4}"
            );
            assert!(
                rel_err < 0.05,
                "P={p_bar} bar: |dM/dP_FD − ρ_n ΣC|/ρ_n ΣC = {rel_err:.4} (threshold 0.05)"
            );
        }
    }

    #[test]
    fn build_tridiagonal_step_single_cell_formula() {
        let pipe = sample_pipe();
        let composition = GasComposition::pure_ch4();
        let mesh = PipeMesh {
            n_cells: 1,
            dx: pipe.length_km * 1e3,
            diameter_m: pipe.diameter_mm * 1e-3,
            length_m: pipe.length_km * 1e3,
            area_m2: std::f64::consts::PI * (pipe.diameter_mm * 1e-3).powi(2) / 4.0,
        };
        let p0 = 65.0;
        let q_prev = 5.0;
        let state = TransientPipeState::uniform_pressure(&mesh, p0, q_prev);
        let source = SourceBoundary::fixed_pressure(70.0);
        let sink = SinkBoundary::fixed_flow(-5.0);
        let dt_s = 60.0;

        let step = build_tridiagonal_step(
            &mesh,
            &state,
            &pipe,
            dt_s,
            &source,
            &sink,
            &composition,
        );

        let inv_c0 = 1.0
            / storage_capacitance_nm3_per_bar(
                &composition,
                p0,
                DEFAULT_GAS_TEMPERATURE_K,
                mesh.area_m2,
                mesh.dx,
            );
        let g0 = segment_conductance(
            &pipe,
            0.5 * (source.pressure_bar + p0),
            mesh.dx * 1e-3,
            &composition,
            q_prev,
        );
        let alpha0 = dt_s * inv_c0 * g0;

        assert!((step.diag[0] - (1.0 + alpha0)).abs() < 1e-12);
        let rhs_expected = p0 + alpha0 * source.pressure_bar + dt_s * inv_c0 * sink.flow_m3s;
        assert!(
            (step.rhs[0] - rhs_expected).abs() < 1e-12,
            "rhs[0]={} expected={rhs_expected}",
            step.rhs[0]
        );

        let p_new = solve_tridiagonal(&step)[0];
        let c0 = 1.0 / inv_c0;
        let dp = p_new - p0;
        let q_in = g0 * (source.pressure_bar - p_new);
        let q_out = -sink.flow_m3s;
        let lhs = c0 * dp;
        let rhs = dt_s * (q_in - q_out);
        let rel_err = (lhs - rhs).abs() / rhs.abs().max(1e-12);
        assert!(
            rel_err < 0.05,
            "n=1 mass balance: C·ΔP={lhs:.4e} vs dt·(Qin−Qout)={rhs:.4e}, rel_err={rel_err:.4}"
        );
    }

    #[test]
    fn sink_bc_sign_affects_last_cell_rhs() {
        let pipe = sample_pipe();
        let mesh = PipeMesh::from_pipe(&pipe, Some(4));
        let composition = GasComposition::pure_ch4();
        let state = TransientPipeState::from_endpoint_pressures(&mesh, 70.0, 60.0, 5.0);
        let source = SourceBoundary::fixed_pressure(70.0);
        let dt_s = 60.0;
        let p_last = state.pressures[mesh.n_cells - 1];

        let step_withdraw = build_tridiagonal_step(
            &mesh,
            &state,
            &pipe,
            dt_s,
            &source,
            &SinkBoundary::fixed_flow(-5.0),
            &composition,
        );
        assert!(
            step_withdraw.rhs[mesh.n_cells - 1] < p_last,
            "negative sink (withdrawal) should lower last-cell RHS"
        );

        let step_inject = build_tridiagonal_step(
            &mesh,
            &state,
            &pipe,
            dt_s,
            &source,
            &SinkBoundary::fixed_flow(5.0),
            &composition,
        );
        assert!(
            step_inject.rhs[mesh.n_cells - 1] > p_last,
            "positive sink (injection) should raise last-cell RHS"
        );
    }
}
