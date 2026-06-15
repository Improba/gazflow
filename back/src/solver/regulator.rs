//! Boucle externe régulateurs / détendeurs (P8.5, P8.8).

use std::collections::HashMap;

use serde::Serialize;

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

/// $g$ pour le seuil gravitaire actif/bypass [m/s²].
const REGULATOR_GRAVITY_M_S2: f64 = 9.80665;

/// Mode opérationnel d'un organe à consigne aval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegulatorMode {
    /// Imposé P_aval = consigne (nœud aval traité comme slack).
    Active,
    /// Amont insuffisant : liaison quasi transparente (bypass).
    Bypass,
}

const MAX_REGULATOR_OUTER_ITERS: usize = 12;

#[derive(Debug, Clone)]
pub(crate) struct RegulatorEdge {
    pub pipe_id: String,
    pub from_id: String,
    pub to_id: String,
    pub setpoint_bar: f64,
    pub delta_p_min_bar: f64,
}

/// État des organes après convergence (exposé API / résultats).
#[derive(Debug, Clone, Serialize)]
pub struct EquipmentState {
    pub pipe_id: String,
    pub kind: ConnectionKind,
    pub mode: RegulatorMode,
}

pub(crate) fn collect_regulator_edges(network: &GasNetwork) -> Vec<RegulatorEdge> {
    network
        .pipes()
        .filter_map(regulator_edge_from_pipe)
        .collect()
}

fn regulator_edge_from_pipe(pipe: &Pipe) -> Option<RegulatorEdge> {
    if !pipe.is_open {
        return None;
    }
    let setpoint = pipe.equipment.effective_setpoint_bar()?;
    if !setpoint.is_finite() || setpoint <= 0.0 {
        return None;
    }
    let delta = pipe
        .equipment
        .regulator_delta_p_min_bar
        .unwrap_or(0.5)
        .max(0.0);
    if !delta.is_finite() {
        return None;
    }
    match pipe.kind {
        ConnectionKind::PressureRegulator | ConnectionKind::DeliveryStation => {
            Some(RegulatorEdge {
                pipe_id: pipe.id.clone(),
                from_id: pipe.from.clone(),
                to_id: pipe.to.clone(),
                setpoint_bar: setpoint,
                delta_p_min_bar: delta,
            })
        }
        _ => None,
    }
}

pub(crate) fn has_regulator_edges(network: &GasNetwork) -> bool {
    !collect_regulator_edges(network).is_empty()
}

/// Densité de référence pour le seuil gravitaire actif/bypass [kg/m³].
pub(crate) const REGULATOR_THRESHOLD_RHO_KG_M3: f64 = 50.0;

fn node_height_m(network: &GasNetwork, id: &str) -> f64 {
    network
        .nodes()
        .find(|n| n.id == id)
        .map(|n| n.height_m)
        .unwrap_or(0.0)
}

/// Pression amont minimale pour régulation : consigne + ΔP_ouverture + ρgΔz (aval en surélévation).
///
/// En descente ($z_{\text{aval}} < z_{\text{amont}}$), le terme hydrostatique est négatif :
/// $P_{\text{requis}}$ peut être inférieur à $P_{\text{consigne}}$ (pressions manométriques aux nœuds).
pub(crate) fn required_upstream_pressure_bar(
    setpoint_bar: f64,
    delta_p_min_bar: f64,
    height_from_m: f64,
    height_to_m: f64,
    density_kg_per_m3: f64,
) -> f64 {
    let hydro_bar =
        density_kg_per_m3 * REGULATOR_GRAVITY_M_S2 * (height_to_m - height_from_m) / 1e5;
    (setpoint_bar + delta_p_min_bar + hydro_bar).max(1e-6)
}

/// Détermine le mode avec hystérésis légère (P8.8).
///
/// Seuil d'activation : $P_{\text{amont}} \ge P_{\text{requis}}$ avec
/// $P_{\text{requis}} = P_{\text{consigne}} + \Delta P_{\min} + \rho g (z_{\text{aval}} - z_{\text{amont}})$.
/// Seuil de désactivation (depuis actif) : $P_{\text{amont}} < P_{\text{requis}} - 0{,}05\,\Delta P_{\min}$.
pub(crate) fn regulator_mode_with_hysteresis(
    p_upstream_bar: f64,
    required_upstream_bar: f64,
    delta_p_min_bar: f64,
    previous: Option<RegulatorMode>,
) -> RegulatorMode {
    let on_threshold = required_upstream_bar;
    let off_threshold = required_upstream_bar - delta_p_min_bar * 0.05;
    match previous {
        Some(RegulatorMode::Active) if p_upstream_bar < off_threshold => RegulatorMode::Bypass,
        Some(RegulatorMode::Bypass) if p_upstream_bar < on_threshold => RegulatorMode::Bypass,
        Some(RegulatorMode::Active) => RegulatorMode::Active,
        _ if p_upstream_bar >= on_threshold => RegulatorMode::Active,
        _ => RegulatorMode::Bypass,
    }
}

/// Clone le réseau en imposant les slacks aval pour régulateurs actifs.
pub(crate) fn network_for_regulator_modes(
    base: &GasNetwork,
    modes: &HashMap<String, RegulatorMode>,
) -> GasNetwork {
    let mut net = base.clone();
    for reg in collect_regulator_edges(base) {
        let mode = modes
            .get(&reg.pipe_id)
            .copied()
            // Ne pas imposer de slack si le mode n'est pas connu (prudent).
            .unwrap_or(RegulatorMode::Bypass);
        if mode != RegulatorMode::Active {
            continue;
        }
        if let Some(node) = net.node_mut(&reg.to_id) {
            if node.pressure_fixed_bar.is_none() {
                node.pressure_fixed_bar = Some(reg.setpoint_bar);
            }
        }
    }
    net
}

pub(crate) fn all_bypass_modes(network: &GasNetwork) -> HashMap<String, RegulatorMode> {
    collect_regulator_edges(network)
        .into_iter()
        .map(|reg| (reg.pipe_id, RegulatorMode::Bypass))
        .collect()
}

pub(crate) fn modes_from_bypass_reference(
    network: &GasNetwork,
    reference_pressures_bar: &HashMap<String, f64>,
    previous: Option<&HashMap<String, RegulatorMode>>,
) -> HashMap<String, RegulatorMode> {
    let empty = HashMap::new();
    let prev = previous.unwrap_or(&empty);
    update_regulator_modes(network, reference_pressures_bar, prev)
}

pub(crate) fn update_regulator_modes(
    network: &GasNetwork,
    pressures_bar: &HashMap<String, f64>,
    previous: &HashMap<String, RegulatorMode>,
) -> HashMap<String, RegulatorMode> {
    let mut modes = HashMap::new();
    for reg in collect_regulator_edges(network) {
        let h_from = node_height_m(network, &reg.from_id);
        let h_to = node_height_m(network, &reg.to_id);
        let p_required = required_upstream_pressure_bar(
            reg.setpoint_bar,
            reg.delta_p_min_bar,
            h_from,
            h_to,
            REGULATOR_THRESHOLD_RHO_KG_M3,
        );
        let p_up = pressures_bar
            .get(&reg.from_id)
            .copied()
            // Pression amont issue du réseau « tout bypass » (voir `reference_upstream_pressures`).
            .unwrap_or(p_required + 1.0);
        modes.insert(
            reg.pipe_id.clone(),
            regulator_mode_with_hysteresis(
                p_up,
                p_required,
                reg.delta_p_min_bar,
                previous.get(&reg.pipe_id).copied(),
            ),
        );
    }
    modes
}

pub(crate) fn equipment_states_from_modes(
    network: &GasNetwork,
    modes: &HashMap<String, RegulatorMode>,
) -> Vec<EquipmentState> {
    network
        .pipes()
        .filter_map(|pipe| {
            let mode = modes.get(&pipe.id).copied()?;
            Some(EquipmentState {
                pipe_id: pipe.id.clone(),
                kind: pipe.kind,
                mode,
            })
        })
        .collect()
}

pub(crate) fn delivery_pressure_warnings(
    network: &GasNetwork,
    pressures_bar: &HashMap<String, f64>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for pipe in network.pipes() {
        if pipe.kind != ConnectionKind::DeliveryStation {
            continue;
        }
        let Some(min_p) = pipe.equipment.delivery_min_pressure_bar else {
            continue;
        };
        if let Some(setpoint) = pipe.equipment.regulator_setpoint_bar {
            if setpoint + 1e-6 < min_p {
                warnings.push(format!(
                    "poste {} : consigne {setpoint:.3} bar < minimum contractuel {min_p:.3} bar",
                    pipe.id
                ));
            }
        }
        let Some(&p) = pressures_bar.get(&pipe.to) else {
            continue;
        };
        if p + 1e-6 < min_p {
            warnings.push(format!(
                "poste {} : P_aval={p:.3} bar < minimum contractuel {min_p:.3} bar",
                pipe.id
            ));
        }
    }
    warnings
}

/// Avertissements si l'état convergé contredit la logique actif/bypass.
pub(crate) fn regulator_consistency_warnings(
    network: &GasNetwork,
    modes: &HashMap<String, RegulatorMode>,
    bypass_pressures_bar: &HashMap<String, f64>,
    final_pressures_bar: &HashMap<String, f64>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for reg in collect_regulator_edges(network) {
        let Some(&mode) = modes.get(&reg.pipe_id) else {
            continue;
        };
        let p_up_bypass = bypass_pressures_bar
            .get(&reg.from_id)
            .copied()
            .unwrap_or(f64::NAN);
        let h_from = node_height_m(network, &reg.from_id);
        let h_to = node_height_m(network, &reg.to_id);
        let on_threshold = required_upstream_pressure_bar(
            reg.setpoint_bar,
            reg.delta_p_min_bar,
            h_from,
            h_to,
            REGULATOR_THRESHOLD_RHO_KG_M3,
        );
        match mode {
            RegulatorMode::Active => {
                if let Some(&p_down) = final_pressures_bar.get(&reg.to_id) {
                    if (p_down - reg.setpoint_bar).abs() > 0.2 {
                        warnings.push(format!(
                            "régulateur {} actif mais P_aval={p_down:.3} bar ≠ consigne {:.3} bar",
                            reg.pipe_id, reg.setpoint_bar
                        ));
                    }
                }
                if p_up_bypass.is_finite() && p_up_bypass + 1e-6 < on_threshold {
                    warnings.push(format!(
                        "régulateur {} actif alors que P_amont (réf. bypass)={p_up_bypass:.3} < seuil {on_threshold:.3} bar",
                        reg.pipe_id
                    ));
                }
            }
            RegulatorMode::Bypass => {
                if let (Some(&p_up), Some(&p_down)) = (
                    final_pressures_bar.get(&reg.from_id),
                    final_pressures_bar.get(&reg.to_id),
                ) {
                    let hydro_bar =
                        REGULATOR_THRESHOLD_RHO_KG_M3 * REGULATOR_GRAVITY_M_S2 * (h_to - h_from)
                            / 1e5;
                    // Liaison quasi sans perte : écart nodal attendu ≈ |ρgΔz| + marge friction.
                    let expected_drop = hydro_bar.abs() + reg.delta_p_min_bar + 0.25;
                    let drop = (p_up - p_down).abs();
                    if drop > expected_drop {
                        warnings.push(format!(
                            "régulateur {} en bypass mais |ΔP|={drop:.3} bar > attendu ~{expected_drop:.3} bar",
                            reg.pipe_id
                        ));
                    }
                }
            }
        }
    }
    warnings
}

pub(crate) const MAX_REGULATOR_OUTER: usize = MAX_REGULATOR_OUTER_ITERS;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::equipment::EquipmentSpec;
    use crate::graph::{Node, Pipe};

    fn regulator_network(setpoint: f64) -> GasNetwork {
        let mut net = GasNetwork::new();
        for (id, p_fix) in [("HP", Some(70.0)), ("MP", None), ("SK", None)] {
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
            equipment: EquipmentSpec::pressure_regulator(setpoint, 0.5),
        });
        net
    }

    #[test]
    fn test_regulator_mode_active_when_upstream_sufficient() {
        let required = required_upstream_pressure_bar(20.0, 0.5, 0.0, 0.0, 50.0);
        assert_eq!(
            regulator_mode_with_hysteresis(25.0, required, 0.5, None),
            RegulatorMode::Active
        );
    }

    #[test]
    fn test_regulator_mode_bypass_when_upstream_low() {
        let required = required_upstream_pressure_bar(20.0, 0.5, 0.0, 0.0, 50.0);
        assert_eq!(
            regulator_mode_with_hysteresis(19.0, required, 0.5, None),
            RegulatorMode::Bypass
        );
    }

    #[test]
    fn test_regulator_gravity_downhill_lowers_required_upstream() {
        let flat = required_upstream_pressure_bar(20.0, 0.5, 0.0, 0.0, 50.0);
        let downhill = required_upstream_pressure_bar(20.0, 0.5, 200.0, 0.0, 50.0);
        assert!(downhill < flat);
        assert_eq!(
            regulator_mode_with_hysteresis(20.3, downhill, 0.5, None),
            RegulatorMode::Active
        );
    }

    #[test]
    fn test_invalid_setpoint_regulator_excluded() {
        let mut net = regulator_network(20.0);
        for pipe in net.graph.edge_weights_mut() {
            if pipe.id == "REG" {
                pipe.equipment.regulator_setpoint_bar = Some(-1.0);
            }
        }
        assert!(collect_regulator_edges(&net).is_empty());
    }

    #[test]
    fn test_regulator_gravity_uphill_raises_required_upstream() {
        // Δz = 200 m → ρgΔz ≈ 50·9.81·200/1e5 ≈ 0.98 bar
        let flat = required_upstream_pressure_bar(20.0, 0.5, 0.0, 0.0, 50.0);
        let uphill = required_upstream_pressure_bar(20.0, 0.5, 0.0, 200.0, 50.0);
        assert!(uphill > flat + 0.9);
        // P_amont = 21 bar : actif à plat, bypass en montée (seuil ~21.5)
        assert_eq!(
            regulator_mode_with_hysteresis(21.0, flat, 0.5, None),
            RegulatorMode::Active
        );
        assert_eq!(
            regulator_mode_with_hysteresis(21.0, uphill, 0.5, None),
            RegulatorMode::Bypass
        );
    }

    #[test]
    fn test_closed_regulator_excluded_from_edges() {
        let mut net = regulator_network(20.0);
        for pipe in net.graph.edge_weights_mut() {
            if pipe.id == "REG" {
                pipe.is_open = false;
            }
        }
        assert!(collect_regulator_edges(&net).is_empty());
    }

    #[test]
    fn test_regulator_imposes_downstream_slack_when_active() {
        let net = regulator_network(20.0);
        let mut modes = HashMap::new();
        modes.insert("REG".into(), RegulatorMode::Active);
        let adjusted = network_for_regulator_modes(&net, &modes);
        let sk = adjusted.nodes().find(|n| n.id == "SK").unwrap();
        assert_eq!(sk.pressure_fixed_bar, Some(20.0));
    }

    #[test]
    fn test_regulator_hysteresis_prevents_chatter() {
        let sp = 20.0;
        let dp = 0.5;
        let required = required_upstream_pressure_bar(sp, dp, 0.0, 0.0, 50.0);
        // Zone d'hystérésis : off = required − 0,05·ΔP, on = required
        let active =
            regulator_mode_with_hysteresis(20.48, required, dp, Some(RegulatorMode::Active));
        assert_eq!(active, RegulatorMode::Active);
        let still_active = regulator_mode_with_hysteresis(20.48, required, dp, Some(active));
        assert_eq!(still_active, RegulatorMode::Active);
        let bypass =
            regulator_mode_with_hysteresis(20.47, required, dp, Some(RegulatorMode::Active));
        assert_eq!(bypass, RegulatorMode::Bypass);
        let stays_bypass = regulator_mode_with_hysteresis(20.49, required, dp, Some(bypass));
        assert_eq!(stays_bypass, RegulatorMode::Bypass);
    }

    #[test]
    fn test_delivery_min_is_not_regulator_setpoint() {
        let spec = EquipmentSpec::delivery_station(4.5, 4.0, 0.3);
        assert_eq!(spec.effective_setpoint_bar(), Some(4.5));
    }
}
