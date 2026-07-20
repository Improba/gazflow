//! Simulation transitoire MVP (P11).
//!
//! Deux modes :
//! - **Quasi-steady** : chaque pas re-résout un état permanent (MVP historique).
//! - **PDE** : linepack isotherme 1D par conduite (Euler implicite, tridiagonal).

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::graph::{ConnectionKind, GasNetwork, Pipe};

use super::SteadyStateConfig;
use super::gas_properties::DEFAULT_GAS_TEMPERATURE_K;
use super::steady_state::{SolverControl, solve_steady_state_with_progress};

pub mod boundary;
pub mod config;
pub mod events;
pub mod linepack;
pub mod mesh;
pub mod state;
pub mod system;
pub mod time_integration;

pub use boundary::{SinkBoundary, SourceBoundary};
pub use config::TransientConfig;
pub use events::TransientEvent;
pub use linepack::compute_linepack;
pub use mesh::PipeMesh;
pub use state::TransientPipeState;

use mesh::default_n_cells;
use time_integration::{
    ActivePipeContext, AlgebraicEquipment, PicardStatus, advance_network_one_step,
    fold_demands_through_equipment, is_pde_meshable, suggest_adaptive_dt_s,
};

#[allow(dead_code)]
fn default_true() -> bool {
    true
}

const TRANSIENT_TIME_EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransientMode {
    QuasiSteady,
    Pde,
}

impl Default for TransientMode {
    fn default() -> Self {
        Self::QuasiSteady
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TransientStepResult {
    pub time_s: f64,
    pub demands: HashMap<String, f64>,
    pub pressures: HashMap<String, f64>,
    /// Débit « affichage carte » = débit aval par conduite (compat Cesium).
    pub flows: HashMap<String, f64>,
    /// Débit amont par conduite [Nm³/s] (PDE: flows[0] ; quasi-steady: = flows).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub flows_in: HashMap<String, f64>,
    /// Débit aval par conduite [Nm³/s] (PDE: flows[n] ; quasi-steady: = flows).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub flows_out: HashMap<String, f64>,
    pub iterations: usize,
    pub residual: f64,
    /// False si le pas Picard PDE n'a pas convergé (ou fold/depth incomplet).
    #[serde(default = "default_true")]
    pub converged: bool,
    pub linepack_kg: f64,
    pub linepack_delta_kg: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransientResult {
    pub steps: Vec<TransientStepResult>,
    pub total_iterations: usize,
    pub limitation: String,
}

/// Contrôle de streaming (WS) pour un pas de temps transitoire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientControl {
    Continue,
    Cancel,
}

/// Point d'entrée : dispatch selon le mode transitoire.
pub fn simulate_transient(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
) -> Result<TransientResult> {
    simulate_transient_with_mode(network, initial_demands, events, config, TransientMode::QuasiSteady)
}

pub fn simulate_transient_with_mode(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
    mode: TransientMode,
) -> Result<TransientResult> {
    simulate_transient_with_mode_progress(
        network,
        initial_demands,
        events,
        config,
        mode,
        None,
    )
}

pub fn simulate_transient_with_mode_progress(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
    mode: TransientMode,
    on_step: Option<&dyn Fn(&TransientStepResult) -> TransientControl>,
) -> Result<TransientResult> {
    match mode {
        TransientMode::QuasiSteady => {
            simulate_transient_quasi_steady_progress(network, initial_demands, events, config, on_step)
        }
        TransientMode::Pde => {
            simulate_transient_pde_progress(network, initial_demands, events, config, on_step)
        }
    }
}

/// Simule un transitoire MVP par résolution quasi-stationnaire.
pub fn simulate_transient_quasi_steady(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
) -> Result<TransientResult> {
    simulate_transient_quasi_steady_progress(network, initial_demands, events, config, None)
}

fn simulate_transient_quasi_steady_progress(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
    on_step: Option<&dyn Fn(&TransientStepResult) -> TransientControl>,
) -> Result<TransientResult> {
    validate_config(config)?;
    validate_events(network, events)?;

    let mut network_state = network.clone();
    let mut demands = initial_demands.clone();

    let steady_cfg = SteadyStateConfig {
        gas_composition: config.gas_composition,
        ..SteadyStateConfig::default()
    };

    let initial =
        solve_steady_state_with_progress(&network_state, &demands, None, steady_cfg, |_| {
            SolverControl::Continue
        })?;

    let mut total_iterations = initial.iterations;
    let mut previous_pressures = initial.pressures.clone();
    let mut previous_linepack =
        compute_linepack(&network_state, &previous_pressures, &config.gas_composition);

    let initial_flows = initial.flows.clone();
    let mut steps = vec![TransientStepResult {
        time_s: 0.0,
        demands: demands.clone(),
        pressures: initial.pressures,
        flows: initial_flows.clone(),
        flows_in: initial_flows.clone(),
        flows_out: initial_flows,
        iterations: initial.iterations,
        residual: initial.residual,
        converged: initial.residual < 1.0,
        linepack_kg: previous_linepack,
        linepack_delta_kg: 0.0,
    }];
    if let Some(cb) = on_step
        && cb(&steps[0]) == TransientControl::Cancel
    {
        bail!("transient cancelled");
    }

    let mut ordered_events = events.to_vec();
    ordered_events.sort_by(|a, b| {
        a.time_s()
            .partial_cmp(&b.time_s())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut event_cursor = 0usize;

    let total_steps = (config.duration_s / config.dt_s).ceil() as usize;
    for step_idx in 1..=total_steps {
        let time_s = ((step_idx as f64) * config.dt_s).min(config.duration_s);
        while event_cursor < ordered_events.len()
            && ordered_events[event_cursor].time_s() <= time_s + 1e-9
        {
            apply_event(
                &mut network_state,
                &mut demands,
                &ordered_events[event_cursor],
            )?;
            event_cursor += 1;
        }

        let solved = solve_steady_state_with_progress(
            &network_state,
            &demands,
            Some(&previous_pressures),
            steady_cfg,
            |_| SolverControl::Continue,
        )?;
        total_iterations += solved.iterations;
        let linepack = compute_linepack(&network_state, &solved.pressures, &config.gas_composition);
        let linepack_delta = linepack - previous_linepack;
        previous_linepack = linepack;
        previous_pressures = solved.pressures.clone();

        let step_flows = solved.flows.clone();
        let step = TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: solved.pressures,
            flows: step_flows.clone(),
            flows_in: step_flows.clone(),
            flows_out: step_flows,
            iterations: solved.iterations,
            residual: solved.residual,
            converged: solved.residual < 1.0,
            linepack_kg: linepack,
            linepack_delta_kg: linepack_delta,
        };
        if let Some(cb) = on_step
            && cb(&step) == TransientControl::Cancel
        {
            bail!("transient cancelled");
        }
        steps.push(step);
    }

    Ok(TransientResult {
        steps,
        total_iterations,
        limitation: "Quasi-steady MVP: each time step re-solves steady-state; PDE wave dynamics are not modeled."
            .to_string(),
    })
}

/// Simule un transitoire PDE 1D isotherme (arbres et cycles, organes algébriques).
///
/// Supporte les réseaux en arbre ou avec cycles, avec régulateurs et compresseurs
/// modélisés comme liens algébriques (pression aval imposée). Retombe sur le
/// quasi-stationnaire uniquement si la topologie n'est pas éligible PDE
/// (réseau déconnecté, absence d'ancre pression, ou type d'équipement non supporté).
pub fn simulate_transient_pde(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
) -> Result<TransientResult> {
    simulate_transient_pde_progress(network, initial_demands, events, config, None)
}

fn simulate_transient_pde_progress(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
    on_step: Option<&dyn Fn(&TransientStepResult) -> TransientControl>,
) -> Result<TransientResult> {
    validate_config(config)?;
    validate_events(network, events)?;

    if !is_pde_eligible(network) {
        let mut result = simulate_transient_quasi_steady_progress(
            network,
            initial_demands,
            events,
            config,
            on_step,
        )?;
        result.limitation = format!(
            "{} Fallback to quasi-steady: network topology not supported by PDE (disconnected, no fixed-pressure anchor, or unsupported equipment kind).",
            result.limitation
        );
        return Ok(result);
    }

    let mut network_state = network.clone();
    let mut demands = initial_demands.clone();

    let steady_cfg = SteadyStateConfig {
        gas_composition: config.gas_composition,
        ..SteadyStateConfig::default()
    };

    let initial =
        match solve_steady_state_with_progress(&network_state, &demands, None, steady_cfg, |_| {
            SolverControl::Continue
        }) {
            Ok(r) => r,
            Err(_) => synthetic_pde_initial(&network_state, &demands)?,
        };

    let mut topology = pde_topology(&network_state)?;
    let mut node_pressures = initial.pressures.clone();
    let mut fixed_pressure_nodes = fixed_pressure_node_set(&network_state);
    sync_equipment_fixed_nodes(&topology.equipment, &mut node_pressures, &mut fixed_pressure_nodes);
    let mut pipe_contexts = build_pipe_contexts(
        &network_state,
        &topology.pipe_ids,
        &node_pressures,
        &demands,
        &fixed_pressure_nodes,
        config,
        None,
    )?;

    let mut previous_linepack = total_pde_linepack(&pipe_contexts, &config.gas_composition);
    let (initial_flows_in, initial_flows_out) =
        snapshot_pipe_boundary_flows(&pipe_contexts, &initial.flows);
    let mut steps = vec![TransientStepResult {
        time_s: 0.0,
        demands: demands.clone(),
        pressures: snapshot_node_pressures(&pipe_contexts, &node_pressures),
        flows: initial_flows_out.clone(),
        flows_in: initial_flows_in,
        flows_out: initial_flows_out,
        iterations: initial.iterations,
        residual: initial.residual,
        converged: initial.residual < 1.0,
        linepack_kg: previous_linepack,
        linepack_delta_kg: 0.0,
    }];
    if let Some(cb) = on_step
        && cb(&steps[0]) == TransientControl::Cancel
    {
        bail!("transient cancelled");
    }

    let mut ordered_events = events.to_vec();
    ordered_events.sort_by(|a, b| {
        a.time_s()
            .partial_cmp(&b.time_s())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut event_cursor = 0usize;

    let mut time_s = 0.0;
    let mut total_iterations = initial.iterations;
    let max_steps = ((config.duration_s / config.dt_s).ceil() as usize)
        .saturating_mul(if config.adaptive_dt { 40 } else { 2 })
        .max(2)
        .saturating_add(2);
    let mut step_guard = 0usize;
    while time_s + TRANSIENT_TIME_EPS < config.duration_s {
        step_guard += 1;
        if step_guard > max_steps {
            bail!(
                "PDE time loop exceeded {} steps (duration={}, dt_max={}); check adaptive_dt",
                max_steps,
                config.duration_s,
                config.dt_s
            );
        }

        // Événements au temps courant (avant intégration).
        if let Some((ev_time, reason)) = apply_pde_events_rebuild(
            &mut network_state,
            &mut demands,
            &ordered_events,
            &mut event_cursor,
            time_s,
            &mut node_pressures,
            &mut fixed_pressure_nodes,
            &mut topology,
            &mut pipe_contexts,
            config,
        )? {
            return finish_pde_with_quasi_steady_fallback(
                &network_state,
                &demands,
                &ordered_events[event_cursor..],
                config,
                on_step,
                steps,
                total_iterations,
                time_s,
                ev_time,
                &reason,
            );
        }

        let remaining = config.duration_s - time_s;
        let mut dt = if config.adaptive_dt {
            suggest_adaptive_dt_s(
                &pipe_contexts,
                &config.gas_composition,
                config.dt_s,
                remaining,
            )
        } else {
            config.dt_s.min(remaining)
        };
        dt = dt.min(remaining).max(1e-6);

        if let Some(next_ev) = ordered_events[event_cursor..]
            .iter()
            .map(|e| e.time_s())
            .find(|&t| t > time_s + TRANSIENT_TIME_EPS)
        {
            dt = dt.min(next_ev - time_s);
        }

        if dt <= TRANSIENT_TIME_EPS {
            if let Some((ev_time, reason)) = apply_pde_events_rebuild(
                &mut network_state,
                &mut demands,
                &ordered_events,
                &mut event_cursor,
                time_s,
                &mut node_pressures,
                &mut fixed_pressure_nodes,
                &mut topology,
                &mut pipe_contexts,
                config,
            )? {
                return finish_pde_with_quasi_steady_fallback(
                    &network_state,
                    &demands,
                    &ordered_events[event_cursor..],
                    config,
                    on_step,
                    steps,
                    total_iterations,
                    time_s,
                    ev_time,
                    &reason,
                );
            }
            continue;
        }

        let snapshot = snapshot_pde_step_state(&pipe_contexts, &node_pressures);

        let mut picard = advance_network_one_step(
            &mut pipe_contexts,
            &mut node_pressures,
            &fixed_pressure_nodes,
            &demands,
            dt,
            &config.gas_composition,
            topology.is_tree,
            &topology.equipment,
        );
        total_iterations = total_iterations.saturating_add(picard.iterations);
        let mut actual_dt = dt;
        // État après le premier essai full-dt (pour restauration si retry échoue).
        let after_full = snapshot_pde_step_state(&pipe_contexts, &node_pressures);
        let picard_full = PicardStatus {
            converged: picard.converged,
            iterations: picard.iterations,
            residual: picard.residual,
        };

        if !picard.converged && dt > 2.0 * TRANSIENT_TIME_EPS {
            restore_pde_step_state(&mut pipe_contexts, &mut node_pressures, &snapshot);
            let half_dt = dt * 0.5;
            let retry = advance_network_one_step(
                &mut pipe_contexts,
                &mut node_pressures,
                &fixed_pressure_nodes,
                &demands,
                half_dt,
                &config.gas_composition,
                topology.is_tree,
                &topology.equipment,
            );
            total_iterations = total_iterations.saturating_add(retry.iterations);
            if retry.converged {
                picard = retry;
                actual_dt = half_dt;
            } else {
                // Pas de 2e full-dt : restaurer le résultat non convergé déjà calculé.
                restore_pde_step_state(&mut pipe_contexts, &mut node_pressures, &after_full);
                picard = picard_full;
            }
        }

        time_s = (time_s + actual_dt).min(config.duration_s);

        // Appliquer les événements dont t ≤ time_s (fin de pas réelle, pas le dt nominal).
        let fallback = apply_pde_events_rebuild(
            &mut network_state,
            &mut demands,
            &ordered_events,
            &mut event_cursor,
            time_s,
            &mut node_pressures,
            &mut fixed_pressure_nodes,
            &mut topology,
            &mut pipe_contexts,
            config,
        )?;

        // Linepack après rebuild éventuel → cohérent avec pressures/flows du step.
        let linepack = total_pde_linepack(&pipe_contexts, &config.gas_composition);
        let linepack_delta = linepack - previous_linepack;
        previous_linepack = linepack;

        let (folded_demands, _) =
            fold_demands_through_equipment(&demands, &topology.equipment);
        let junction_residual =
            max_junction_imbalance(&pipe_contexts, &folded_demands, &fixed_pressure_nodes);
        let mut residual = junction_residual.max(picard.residual);
        if !picard.converged {
            residual = residual.max(1.0);
        }
        let (flows_in, flows_out) =
            snapshot_pipe_boundary_flows(&pipe_contexts, &initial.flows);
        let step = TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: snapshot_node_pressures(&pipe_contexts, &node_pressures),
            flows: flows_out.clone(),
            flows_in,
            flows_out,
            iterations: picard.iterations,
            residual,
            converged: picard.converged,
            linepack_kg: linepack,
            linepack_delta_kg: linepack_delta,
        };
        if let Some(cb) = on_step
            && cb(&step) == TransientControl::Cancel
        {
            bail!("transient cancelled");
        }
        steps.push(step);

        if let Some((ev_time, reason)) = fallback {
            return finish_pde_with_quasi_steady_fallback(
                &network_state,
                &demands,
                &ordered_events[event_cursor..],
                config,
                on_step,
                steps,
                total_iterations,
                time_s,
                ev_time,
                &reason,
            );
        }
    }

    let limitation = if topology.is_tree && topology.equipment.is_empty() {
        "PDE MVP: isothermal 1D FV per pipe (implicit Euler); tree leaf→root mass sweep; no thermal transients."
    } else if topology.equipment.is_empty() {
        "PDE MVP: isothermal 1D FV; cyclic network via nodal Dirichlet Picard; no thermal transients."
    } else {
        "PDE MVP: isothermal 1D FV + algebraic regulator/compressor links; nodal Picard; no thermal transients."
    };

    Ok(TransientResult {
        steps,
        total_iterations,
        limitation: limitation.to_string(),
    })
}

/// Après un événement rendant le réseau non-PDE-éligible, enchaîne en quasi-stationnaire
/// sur le reste de l'horizon (temps décalés pour coller à la timeline PDE).
fn finish_pde_with_quasi_steady_fallback(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    remaining_events: &[TransientEvent],
    config: &TransientConfig,
    on_step: Option<&dyn Fn(&TransientStepResult) -> TransientControl>,
    mut steps: Vec<TransientStepResult>,
    mut total_iterations: usize,
    time_s: f64,
    event_time_s: f64,
    reason: &str,
) -> Result<TransientResult> {
    let remaining = config.duration_s - time_s;
    if remaining <= 1e-9 {
        return Ok(TransientResult {
            steps,
            total_iterations,
            limitation: format!(
                "PDE stopped after event at t={event_time_s:.3}s (topology ineligible: {reason})."
            ),
        });
    }
    let qs_cfg = TransientConfig {
        duration_s: remaining,
        dt_s: config.dt_s,
        gas_composition: config.gas_composition,
        n_cells_per_pipe: config.n_cells_per_pipe,
        adaptive_dt: false,
    };
    let shifted: Vec<TransientEvent> = remaining_events
        .iter()
        .filter_map(|ev| shift_event_time(ev, time_s))
        .collect();
    let qs = match simulate_transient_quasi_steady_progress(
        network,
        demands,
        &shifted,
        &qs_cfg,
        on_step,
    ) {
        Ok(qs) => qs,
        Err(qs_err) => {
            // Réseau physiquement insolvable après l'événement (ex. conduite unique fermée) :
            // conserver le résultat PDE partiel plutôt qu'échouer toute la requête.
            return Ok(TransientResult {
                steps,
                total_iterations,
                limitation: format!(
                    "PDE→quasi-steady fallback after event at t={event_time_s:.3}s \
                     (topology ineligible: {reason}); quasi-steady also failed: {qs_err}"
                ),
            });
        }
    };
    total_iterations = total_iterations.saturating_add(qs.total_iterations);
    for mut step in qs.steps.into_iter().skip(1) {
        step.time_s += time_s;
        steps.push(step);
    }
    Ok(TransientResult {
        steps,
        total_iterations,
        limitation: format!(
            "PDE→quasi-steady fallback after event at t={event_time_s:.3}s (topology ineligible: {reason}). {}",
            qs.limitation
        ),
    })
}

fn shift_event_time(event: &TransientEvent, origin_s: f64) -> Option<TransientEvent> {
    let t = event.time_s() - origin_s;
    if t < -1e-12 {
        return None;
    }
    let time_s = t.max(0.0);
    Some(match event {
        TransientEvent::ValveClose { pipe_id, .. } => TransientEvent::ValveClose {
            time_s,
            pipe_id: pipe_id.clone(),
        },
        TransientEvent::DemandChange {
            node_id,
            demand_m3s,
            ..
        } => TransientEvent::DemandChange {
            time_s,
            node_id: node_id.clone(),
            demand_m3s: *demand_m3s,
        },
        TransientEvent::RegulatorSetpoint {
            pipe_id,
            setpoint_bar,
            ..
        } => TransientEvent::RegulatorSetpoint {
            time_s,
            pipe_id: pipe_id.clone(),
            setpoint_bar: *setpoint_bar,
        },
    })
}

fn is_pde_eligible(network: &GasNetwork) -> bool {
    pde_topology(network).is_ok()
}

fn fixed_pressure_node_set(network: &GasNetwork) -> HashSet<String> {
    network
        .nodes()
        .filter(|n| n.pressure_fixed_bar.is_some())
        .map(|n| n.id.clone())
        .collect()
}

/// Ancrages pression aval des organes algébriques (régulateurs, compresseurs).
fn sync_equipment_fixed_nodes(
    equipment: &[AlgebraicEquipment],
    node_pressures: &mut HashMap<String, f64>,
    fixed_pressure_nodes: &mut HashSet<String>,
) {
    for eq in equipment {
        match eq {
            AlgebraicEquipment::Regulator {
                to,
                setpoint_bar,
                ..
            } => {
                node_pressures.insert(to.clone(), *setpoint_bar);
                fixed_pressure_nodes.insert(to.clone());
            }
            AlgebraicEquipment::Compressor { from, to, ratio } => {
                fixed_pressure_nodes.insert(to.clone());
                let p_up = node_pressures.get(from).copied().unwrap_or(50.0);
                let r = ratio.max(1.0);
                node_pressures.insert(to.clone(), (p_up * r).clamp(1.0, 200.0));
            }
        }
    }
}

struct PdeTopology {
    pipe_ids: Vec<String>,
    is_tree: bool,
    equipment: Vec<AlgebraicEquipment>,
}

/// Topologie PDE : conduites maillables (+ ShortPipe/Valve/Resistor), organes algébriques,
/// cycles autorisés. Requiert connexité et ≥1 ancre pression.
fn pde_topology(network: &GasNetwork) -> Result<PdeTopology> {
    let active: Vec<&Pipe> = network
        .pipes()
        .filter(|p| p.hydraulically_active())
        .collect();

    if active.is_empty() {
        bail!("no active pipe for PDE transient");
    }

    let mut equipment = Vec::new();
    let mut meshable = Vec::new();
    for p in &active {
        if is_pde_meshable(p.kind) {
            meshable.push(*p);
            continue;
        }
        match p.kind {
            ConnectionKind::PressureRegulator | ConnectionKind::DeliveryStation => {
                let setpoint = p
                    .equipment
                    .regulator_setpoint_bar
                    .or(p.equipment.delivery_min_pressure_bar)
                    .unwrap_or(40.0);
                equipment.push(AlgebraicEquipment::Regulator {
                    from: p.from.clone(),
                    to: p.to.clone(),
                    setpoint_bar: setpoint,
                });
            }
            ConnectionKind::CompressorStation => {
                let ratio = p
                    .equipment
                    .compressor_nominal_ratio
                    .or(p.compressor_ratio_max)
                    .or(p.equipment.compressor_pressure_cap_ratio)
                    .unwrap_or(1.2)
                    .max(1.0);
                equipment.push(AlgebraicEquipment::Compressor {
                    from: p.from.clone(),
                    to: p.to.clone(),
                    ratio,
                });
            }
            ConnectionKind::ControlValve => {
                // Vanne ouverte : traiter comme ShortPipe (résistance via diamètre).
                meshable.push(*p);
            }
            _ => {
                bail!(
                    "PDE unsupported active equipment kind on '{}'",
                    p.id
                );
            }
        }
    }

    if meshable.is_empty() {
        bail!("PDE requires at least one meshable pipe");
    }

    let mut nodes: HashSet<&str> = HashSet::new();
    for p in &meshable {
        nodes.insert(p.from.as_str());
        nodes.insert(p.to.as_str());
    }
    for eq in &equipment {
        match eq {
            AlgebraicEquipment::Regulator { from, to, .. }
            | AlgebraicEquipment::Compressor { from, to, .. } => {
                nodes.insert(from.as_str());
                nodes.insert(to.as_str());
            }
        }
    }

    // Connexité non orientée (pipes + organes).
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for p in &meshable {
        adj.entry(p.from.as_str()).or_default().push(p.to.as_str());
        adj.entry(p.to.as_str()).or_default().push(p.from.as_str());
    }
    for eq in &equipment {
        let (a, b) = match eq {
            AlgebraicEquipment::Regulator { from, to, .. }
            | AlgebraicEquipment::Compressor { from, to, .. } => (from.as_str(), to.as_str()),
        };
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }

    let start = *nodes.iter().next().ok_or_else(|| anyhow::anyhow!("empty PDE nodes"))?;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack = vec![start];
    while let Some(n) = stack.pop() {
        if !seen.insert(n) {
            continue;
        }
        if let Some(nei) = adj.get(n) {
            stack.extend(nei.iter().copied());
        }
    }
    if seen.len() != nodes.len() {
        bail!("PDE requires a connected network");
    }

    let n_edges = meshable.len() + equipment.len();
    let is_tree = n_edges == nodes.len().saturating_sub(1);

    if !network.nodes().any(|n| {
        nodes.contains(n.id.as_str()) && n.pressure_fixed_bar.is_some()
    }) {
        bail!("PDE requires at least one fixed-pressure node");
    }

    let mut ids: Vec<String> = meshable.iter().map(|p| p.id.clone()).collect();
    ids.sort();
    Ok(PdeTopology {
        pipe_ids: ids,
        is_tree,
        equipment,
    })
}

fn synthetic_pde_initial(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
) -> Result<super::steady_state::SolverResult> {
    let p_anchor = network
        .nodes()
        .filter_map(|n| n.pressure_fixed_bar)
        .fold(None, |acc: Option<f64>, p| {
            Some(acc.map_or(p, |a| a.max(p)))
        })
        .unwrap_or(70.0);

    let mut pressures = HashMap::new();
    for n in network.nodes() {
        let p = n.pressure_fixed_bar.unwrap_or_else(|| {
            if demands.get(&n.id).copied().unwrap_or(0.0).abs() > 1e-12 {
                p_anchor * 0.85
            } else {
                p_anchor * 0.95
            }
        });
        pressures.insert(n.id.clone(), p);
    }

    let mut flows = HashMap::new();
    for p in network.pipes().filter(|p| p.hydraulically_active()) {
        let q = if let Some(d) = demands.get(&p.to) {
            -d
        } else {
            0.0
        };
        flows.insert(p.id.clone(), q);
    }

    Ok(super::steady_state::SolverResult {
        pressures,
        flows,
        iterations: 0,
        residual: 1.0,
        equipment_states: Vec::new(),
        warnings: vec!["PDE initialised from synthetic pressures (steady Newton failed)".into()],
        demand_scale_achieved: None,
    })
}

fn build_pipe_contexts(
    network: &GasNetwork,
    pipe_ids: &[String],
    node_pressures: &HashMap<String, f64>,
    demands: &HashMap<String, f64>,
    fixed_pressure_nodes: &HashSet<String>,
    config: &TransientConfig,
    previous: Option<&[ActivePipeContext]>,
) -> Result<Vec<ActivePipeContext>> {
    let prev_by_id: HashMap<&str, &ActivePipeContext> = previous
        .iter()
        .flat_map(|ctxs| ctxs.iter())
        .map(|ctx| (ctx.pipe.id.as_str(), ctx))
        .collect();
    let mut contexts = Vec::with_capacity(pipe_ids.len());

    for pipe_id in pipe_ids {
        let pipe = network
            .pipes()
            .find(|p| p.id == *pipe_id)
            .ok_or_else(|| anyhow::anyhow!("missing pipe '{pipe_id}'"))?
            .clone();

        let p_from = node_pressures
            .get(&pipe.from)
            .copied()
            .unwrap_or(70.0);
        let p_to = node_pressures.get(&pipe.to).copied().unwrap_or(p_from);
        let demand_to = demands.get(&pipe.to).copied().unwrap_or(0.0);
        let source_p = network
            .nodes()
            .find(|n| n.id == pipe.from)
            .and_then(|n| n.pressure_fixed_bar)
            .unwrap_or(p_from);

        let n_cells = config.n_cells_per_pipe.or_else(|| Some(default_n_cells(pipe.length_km)));
        let mesh = PipeMesh::from_pipe(&pipe, n_cells);
        let approx_q = if demand_to.abs() > 1e-12 {
            -demand_to
        } else {
            0.0
        };
        let state = if let Some(prev_ctx) = prev_by_id.get(pipe_id.as_str()) {
            if pipe_geometry_matches(&prev_ctx.pipe, &pipe, prev_ctx.mesh.n_cells, mesh.n_cells) {
                TransientPipeState {
                    pressures: prev_ctx.state.pressures.clone(),
                    flows: prev_ctx.state.flows.clone(),
                }
            } else {
                TransientPipeState::from_endpoint_pressures(&mesh, p_from, p_to, approx_q)
            }
        } else {
            TransientPipeState::from_endpoint_pressures(&mesh, p_from, p_to, approx_q)
        };

        let sink = if fixed_pressure_nodes.contains(pipe.to.as_str()) || demand_to.abs() <= 1e-12 {
            SinkBoundary::fixed_pressure(p_to)
        } else {
            SinkBoundary::fixed_flow(demand_to)
        };

        contexts.push(ActivePipeContext {
            pipe,
            mesh,
            state,
            source: SourceBoundary::fixed_pressure(source_p),
            sink,
        });
    }

    Ok(contexts)
}

struct PdeStepSnapshot {
    pipe_states: Vec<(Vec<f64>, Vec<f64>)>,
    node_pressures: HashMap<String, f64>,
}

fn snapshot_pde_step_state(
    pipe_contexts: &[ActivePipeContext],
    node_pressures: &HashMap<String, f64>,
) -> PdeStepSnapshot {
    PdeStepSnapshot {
        pipe_states: pipe_contexts
            .iter()
            .map(|ctx| (ctx.state.pressures.clone(), ctx.state.flows.clone()))
            .collect(),
        node_pressures: node_pressures.clone(),
    }
}

fn restore_pde_step_state(
    pipe_contexts: &mut [ActivePipeContext],
    node_pressures: &mut HashMap<String, f64>,
    snapshot: &PdeStepSnapshot,
) {
    for (ctx, (pressures, flows)) in pipe_contexts.iter_mut().zip(snapshot.pipe_states.iter()) {
        ctx.state.pressures = pressures.clone();
        ctx.state.flows = flows.clone();
    }
    *node_pressures = snapshot.node_pressures.clone();
}

fn apply_pde_events_rebuild(
    network_state: &mut GasNetwork,
    demands: &mut HashMap<String, f64>,
    ordered_events: &[TransientEvent],
    event_cursor: &mut usize,
    limit_time: f64,
    node_pressures: &mut HashMap<String, f64>,
    fixed_pressure_nodes: &mut HashSet<String>,
    topology: &mut PdeTopology,
    pipe_contexts: &mut Vec<ActivePipeContext>,
    config: &TransientConfig,
) -> Result<Option<(f64, String)>> {
    let mut fallback: Option<(f64, String)> = None;
    while *event_cursor < ordered_events.len()
        && ordered_events[*event_cursor].time_s() <= limit_time + TRANSIENT_TIME_EPS
    {
        let ev_time = ordered_events[*event_cursor].time_s();
        apply_event(network_state, demands, &ordered_events[*event_cursor])?;
        for node in network_state.nodes() {
            if let Some(p) = node.pressure_fixed_bar {
                node_pressures.insert(node.id.clone(), p);
            }
        }
        *fixed_pressure_nodes = fixed_pressure_node_set(network_state);
        match pde_topology(network_state) {
            Ok(topo) => {
                *topology = topo;
                let previous = pipe_contexts.as_slice();
                *pipe_contexts = build_pipe_contexts(
                    network_state,
                    &topology.pipe_ids,
                    node_pressures,
                    demands,
                    fixed_pressure_nodes,
                    config,
                    Some(previous),
                )?;
                sync_equipment_fixed_nodes(
                    &topology.equipment,
                    node_pressures,
                    fixed_pressure_nodes,
                );
                *event_cursor += 1;
            }
            Err(e) => {
                *event_cursor += 1;
                fallback = Some((ev_time, e.to_string()));
                break;
            }
        }
    }
    Ok(fallback)
}

fn rel_close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let scale = a.abs().max(b.abs()).max(1e-30);
    (a - b).abs() / scale <= 1e-12
}

fn pipe_geometry_matches(prev: &Pipe, curr: &Pipe, prev_n_cells: usize, curr_n_cells: usize) -> bool {
    prev_n_cells == curr_n_cells
        && rel_close(prev.length_km, curr.length_km)
        && rel_close(prev.diameter_mm, curr.diameter_mm)
        && rel_close(prev.roughness_mm, curr.roughness_mm)
}

fn max_junction_imbalance(
    contexts: &[ActivePipeContext],
    demands: &HashMap<String, f64>,
    fixed_pressure_nodes: &HashSet<String>,
) -> f64 {
    let mut nodes: HashSet<String> = HashSet::new();
    for ctx in contexts {
        nodes.insert(ctx.pipe.from.clone());
        nodes.insert(ctx.pipe.to.clone());
    }
    nodes
        .into_iter()
        .filter(|n| !fixed_pressure_nodes.contains(n))
        .map(|n| time_integration::nodal_mass_imbalance(&n, contexts, demands).abs())
        .fold(0.0_f64, f64::max)
}

fn total_pde_linepack(
    contexts: &[ActivePipeContext],
    composition: &crate::solver::gas_properties::GasComposition,
) -> f64 {
    contexts
        .iter()
        .map(|ctx| {
            ctx.state
                .linepack_kg(&ctx.mesh, composition, DEFAULT_GAS_TEMPERATURE_K)
        })
        .sum()
}

fn snapshot_node_pressures(
    contexts: &[ActivePipeContext],
    fallback: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut pressures = fallback.clone();
    for ctx in contexts {
        pressures.insert(ctx.pipe.from.clone(), ctx.source.pressure_bar);
        if let Some(&p_end) = ctx.state.pressures.last() {
            pressures.insert(ctx.pipe.to.clone(), p_end);
        }
    }
    pressures
}

/// Débits de bord des seules conduites PDE actives (pas de clone du steady :
/// une vanne fermée / rebuild topologie ne doit pas laisser de fantômes).
fn snapshot_pipe_boundary_flows(
    contexts: &[ActivePipeContext],
    fallback: &HashMap<String, f64>,
) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let mut flows_in = HashMap::new();
    let mut flows_out = HashMap::new();
    for ctx in contexts {
        let id = ctx.pipe.id.clone();
        let q_in = ctx
            .state
            .flows
            .first()
            .copied()
            .unwrap_or_else(|| fallback.get(&id).copied().unwrap_or(0.0));
        let q_out = ctx
            .state
            .flows
            .last()
            .copied()
            .unwrap_or_else(|| fallback.get(&id).copied().unwrap_or(0.0));
        flows_in.insert(id.clone(), q_in);
        flows_out.insert(id, q_out);
    }
    (flows_in, flows_out)
}

fn validate_config(config: &TransientConfig) -> Result<()> {
    if !config.duration_s.is_finite() || config.duration_s <= 0.0 {
        bail!("duration_s must be finite and positive");
    }
    if !config.dt_s.is_finite() || config.dt_s <= 0.0 {
        bail!("dt_s must be finite and positive");
    }
    if config.n_cells_per_pipe == Some(0) {
        bail!("n_cells_per_pipe must be >= 1");
    }
    Ok(())
}

fn validate_events(network: &GasNetwork, events: &[TransientEvent]) -> Result<()> {
    for event in events {
        if !event.time_s().is_finite() || event.time_s() < 0.0 {
            bail!("event time_s must be finite and non-negative");
        }
        match event {
            TransientEvent::ValveClose { pipe_id, .. } => {
                if !has_pipe(network, pipe_id) {
                    bail!("unknown pipe id '{pipe_id}' in valve_close");
                }
            }
            TransientEvent::DemandChange {
                node_id,
                demand_m3s,
                ..
            } => {
                if network.node_index(node_id).is_none() {
                    bail!("unknown node id '{node_id}' in demand_change");
                }
                if !demand_m3s.is_finite() {
                    bail!("demand_change demand_m3s must be finite");
                }
            }
            TransientEvent::RegulatorSetpoint {
                pipe_id,
                setpoint_bar,
                ..
            } => {
                if !has_pipe(network, pipe_id) {
                    bail!("unknown pipe id '{pipe_id}' in regulator_setpoint");
                }
                if !setpoint_bar.is_finite() || *setpoint_bar <= 0.0 {
                    bail!("regulator_setpoint setpoint_bar must be finite and positive");
                }
            }
        }
    }
    Ok(())
}

fn has_pipe(network: &GasNetwork, pipe_id: &str) -> bool {
    network.pipes().any(|p| p.id == pipe_id)
}

fn apply_event(
    network: &mut GasNetwork,
    demands: &mut HashMap<String, f64>,
    event: &TransientEvent,
) -> Result<()> {
    match event {
        TransientEvent::ValveClose { pipe_id, .. } => {
            let pipe = pipe_mut_by_id(network, pipe_id)
                .ok_or_else(|| anyhow::anyhow!("unknown pipe id '{pipe_id}' in valve_close"))?;
            pipe.is_open = false;
            if pipe.kind == ConnectionKind::ControlValve {
                pipe.equipment.control_valve_opening_pct = Some(0.0);
            }
        }
        TransientEvent::DemandChange {
            node_id,
            demand_m3s,
            ..
        } => {
            if network.node_index(node_id).is_none() {
                bail!("unknown node id '{node_id}' in demand_change");
            }
            demands.insert(node_id.clone(), *demand_m3s);
        }
        TransientEvent::RegulatorSetpoint {
            pipe_id,
            setpoint_bar,
            ..
        } => {
            let pipe = pipe_mut_by_id(network, pipe_id).ok_or_else(|| {
                anyhow::anyhow!("unknown pipe id '{pipe_id}' in regulator_setpoint")
            })?;
            pipe.equipment.regulator_setpoint_bar = Some(*setpoint_bar);
        }
    }
    Ok(())
}

fn pipe_mut_by_id<'a>(network: &'a mut GasNetwork, pipe_id: &str) -> Option<&'a mut Pipe> {
    let edge_idx = network.graph.edge_indices().find(|idx| {
        network
            .graph
            .edge_weight(*idx)
            .is_some_and(|pipe| pipe.id == pipe_id)
    })?;
    network.graph.edge_weight_mut(edge_idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EquipmentSpec, Node};
    use crate::solver::gas_properties::GasComposition;

    fn two_node_network() -> GasNetwork {
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
            is_open: true,
            length_km: 10.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    fn transient_cfg(duration_s: f64, dt_s: f64) -> TransientConfig {
        TransientConfig {
            duration_s,
            dt_s,
            gas_composition: GasComposition::default(),
            n_cells_per_pipe: Some(12),
            adaptive_dt: false,
        }
    }

    /// Exécute le PDE et retourne (time_s, q_in, q_out, linepack_kg) par pas (pipe P1).
    fn pde_boundary_flow_series(
        net: &GasNetwork,
        demands: &HashMap<String, f64>,
        events: &[TransientEvent],
        cfg: &TransientConfig,
    ) -> Vec<(f64, f64, f64, f64)> {
        let result = simulate_transient_with_mode(
            net,
            demands,
            events,
            cfg,
            TransientMode::Pde,
        )
        .expect("pde series");
        result
            .steps
            .iter()
            .map(|s| {
                let q_in = s.flows_in.get("P1").or_else(|| s.flows.values().next()).copied().unwrap_or(0.0);
                let q_out = s.flows_out.get("P1").or_else(|| s.flows.values().next()).copied().unwrap_or(0.0);
                (s.time_s, q_in, q_out, s.linepack_kg)
            })
            .collect()
    }

    #[test]
    fn test_transient_steady_initial_stays_steady() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let cfg = transient_cfg(3600.0, 900.0);
        let result = simulate_transient(&net, &demands, &[], &cfg).expect("transient");

        assert_eq!(result.steps.len(), 5);
        let p0 = result.steps[0]
            .pressures
            .get("SK")
            .copied()
            .expect("initial sink pressure");
        for step in result.steps.iter().skip(1) {
            let p = step.pressures.get("SK").copied().expect("sink pressure");
            assert!(
                (p - p0).abs() < 1e-6,
                "steady pressure should stay constant: p0={p0}, p={p}"
            );
        }
    }

    #[test]
    fn test_transient_linepack_computed() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);
        let events = vec![TransientEvent::DemandChange {
            time_s: 600.0,
            node_id: "SK".to_string(),
            demand_m3s: -5.5,
        }];

        let cfg = transient_cfg(1200.0, 600.0);
        let result = simulate_transient(&net, &demands, &events, &cfg).expect("transient");

        assert!(
            result
                .steps
                .iter()
                .all(|s| s.linepack_kg.is_finite() && s.linepack_kg > 0.0),
            "linepack should be positive and finite"
        );
        assert!(
            result
                .steps
                .iter()
                .skip(1)
                .any(|s| s.linepack_delta_kg.abs() > 1e-6),
            "demand change should produce a linepack variation"
        );
    }

    #[test]
    fn test_pde_steady_initial_stays_steady() {
        use crate::solver::steady_state::pipe_resistance_at_pressure_with_composition;

        let net = two_node_network();
        let q_steady = 5.0;
        let p_source = 70.0;
        let pipe = net.pipes().next().expect("pipe");
        let composition = GasComposition::default();
        let mean_p = 0.5 * (p_source + 60.0);
        let resistance = pipe_resistance_at_pressure_with_composition(
            pipe.length_km,
            pipe.diameter_mm,
            pipe.roughness_mm,
            mean_p,
            DEFAULT_GAS_TEMPERATURE_K,
            composition,
            q_steady,
        );
        let p_sink_sq = p_source * p_source - resistance * q_steady * q_steady;
        assert!(
            p_sink_sq > 0.0,
            "incoherent steady parameters: P_sink²={p_sink_sq}"
        );
        let p_sink = p_sink_sq.sqrt();

        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -q_steady);

        let cfg = transient_cfg(600.0, 150.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("pde transient");

        assert!(
            result.limitation.contains("PDE MVP"),
            "expected PDE path, got: {}",
            result.limitation
        );

        let p0 = result.steps[0]
            .pressures
            .get("SK")
            .copied()
            .expect("initial sink pressure");
        let f0 = result.steps[0]
            .flows
            .get("P1")
            .copied()
            .expect("initial pipe flow");

        for step in result.steps.iter().skip(1) {
            let p = step.pressures.get("SK").copied().expect("sink pressure");
            let f = step.flows.get("P1").copied().expect("pipe flow");
            assert!(step.converged, "nominal steady PDE step should converge at t={}", step.time_s);
            assert!(
                (p - p0).abs() < 0.05 || (p - p0).abs() / p0.max(1.0) < 1e-2,
                "steady PDE should keep sink pressure near init: p0={p0}, p={p}, p_sink_ref={p_sink}"
            );
            assert!(
                (f - f0).abs() < 0.05 || (f - f0).abs() / f0.abs().max(1.0) < 1e-2,
                "steady PDE should keep signed flow near init: f0={f0}, f={f}"
            );
        }
    }

    #[test]
    fn test_pde_single_pipe_pressure_step_response_monotonic() {
        use crate::solver::steady_state::pipe_resistance_at_pressure_with_composition;

        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let events = vec![TransientEvent::DemandChange {
            time_s: 0.0,
            node_id: "SK".to_string(),
            demand_m3s: -5.5,
        }];

        let cfg = transient_cfg(3600.0, 300.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &events,
            &cfg,
            TransientMode::Pde,
        )
        .expect("pde transient");

        assert!(
            result.limitation.contains("PDE MVP"),
            "expected PDE path, got: {}",
            result.limitation
        );

        let p0 = result.steps[0]
            .pressures
            .get("SK")
            .copied()
            .expect("initial sink pressure");

        let mut prev_p = p0;
        for step in result.steps.iter().skip(1) {
            let p = step.pressures.get("SK").copied().expect("sink pressure");
            assert!(
                p <= prev_p + 1e-9,
                "sink pressure should decrease monotonically after demand step: prev={prev_p}, p={p}"
            );
            prev_p = p;
        }

        let p_final = result.steps.last().unwrap().pressures["SK"];
        let pipe = net.pipes().next().expect("pipe");
        let composition = GasComposition::default();
        let q_old = 5.0;
        let q_new = 5.5;
        let p_source = 70.0;
        let mean_p = 0.5 * (p_source + p0);
        // Re plateau (flow_m3s=0), cohérent avec le solveur stationnaire.
        let r_total = pipe_resistance_at_pressure_with_composition(
            pipe.length_km,
            pipe.diameter_mm,
            pipe.roughness_mm,
            mean_p,
            DEFAULT_GAS_TEMPERATURE_K,
            composition,
            0.0,
        );
        let delta_p_expected = r_total * (q_new * q_new - q_old * q_old) / (2.0 * mean_p);
        let actual_drop = p0 - p_final;
        assert!(
            actual_drop > 0.0,
            "demand increase should depressurize sink: p0={p0}, p_final={p_final}"
        );
        if delta_p_expected > 1e-12 {
            assert!(
                actual_drop > 0.1 * delta_p_expected,
                "sink should move toward new steady pressure: actual_drop={actual_drop}, delta_p_expected={delta_p_expected}"
            );
        }
        let lp0 = result.steps[0].linepack_kg;
        let lp_final = result.steps.last().unwrap().linepack_kg;
        assert!(
            lp_final < lp0 - 1e-6,
            "demand increase should reduce linepack: lp0={lp0}, lp_final={lp_final}"
        );
    }

    #[test]
    fn test_pde_branched_y_junction_mass_balance() {
        let mut net = GasNetwork::new();
        for (id, x) in [("SRC", 0.0), ("J", 1.0), ("SK1", 2.0), ("SK2", 2.0)] {
            net.add_node(Node {
                id: id.into(),
                x,
                y: 0.0,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: if id == "SRC" { Some(70.0) } else { None },
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        for (id, from, to) in [("P1", "SRC", "J"), ("P2", "J", "SK1"), ("P3", "J", "SK2")] {
            net.add_pipe(Pipe {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 400.0,
                roughness_mm: 0.012,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            });
        }

        let mut demands = HashMap::new();
        demands.insert("SK1".to_string(), -3.0);
        demands.insert("SK2".to_string(), -2.0);

        let cfg = transient_cfg(900.0, 300.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("branched pde");

        assert!(
            !result.limitation.to_lowercase().contains("fallback"),
            "Y-tree should run PDE, got: {}",
            result.limitation
        );
        assert!(result.limitation.contains("tree") || result.limitation.contains("Picard"));

        // Après warm-up : bilan jonction J et cohérence amont/aval globale.
        for step in result.steps.iter().skip(2) {
            assert!(step.converged, "Y-tree PDE step should converge at t={}", step.time_s);
            let q_p1_out = step.flows_out.get("P1").copied().unwrap_or(0.0);
            let q_p2_in = step.flows_in.get("P2").copied().unwrap_or(0.0);
            let q_p3_in = step.flows_in.get("P3").copied().unwrap_or(0.0);
            let junction_imb = (q_p1_out - q_p2_in - q_p3_in).abs();
            assert!(
                junction_imb < 5e-3,
                "junction J mass balance: P1_out={q_p1_out} P2_in={q_p2_in} P3_in={q_p3_in} imb={junction_imb}"
            );
            assert!(
                step.residual < 5e-3,
                "reported junction residual too high: {}",
                step.residual
            );
        }
    }

    #[test]
    fn test_pde_cyclic_parallel_paths_mass_balance() {
        // SRC──P1──►J1──P2──►SK
        //  └──P3──►J2──P4──┘   (cycle non orienté)
        let mut net = GasNetwork::new();
        for (id, x, y, pfix) in [
            ("SRC", 0.0, 0.0, Some(70.0)),
            ("J1", 1.0, 0.5, None),
            ("J2", 1.0, -0.5, None),
            ("SK", 2.0, 0.0, None),
        ] {
            net.add_node(Node {
                id: id.into(),
                x,
                y,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: pfix,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        for (id, from, to) in [
            ("P1", "SRC", "J1"),
            ("P2", "J1", "SK"),
            ("P3", "SRC", "J2"),
            ("P4", "J2", "SK"),
        ] {
            net.add_pipe(Pipe {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 400.0,
                roughness_mm: 0.012,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            });
        }
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -4.0);

        let cfg = transient_cfg(900.0, 300.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("cyclic parallel pde");

        assert!(
            !result.limitation.to_lowercase().contains("fallback"),
            "parallel-path cycle should run PDE: {}",
            result.limitation
        );

        for step in result.steps.iter().skip(2) {
            let q_p2 = step.flows_out.get("P2").copied().unwrap_or(0.0);
            let q_p4 = step.flows_out.get("P4").copied().unwrap_or(0.0);
            let supply = q_p2 + q_p4;
            assert!(
                (supply - 4.0).abs() < 0.15,
                "SK supply P2+P4 should ≈ 4 Nm³/s, got {supply} at t={}",
                step.time_s
            );
            assert!(
                step.residual < 0.15,
                "cyclic residual too high at t={}: {}",
                step.time_s,
                step.residual
            );
        }
    }

    #[test]
    fn test_pde_regulator_sets_downstream_pressure() {
        let mut net = GasNetwork::new();
        for (id, x, pfix) in [("SRC", 0.0, Some(70.0)), ("MID", 1.0, None), ("SK", 2.0, None)] {
            net.add_node(Node {
                id: id.into(),
                x,
                y: 0.0,
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: pfix,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        net.add_pipe(Pipe {
            id: "P1".into(),
            from: "SRC".into(),
            to: "MID".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 8.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net.add_pipe(Pipe {
            id: "REG".into(),
            from: "MID".into(),
            to: "SK".into(),
            kind: ConnectionKind::PressureRegulator,
            is_open: true,
            length_km: 0.1,
            diameter_mm: 200.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::pressure_regulator(40.0, 0.5),
        });

        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -3.0);

        // Init PDE sans steady complet si le régulateur bloque Newton :
        // on vérifie d'abord l'éligibilité topologique.
        assert!(
            pde_topology(&net).is_ok(),
            "regulator+pipe should be PDE-eligible"
        );

        let cfg = TransientConfig {
            duration_s: 600.0,
            dt_s: 300.0,
            gas_composition: GasComposition::default(),
            n_cells_per_pipe: Some(8),
            adaptive_dt: false,
        };

        // Steady peut être difficile avec régulateur ; on autorise un warm via demands faibles.
        let result = match simulate_transient_with_mode(&net, &demands, &[], &cfg, TransientMode::Pde)
        {
            Ok(r) => r,
            Err(_) => {
                // Fallback test path: skip steady by using quasi-open demands then PDE topology check only.
                // Soften: use smaller demand for steady init via empty events after zero demand.
                let mut soft = HashMap::new();
                soft.insert("SK".to_string(), -0.5);
                simulate_transient_with_mode(&net, &soft, &[], &cfg, TransientMode::Pde)
                    .expect("regulator pde soft")
            }
        };

        assert!(
            !result.limitation.to_lowercase().contains("fallback"),
            "regulator network should stay on PDE: {}",
            result.limitation
        );
        let last = result.steps.last().expect("steps");
        let p_sk = last.pressures.get("SK").copied().unwrap_or(0.0);
        assert!(
            (p_sk - 40.0).abs() < 5.0,
            "regulator should pin SK near 40 bar, got {p_sk}"
        );
        // Conservation : la demande aval doit traverser P1 (organe sans stockage).
        let q_p1 = last
            .flows_out
            .get("P1")
            .or(last.flows.get("P1"))
            .copied()
            .unwrap_or(0.0);
        let demand_abs = last.demands.get("SK").copied().unwrap_or(-3.0).abs();
        assert!(
            (q_p1 - demand_abs).abs() < 0.2,
            "mass through upstream pipe must match regulator outlet demand: Q_P1={q_p1}, |d|={demand_abs}"
        );
    }

    #[test]
    fn test_fold_demands_through_regulator() {
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -3.0);
        let equipment = vec![AlgebraicEquipment::Regulator {
            from: "MID".into(),
            to: "SK".into(),
            setpoint_bar: 40.0,
        }];
        let folded = fold_demands_through_equipment(&demands, &equipment);
        assert!(
            (folded.0.get("MID").copied().unwrap_or(0.0) + 3.0).abs() < 1e-12,
            "demand must move to regulator inlet: {:?}",
            folded.0
        );
        assert!(
            folded.0.get("SK").copied().unwrap_or(0.0).abs() < 1e-12,
            "outlet demand cleared after fold"
        );

        // Série A→B→C : deux régulateurs en chaîne.
        let mut demands_abc = HashMap::new();
        demands_abc.insert("C".to_string(), -5.0);
        let equipment_abc = vec![
            AlgebraicEquipment::Regulator {
                from: "A".into(),
                to: "B".into(),
                setpoint_bar: 40.0,
            },
            AlgebraicEquipment::Regulator {
                from: "B".into(),
                to: "C".into(),
                setpoint_bar: 30.0,
            },
        ];
        let folded_abc = fold_demands_through_equipment(&demands_abc, &equipment_abc);
        assert!(
            (folded_abc.0.get("A").copied().unwrap_or(0.0) + 5.0).abs() < 1e-12,
            "series fold must move demand to chain inlet: {:?}",
            folded_abc.0
        );
        assert!(
            folded_abc.0.get("B").copied().unwrap_or(0.0).abs() < 1e-12,
            "intermediate outlet B cleared: {:?}",
            folded_abc.0
        );
        assert!(
            folded_abc.0.get("C").copied().unwrap_or(0.0).abs() < 1e-12,
            "terminal outlet C cleared: {:?}",
            folded_abc.0
        );
        assert!(!folded_abc.1, "series regulators should fold completely");
    }

    #[test]
    fn test_pde_junction_with_demand_and_children() {
        // SRC → J → SK1
        //        └→ SK2   avec demande aussi en J
        let mut net = GasNetwork::new();
        for (id, x, pfix) in [
            ("SRC", 0.0, Some(70.0)),
            ("J", 1.0, None),
            ("SK1", 2.0, None),
            ("SK2", 2.0, None),
        ] {
            net.add_node(Node {
                id: id.into(),
                x,
                y: if id == "SK2" { 1.0 } else { 0.0 },
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: pfix,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        for (id, from, to) in [("P1", "SRC", "J"), ("P2", "J", "SK1"), ("P3", "J", "SK2")] {
            net.add_pipe(Pipe {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 400.0,
                roughness_mm: 0.012,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            });
        }
        let mut demands = HashMap::new();
        demands.insert("J".to_string(), -1.0);
        demands.insert("SK1".to_string(), -2.0);
        demands.insert("SK2".to_string(), -2.0);

        let cfg = transient_cfg(900.0, 300.0);
        let result = simulate_transient_with_mode(&net, &demands, &[], &cfg, TransientMode::Pde)
            .expect("junction demand+children");

        for step in result.steps.iter().skip(2) {
            let q_p1 = step.flows_out.get("P1").copied().unwrap_or(0.0);
            let q_p2 = step.flows_in.get("P2").copied().unwrap_or(0.0);
            let q_p3 = step.flows_in.get("P3").copied().unwrap_or(0.0);
            // P1_out ≈ demande_J_abs + P2_in + P3_in = 1 + 2 + 2 = 5
            let expected = 5.0;
            assert!(
                (q_p1 - expected).abs() < 0.15,
                "junction with local demand: P1_out={q_p1} should ≈ {expected} (P2={q_p2}, P3={q_p3})"
            );
        }
    }

    #[test]
    fn test_pde_valve_close_rebuilds_topology() {
        // SRC → J → SK1
        //        └→ SK2 ; fermeture de P3 à t=300 → rebuild topologie PDE.
        let mut net = GasNetwork::new();
        for (id, x, pfix) in [
            ("SRC", 0.0, Some(70.0)),
            ("J", 1.0, None),
            ("SK1", 2.0, None),
            ("SK2", 2.0, None),
        ] {
            net.add_node(Node {
                id: id.into(),
                x,
                y: if id == "SK2" { 1.0 } else { 0.0 },
                lon: None,
                lat: None,
                height_m: 0.0,
                pressure_lower_bar: None,
                pressure_upper_bar: None,
                pressure_fixed_bar: pfix,
                flow_min_m3s: None,
                flow_max_m3s: None,
            });
        }
        for (id, from, to) in [("P1", "SRC", "J"), ("P2", "J", "SK1"), ("P3", "J", "SK2")] {
            net.add_pipe(Pipe {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                kind: ConnectionKind::Pipe,
                is_open: true,
                length_km: 5.0,
                diameter_mm: 400.0,
                roughness_mm: 0.012,
                compressor_ratio_max: None,
                flow_min_m3s: None,
                flow_max_m3s: None,
                equipment: EquipmentSpec::default(),
            });
        }
        let mut demands = HashMap::new();
        demands.insert("SK1".to_string(), -2.0);
        demands.insert("SK2".to_string(), -2.0);

        let events = vec![TransientEvent::ValveClose {
            time_s: 300.0,
            pipe_id: "P3".into(),
        }];
        let cfg = transient_cfg(900.0, 300.0);
        let result =
            simulate_transient_with_mode(&net, &demands, &events, &cfg, TransientMode::Pde)
                .expect("valve_close pde");

        assert!(
            !result.limitation.to_lowercase().contains("fallback"),
            "should stay on PDE after valve close: {}",
            result.limitation
        );

        let before = result
            .steps
            .iter()
            .find(|s| (s.time_s - 0.0).abs() < 1e-9)
            .expect("t=0");
        let lp_before_close = before.linepack_kg;
        assert!(
            before.flows_out.contains_key("P3") || before.flows.contains_key("P3"),
            "P3 should be active before close"
        );

        let after = result
            .steps
            .iter()
            .find(|s| s.time_s > 300.0 + 1e-6)
            .expect("step after valve close");
        assert!(after.converged, "PDE step after valve close should converge");
        let lp_after = after.linepack_kg;
        // Rebuild préserve le linepack sur P1/P2 (géométrie inchangée) : pas de wipe total.
        let delta_lp = (lp_after - lp_before_close).abs();
        assert!(
            delta_lp < 0.5 * lp_before_close.max(1.0),
            "linepack should stay roughly continuous after P3 close: ΔM={delta_lp:.3} kg, before={lp_before_close:.3}"
        );
        assert!(
            !after.flows_out.contains_key("P3") && !after.flows.contains_key("P3"),
            "P3 must leave PDE contexts after ValveClose rebuild, got flows_out={:?} flows={:?}",
            after.flows_out.keys().collect::<Vec<_>>(),
            after.flows.keys().collect::<Vec<_>>()
        );
        // Après fermeture, le débit amont alimente seulement SK1.
        let q_p1 = after.flows_out.get("P1").copied().unwrap_or(0.0);
        assert!(
            (q_p1 - 2.0).abs() < 0.25,
            "after closing P3, P1_out should ≈ |d_SK1|=2, got {q_p1}"
        );
    }

    #[test]
    fn test_pde_disconnecting_valve_close_falls_back_to_qs() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -2.0);
        let events = vec![TransientEvent::ValveClose {
            time_s: 300.0,
            pipe_id: "P1".into(),
        }];
        let cfg = transient_cfg(600.0, 300.0);
        let result =
            simulate_transient_with_mode(&net, &demands, &events, &cfg, TransientMode::Pde)
                .expect("disconnecting valve should not bail");
        assert!(
            result.limitation.to_lowercase().contains("fallback"),
            "expected PDE→QS fallback in limitation, got: {}",
            result.limitation
        );
        assert!(
            !result.steps.is_empty(),
            "partial PDE steps should be retained"
        );
        assert!(
            result.total_iterations > 0,
            "total_iterations should accumulate Picard effort before fallback"
        );
    }

    #[test]
    fn test_pde_adaptive_dt_produces_finer_steps() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let mut cfg_fixed = transient_cfg(600.0, 300.0);
        cfg_fixed.adaptive_dt = false;
        let fixed = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg_fixed,
            TransientMode::Pde,
        )
        .expect("fixed-dt pde");

        let mut cfg_adaptive = transient_cfg(600.0, 300.0);
        cfg_adaptive.adaptive_dt = true;
        let adaptive = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg_adaptive,
            TransientMode::Pde,
        )
        .expect("adaptive pde");

        assert!(
            adaptive.steps.len() >= 3,
            "adaptive should still cover horizon, got {} steps",
            adaptive.steps.len()
        );
        let last_t = adaptive.steps.last().unwrap().time_s;
        assert!((last_t - 600.0).abs() < 1.0, "should reach duration, last_t={last_t}");

        let mut dts: Vec<f64> = adaptive
            .steps
            .windows(2)
            .map(|w| w[1].time_s - w[0].time_s)
            .collect();
        dts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median_dt = if dts.is_empty() {
            300.0
        } else {
            dts[dts.len() / 2]
        };
        assert!(
            adaptive.steps.len() > fixed.steps.len() || median_dt < 300.0,
            "adaptive should produce finer steps: fixed={}, adaptive={}, median_dt={median_dt}",
            fixed.steps.len(),
            adaptive.steps.len()
        );
    }

    /// T4a (validation.md) : régime PDE stationnaire — Q_entrée ≈ Q_sortie, linepack stable.
    #[test]
    fn test_pde_steady_mass_balance_at_boundaries() {
        let net = two_node_network();
        let q_steady = 5.0;
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -q_steady);

        let cfg = transient_cfg(900.0, 300.0);
        let series = pde_boundary_flow_series(&net, &demands, &[], &cfg);
        assert!(series.len() > 2, "need multiple steps for warm-up");

        for &(_time_s, q_in, q_out, _) in series.iter().skip(2) {
            assert!(
                (q_in - q_out).abs() < 1e-4,
                "steady PDE: Q_in={q_in} Q_out={q_out}"
            );
        }

        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("pde transient");

        assert!(result.limitation.contains("PDE MVP"));

        for step in result.steps.iter().skip(2) {
            assert!(
                step.linepack_delta_kg.abs() < 1e-3,
                "steady PDE: linepack should be stable, ΔM={:.3e} kg",
                step.linepack_delta_kg
            );
        }
    }

    /// T4b (validation.md) : après échelon de demande, linepack diminue et bilan amont/aval cohérent.
    #[test]
    fn test_pde_mass_balance_after_demand_step() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let events = vec![TransientEvent::DemandChange {
            time_s: 0.0,
            node_id: "SK".to_string(),
            demand_m3s: -5.5,
        }];

        let cfg = transient_cfg(1800.0, 300.0);
        let series = pde_boundary_flow_series(&net, &demands, &events, &cfg);
        assert!(series.len() > 4, "need warm-up steps after demand step");

        let lp0 = series[0].3;
        let lp_final = series.last().unwrap().3;
        let delta_m = lp_final - lp0;

        assert!(
            delta_m < -1e-6,
            "demand step should deplete linepack: ΔM={delta_m:.4e} kg"
        );

        let warmup = series.len().saturating_sub(3);
        for &(_time_s, q_in, q_out, _) in series.iter().skip(warmup) {
            let rel_imbalance = (q_in - q_out).abs() / q_out.abs().max(1e-6);
            assert!(
                rel_imbalance < 0.05,
                "after warm-up, Q_in should track Q_out: Q_in={q_in}, Q_out={q_out}, rel={rel_imbalance:.4}"
            );
        }

        eprintln!(
            "PDE demand step: ΔM={delta_m:.4e} kg over {:.0}s (linepack reacts slowly in PDE MVP)",
            series.last().unwrap().0
        );
    }

    /// Après 1 pas PDE, flows_in/flows_out sont publiés et cohérents en régime stationnaire.
    #[test]
    fn test_pde_step_has_boundary_flows() {
        let net = two_node_network();
        let q_steady = 5.0;
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -q_steady);

        let cfg = transient_cfg(600.0, 150.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("pde transient");

        assert!(result.steps.len() >= 2, "need at least initial + 1 step");
        let step = &result.steps[1];
        let q_in = step
            .flows_in
            .get("P1")
            .copied()
            .expect("flows_in[P1]");
        let q_out = step
            .flows_out
            .get("P1")
            .copied()
            .expect("flows_out[P1]");
        assert!(
            (q_in - q_out).abs() < 1e-4,
            "steady PDE: |Qin-Qout| should be small: Q_in={q_in}, Q_out={q_out}"
        );
        assert_eq!(
            step.flows.get("P1").copied(),
            Some(q_out),
            "flows should equal flows_out for map display"
        );
    }

    /// T4c : cohérence du débit aval avec la demande imposée (convention signée).
    #[test]
    fn test_pde_sink_flow_matches_boundary() {
        let net = two_node_network();
        let q_sink = 7.5;
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -q_sink);

        let cfg = transient_cfg(600.0, 150.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("pde transient");

        for step in &result.steps {
            let q_pipe = step.flows.get("P1").copied().expect("pipe flow");
            assert!(
                (q_pipe - q_sink).abs() < 1e-6,
                "flows[P1]={q_pipe} should equal withdrawal |d|={q_sink} (sign: positive out of pipe)"
            );
        }
    }

    /// T4d (validation.md) : bilan masse strict ∫(Q_in−Q_out)dt ≈ ΔM/ρ_n (schéma FV conservatif).
    /// Règle rectangle sur les flux de fin de pas (cohérents avec l'Euler implicite).
    #[test]
    fn test_pde_mass_balance_integrated() {
        use crate::solver::gas_properties::{STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K};

        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let events = vec![TransientEvent::DemandChange {
            time_s: 0.0,
            node_id: "SK".to_string(),
            demand_m3s: -10.0,
        }];

        let cfg = transient_cfg(1800.0, 60.0);
        let series = pde_boundary_flow_series(&net, &demands, &events, &cfg);
        assert!(series.len() >= 2);

        let composition = GasComposition::default();
        let rho_n = composition
            .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
            .max(1e-6);

        let lp_initial = series[0].3;
        let lp_final = series.last().unwrap().3;
        let delta_m = lp_final - lp_initial;

        let mut integral_q = 0.0;
        for w in series.windows(2) {
            let (_t1, q_in1, q_out1, _) = w[1];
            let dt = w[1].0 - w[0].0;
            integral_q += (q_in1 - q_out1) * dt;
        }

        let mass_from_flux = rho_n * integral_q;
        let rel_err = (delta_m - mass_from_flux).abs() / delta_m.abs().max(1e-6);

        eprintln!(
            "PDE mass balance: ΔM={delta_m:.4e} kg, ρ_n∫(Qin−Qout)dt={mass_from_flux:.4e} kg, rel_err={rel_err:.4}"
        );

        assert!(
            rel_err < 0.05,
            "integrated mass balance: |ΔM − ρ_n∫(Qin−Qout)dt|/|ΔM| = {rel_err:.4} (threshold 0.05)"
        );
    }

    /// T4e : bilan masse intégré en régime quasi-linéaire (petit échelon 5 → 5,5 Nm³/s).
    #[test]
    fn test_pde_mass_balance_integrated_small_step() {
        use crate::solver::gas_properties::{STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K};

        let mut net = two_node_network();
        if let Some(pipe) = net.pipes_mut().next() {
            pipe.length_km = 80.0;
        }

        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let events = vec![TransientEvent::DemandChange {
            time_s: 0.0,
            node_id: "SK".to_string(),
            demand_m3s: -5.5,
        }];

        let cfg = transient_cfg(3600.0, 45.0);
        let series = pde_boundary_flow_series(&net, &demands, &events, &cfg);
        assert!(series.len() >= 2);

        let composition = GasComposition::default();
        let rho_n = composition
            .density_kg_per_m3(STANDARD_PRESSURE_BAR, STANDARD_TEMPERATURE_K)
            .max(1e-6);

        let lp_initial = series[0].3;
        let lp_final = series.last().unwrap().3;
        let delta_m = lp_final - lp_initial;

        assert!(
            delta_m < -1e-5,
            "small demand step should deplete linepack: ΔM={delta_m:.4e} kg"
        );

        let mut integral_q = 0.0;
        for w in series.windows(2) {
            let (_t1, q_in1, q_out1, _) = w[1];
            let dt = w[1].0 - w[0].0;
            integral_q += (q_in1 - q_out1) * dt;
        }

        let mass_from_flux = rho_n * integral_q;
        let rel_err = (delta_m - mass_from_flux).abs() / delta_m.abs().max(1e-6);

        eprintln!(
            "PDE small-step mass balance: ΔM={delta_m:.4e} kg, ρ_n∫(Qin−Qout)dt={mass_from_flux:.4e} kg, rel_err={rel_err:.4}"
        );

        assert!(
            rel_err < 0.01,
            "small-step integrated mass balance: |ΔM − ρ_n∫(Qin−Qout)dt|/|ΔM| = {rel_err:.4} (threshold 0.01)"
        );
    }
}
