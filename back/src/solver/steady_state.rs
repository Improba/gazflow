use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

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
pub(crate) fn pipe_resistance(length_km: f64, diameter_mm: f64, roughness_mm: f64) -> f64 {
    let d = diameter_mm * 1e-3; // m
    let l = length_km * 1e3; // m
    let f = darcy_friction(roughness_mm, diameter_mm, 1e7);
    let area = std::f64::consts::PI * d * d / 4.0;

    // Facteur de densité effectif (gaz naturel ~50 kg/m³ à 70 bar, 15°C)
    // Conversion Pa² → bar² : diviser par 1e10
    let rho_eff = 50.0;
    f * l * rho_eff / (2.0 * d * area * area * 1e10)
}

pub(crate) fn effective_pipe_resistance(pipe: &Pipe) -> f64 {
    match pipe.kind {
        ConnectionKind::Pipe => {
            pipe_resistance(pipe.length_km, pipe.diameter_mm, pipe.roughness_mm)
        }
        ConnectionKind::Valve | ConnectionKind::ShortPipe => {
            // Valve ouverte / shortPipe -> liaison quasi transparente.
            pipe_resistance(
                pipe.length_km.min(0.001),
                pipe.diameter_mm.max(1000.0),
                pipe.roughness_mm,
            )
        }
        ConnectionKind::CompressorStation => {
            // MVP: compresseur ignoré (approximation), traité comme liaison quasi-passante.
            pipe_resistance(
                pipe.length_km.min(0.001),
                pipe.diameter_mm.max(1000.0),
                pipe.roughness_mm,
            )
        }
    }
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
    solve_steady_state_with_initial_pressures(network, demands, None, max_iter, tolerance)
}

pub fn solve_steady_state_with_initial_pressures(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    initial_pressures_bar: Option<&HashMap<String, f64>>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
    let compressor_count = network
        .pipes()
        .filter(|p| p.kind == ConnectionKind::CompressorStation)
        .count();
    if compressor_count > 0 {
        tracing::warn!(
            compressor_count,
            "Compressor stations are currently approximated as near-zero-loss pipes"
        );
    }

    super::newton::solve_steady_state_newton_hybrid(
        network,
        demands,
        initial_pressures_bar,
        max_iter,
        tolerance,
    )
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
/// - Pas de compresseurs.
pub fn solve_steady_state_jacobi(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    max_iter: usize,
    tolerance: f64,
) -> Result<SolverResult> {
    let n = network.node_count();
    let mut pressures_sq: Vec<f64> = vec![70.0_f64.powi(2); n];

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
            "Compressor stations are currently approximated as near-zero-loss pipes"
        );
    }

    let pipes: Vec<_> = network.pipes().collect();
    let mut iterations = 0;
    let mut residual = f64::MAX;
    let relax = 0.8;

    for iter in 0..max_iter {
        // F_i : résidu nodal (bilan de masse)
        let mut f_node = vec![0.0_f64; n];
        // J_ii positif : somme des conductances linéarisées connectées au nœud i
        let mut j_diag = vec![0.0_f64; n];

        for (id, &d) in demands {
            if let Some(&i) = id_pos.get(id) {
                f_node[i] += d;
            }
        }

        for pipe in &pipes {
            let Some(&a) = id_pos.get(&pipe.from) else {
                continue;
            };
            let Some(&b) = id_pos.get(&pipe.to) else {
                continue;
            };

            let k = effective_pipe_resistance(pipe);
            let dp_sq = pressures_sq[a] - pressures_sq[b];
            let abs_dp = dp_sq.abs().max(1e-10);
            let sign = dp_sq.signum();
            let q = sign * (abs_dp / k).sqrt();

            // Conductance linéarisée : dQ/dπ = 1 / (2·√(K·|Δπ|))
            let g = 1.0 / (2.0 * (k * abs_dp).sqrt());

            // Q > 0 → flow from a to b
            // Node a perd Q (outflow), node b gagne Q (inflow)
            f_node[a] -= q;
            f_node[b] += q;

            // ∂F_i/∂π_i = −g pour chaque pipe connectée (toujours négatif)
            // On accumule g dans j_diag pour l'utiliser comme dénominateur positif
            j_diag[a] += g;
            j_diag[b] += g;
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

    for pipe in &pipes {
        let a = id_pos[&pipe.from];
        let b = id_pos[&pipe.to];
        let k = effective_pipe_resistance(pipe);
        let dp_sq = pressures_sq[a] - pressures_sq[b];
        let sign = dp_sq.signum();
        let q = sign * (dp_sq.abs().max(1e-10) / k).sqrt();
        result_flows.insert(pipe.id.clone(), q);
    }

    Ok(SolverResult {
        pressures: result_pressures,
        flows: result_flows,
        iterations,
        residual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::gaslib::{load_network, load_scenario_demands};
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
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
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
            length_km: 50.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
        });
        net.add_pipe(Pipe {
            id: "JA".into(),
            from: "J".into(),
            to: "A".into(),
            kind: ConnectionKind::Pipe,
            length_km: 30.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
        });
        net.add_pipe(Pipe {
            id: "JB".into(),
            from: "J".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            length_km: 40.0,
            diameter_mm: 400.0,
            roughness_mm: 0.012,
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
            // Même si la géométrie est "lourde", le solveur doit approximer
            // valve/compressor comme liaison quasi-passante.
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
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
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
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

        let result = solve_steady_state(&network, &scenario.demands, 800, 1e-4)
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

        let result = solve_steady_state(&net, &demands, 500, 1e-6).expect("solver should converge");
        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];
        let dp = (p_source - p_sink).abs();

        assert!(
            dp < 0.5,
            "open valve should introduce near-zero pressure loss, got ΔP={dp} bar"
        );
    }

    #[test]
    fn test_compressor_ignored_with_warning_behavior() {
        let net = near_lossless_link_network(ConnectionKind::CompressorStation);
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -20.0);

        let result = solve_steady_state(&net, &demands, 500, 1e-6).expect("solver should converge");
        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];
        let dp = (p_source - p_sink).abs();

        assert!(
            dp < 0.5,
            "compressor approximation should be near-lossless in MVP, got ΔP={dp} bar"
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

        let max_iter = 30;
        let result =
            solve_steady_state(&net, &demands, max_iter, 1e-6).expect("solver should not panic");

        assert!(result.residual.is_finite(), "residual should stay finite");
        assert_eq!(
            result.iterations, max_iter,
            "unsatisfied isolated demand should prevent convergence and use full iterations"
        );
    }
}
