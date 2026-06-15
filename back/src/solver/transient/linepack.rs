use std::collections::HashMap;

use crate::graph::{ConnectionKind, GasNetwork};
use crate::solver::gas_properties::{DEFAULT_GAS_TEMPERATURE_K, GasComposition};

/// Masse de gaz emmagasinée [kg] : $M = \sum_{\text{conduites actives}} \rho(P_{\mathrm{moy}})\, A\, L$.
///
/// $\rho$ via Papay + composition à $T = 288{,}15$ K (réseau isotherme). Conduites
/// fermées ou vannes à 0 % sont exclues.
pub fn compute_linepack(
    network: &GasNetwork,
    pressures_bar: &HashMap<String, f64>,
    composition: &GasComposition,
) -> f64 {
    network
        .pipes()
        .filter(|pipe| pipe.hydraulically_active())
        .map(|pipe| linepack_mass_kg(pipe, pressures_bar, composition))
        .sum()
}

fn linepack_mass_kg(
    pipe: &crate::graph::Pipe,
    pressures_bar: &HashMap<String, f64>,
    composition: &GasComposition,
) -> f64 {
    let Some(&p_from) = pressures_bar.get(&pipe.from) else {
        return 0.0;
    };
    let Some(&p_to) = pressures_bar.get(&pipe.to) else {
        return 0.0;
    };
    if !p_from.is_finite() || !p_to.is_finite() {
        return 0.0;
    }

    let diameter_m = pipe.diameter_mm * 1e-3;
    let length_m = pipe.length_km * 1e3;
    if !diameter_m.is_finite() || !length_m.is_finite() || diameter_m <= 0.0 || length_m <= 0.0 {
        return 0.0;
    }

    let area_m2 = std::f64::consts::PI * diameter_m * diameter_m / 4.0;
    let avg_pressure_bar = ((p_from + p_to) * 0.5).max(0.0);
    let rho = composition.density_kg_per_m3(avg_pressure_bar, DEFAULT_GAS_TEMPERATURE_K);
    if !rho.is_finite() || rho <= 0.0 {
        return 0.0;
    }
    rho * area_m2 * length_m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EquipmentSpec, Node, Pipe};

    fn simple_network(pipe_open: bool) -> (GasNetwork, HashMap<String, f64>) {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "SRC".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(70.0),
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "SK".into(),
            x: 1.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "SRC".into(),
            to: "SK".into(),
            kind: ConnectionKind::Pipe,
            is_open: pipe_open,
            length_km: 10.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        let pressures = HashMap::from([("SRC".to_string(), 70.0), ("SK".to_string(), 50.0)]);
        (net, pressures)
    }

    #[test]
    fn linepack_positive_for_active_pipe() {
        let (net, pressures) = simple_network(true);
        let lp = compute_linepack(&net, &pressures, &GasComposition::default());
        assert!(lp.is_finite() && lp > 0.0);
    }

    #[test]
    fn linepack_scales_with_mean_pressure() {
        let (net, mut pressures) = simple_network(true);
        let comp = GasComposition::default();
        let lp_low = compute_linepack(&net, &pressures, &comp);
        pressures.insert("SK".to_string(), 65.0);
        let lp_high = compute_linepack(&net, &pressures, &comp);
        assert!(lp_high > lp_low * 1.05, "linepack croît avec P_moy (isotherme)");
    }

    #[test]
    fn linepack_zero_for_closed_pipe() {
        let (net, pressures) = simple_network(false);
        let lp = compute_linepack(&net, &pressures, &GasComposition::default());
        assert!(lp.abs() < 1e-12);
    }
}
