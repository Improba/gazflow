use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use crate::graph::GasNetwork;

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
fn darcy_friction(roughness_mm: f64, diameter_mm: f64, reynolds: f64) -> f64 {
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
fn pipe_resistance(length_km: f64, diameter_mm: f64, roughness_mm: f64) -> f64 {
    let d = diameter_mm * 1e-3; // m
    let l = length_km * 1e3; // m
    let f = darcy_friction(roughness_mm, diameter_mm, 1e7);
    let area = std::f64::consts::PI * d * d / 4.0;

    // Facteur de densité effectif (gaz naturel ~50 kg/m³ à 70 bar, 15°C)
    // Conversion Pa² → bar² : diviser par 1e10
    let rho_eff = 50.0;
    f * l * rho_eff / (2.0 * d * area * area * 1e10)
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
pub fn solve_steady_state(
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
            let Some(&a) = id_pos.get(&pipe.from) else { continue };
            let Some(&b) = id_pos.get(&pipe.to) else { continue };

            let k = pipe_resistance(pipe.length_km, pipe.diameter_mm, pipe.roughness_mm);
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
        let k = pipe_resistance(pipe.length_km, pipe.diameter_mm, pipe.roughness_mm);
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
    use crate::graph::{GasNetwork, Node, Pipe};

    fn two_node_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "source".into(),
            x: 0.0, y: 0.0,
            lon: Some(10.0), lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "sink".into(),
            x: 1.0, y: 0.0,
            lon: Some(11.0), lat: Some(50.0),
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "pipe1".into(),
            from: "source".into(),
            to: "sink".into(),
            length_km: 100.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
        });
        net
    }

    fn y_network() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "S".into(), x: 0.0, y: 0.0,
            lon: Some(10.0), lat: Some(50.0), height_m: 0.0,
            pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
        });
        net.add_node(Node {
            id: "J".into(), x: 1.0, y: 0.0,
            lon: Some(10.5), lat: Some(50.5), height_m: 0.0,
            pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_node(Node {
            id: "A".into(), x: 2.0, y: 1.0,
            lon: Some(11.0), lat: Some(51.0), height_m: 0.0,
            pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_node(Node {
            id: "B".into(), x: 2.0, y: -1.0,
            lon: Some(11.0), lat: Some(49.0), height_m: 0.0,
            pressure_lower_bar: None, pressure_upper_bar: None,
            pressure_fixed_bar: None,
        });
        net.add_pipe(Pipe {
            id: "SJ".into(), from: "S".into(), to: "J".into(),
            length_km: 50.0, diameter_mm: 600.0, roughness_mm: 0.012,
        });
        net.add_pipe(Pipe {
            id: "JA".into(), from: "J".into(), to: "A".into(),
            length_km: 30.0, diameter_mm: 400.0, roughness_mm: 0.012,
        });
        net.add_pipe(Pipe {
            id: "JB".into(), from: "J".into(), to: "B".into(),
            length_km: 40.0, diameter_mm: 400.0, roughness_mm: 0.012,
        });
        net
    }

    #[test]
    fn steady_state_two_nodes() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("sink".to_string(), -10.0);

        let result = solve_steady_state(&net, &demands, 500, 1e-6)
            .expect("solver should converge");

        let p_source = result.pressures["source"];
        let p_sink = result.pressures["sink"];
        eprintln!("source={p_source:.4} bar, sink={p_sink:.4} bar, iter={}, res={:.2e}",
            result.iterations, result.residual);

        assert!((p_source - 70.0).abs() < 0.1, "source pressure should be ~70 bar, got {p_source}");
        assert!(p_sink < p_source, "sink pressure ({p_sink}) should be < source ({p_source})");
        assert!(p_sink > 0.0, "sink pressure should be positive, got {p_sink}");
    }

    #[test]
    fn steady_state_y_network_mass_conservation() {
        let net = y_network();
        let mut demands = HashMap::new();
        demands.insert("A".to_string(), -5.0);
        demands.insert("B".to_string(), -5.0);

        let result = solve_steady_state(&net, &demands, 500, 1e-6)
            .expect("solver should converge");

        let q_sj = result.flows["SJ"];
        let q_ja = result.flows["JA"];
        let q_jb = result.flows["JB"];

        eprintln!("Q_SJ={q_sj:.4}, Q_JA={q_ja:.4}, Q_JB={q_jb:.4}");
        eprintln!("Pressions: S={:.2}, J={:.2}, A={:.2}, B={:.2}",
            result.pressures["S"], result.pressures["J"],
            result.pressures["A"], result.pressures["B"]);

        // Conservation de masse à J : Q_SJ = Q_JA + Q_JB
        let balance = q_sj - q_ja - q_jb;
        assert!(balance.abs() < 1e-4,
            "mass conservation at J: {q_sj} != {q_ja} + {q_jb} (diff={balance})");

        // Toutes les pressions sont décroissantes depuis la source
        assert!(result.pressures["S"] > result.pressures["J"]);
        assert!(result.pressures["J"] > result.pressures["A"]);
        assert!(result.pressures["J"] > result.pressures["B"]);
    }

    #[test]
    fn darcy_friction_turbulent() {
        let f = darcy_friction(0.012, 500.0, 1e7);
        assert!(f > 0.005 && f < 0.05, "friction factor in realistic range: {f}");
    }

    #[test]
    fn pipe_resistance_positive() {
        let k = pipe_resistance(100.0, 500.0, 0.012);
        assert!(k > 0.0, "resistance must be positive: {k}");
        assert!(k.is_finite(), "resistance must be finite: {k}");
    }
}
