//! Simulation transitoire MVP (P11).
//!
//! Deux modes :
//! - **Quasi-steady** : chaque pas re-résout un état permanent (MVP historique).
//! - **PDE** : linepack isotherme 1D par conduite (Euler implicite, tridiagonal).

use std::collections::HashMap;

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
use time_integration::{ActivePipeContext, advance_one_step};

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
    pub flows: HashMap<String, f64>,
    pub iterations: usize,
    pub residual: f64,
    pub linepack_kg: f64,
    pub linepack_delta_kg: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransientResult {
    pub steps: Vec<TransientStepResult>,
    pub total_iterations: usize,
    pub limitation: String,
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
    match mode {
        TransientMode::QuasiSteady => {
            simulate_transient_quasi_steady(network, initial_demands, events, config)
        }
        TransientMode::Pde => simulate_transient_pde(network, initial_demands, events, config),
    }
}

/// Simule un transitoire MVP par résolution quasi-stationnaire.
pub fn simulate_transient_quasi_steady(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
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

    let mut steps = vec![TransientStepResult {
        time_s: 0.0,
        demands: demands.clone(),
        pressures: initial.pressures,
        flows: initial.flows,
        iterations: initial.iterations,
        residual: initial.residual,
        linepack_kg: previous_linepack,
        linepack_delta_kg: 0.0,
    }];

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

        steps.push(TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: solved.pressures,
            flows: solved.flows,
            iterations: solved.iterations,
            residual: solved.residual,
            linepack_kg: linepack,
            linepack_delta_kg: linepack_delta,
        });
    }

    Ok(TransientResult {
        steps,
        total_iterations,
        limitation: "Quasi-steady MVP: each time step re-solves steady-state; PDE wave dynamics are not modeled."
            .to_string(),
    })
}

/// Simule un transitoire PDE 1D isotherme (MVP : une conduite ou chaîne série).
///
/// Retombe sur le quasi-stationnaire si le réseau est trop complexe (branches, organes).
pub fn simulate_transient_pde(
    network: &GasNetwork,
    initial_demands: &HashMap<String, f64>,
    events: &[TransientEvent],
    config: &TransientConfig,
) -> Result<TransientResult> {
    validate_config(config)?;
    validate_events(network, events)?;

    if !is_pde_eligible(network) {
        let mut result =
            simulate_transient_quasi_steady(network, initial_demands, events, config)?;
        result.limitation = format!(
            "{} Fallback to quasi-steady: network topology not supported by PDE MVP.",
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
        solve_steady_state_with_progress(&network_state, &demands, None, steady_cfg, |_| {
            SolverControl::Continue
        })?;

    let ordered_pipes = pde_pipe_chain(&network_state)?;
    let mut pipe_contexts = build_pipe_contexts(
        &network_state,
        &ordered_pipes,
        &initial.pressures,
        &demands,
        config,
    )?;

    let mut previous_linepack = total_pde_linepack(&pipe_contexts, &config.gas_composition);
    let mut steps = vec![TransientStepResult {
        time_s: 0.0,
        demands: demands.clone(),
        pressures: snapshot_node_pressures(&pipe_contexts, &initial.pressures),
        flows: snapshot_pipe_flows(&pipe_contexts, &initial.flows),
        iterations: initial.iterations,
        residual: initial.residual,
        linepack_kg: previous_linepack,
        linepack_delta_kg: 0.0,
    }];

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
            refresh_pde_boundaries(&mut pipe_contexts, &network_state, &demands);
            event_cursor += 1;
        }

        advance_one_step(&mut pipe_contexts, config.dt_s, &config.gas_composition);
        sync_chain_junctions(&mut pipe_contexts);

        let linepack = total_pde_linepack(&pipe_contexts, &config.gas_composition);
        let linepack_delta = linepack - previous_linepack;
        previous_linepack = linepack;

        steps.push(TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: snapshot_node_pressures(&pipe_contexts, &initial.pressures),
            flows: snapshot_pipe_flows(&pipe_contexts, &initial.flows),
            iterations: 0,
            residual: 0.0,
            linepack_kg: linepack,
            linepack_delta_kg: linepack_delta,
        });
    }

    Ok(TransientResult {
        steps,
        total_iterations: initial.iterations,
        limitation: "PDE MVP: isothermal linepack 1D implicit Euler per pipe; no thermal transients; chain coupling explicit at junctions."
            .to_string(),
    })
}

fn is_pde_eligible(network: &GasNetwork) -> bool {
    pde_pipe_chain(network).is_ok()
}

/// Retourne les conduites actives ordonnées amont→aval (chaîne série ou pipe unique).
fn pde_pipe_chain(network: &GasNetwork) -> Result<Vec<String>> {
    let active: Vec<&Pipe> = network
        .pipes()
        .filter(|p| p.hydraulically_active() && p.kind == ConnectionKind::Pipe)
        .collect();

    if active.is_empty() {
        bail!("no active pipe for PDE transient");
    }

    let mut in_deg: HashMap<&str, usize> = HashMap::new();
    let mut out_deg: HashMap<&str, usize> = HashMap::new();
    for p in &active {
        *out_deg.entry(p.from.as_str()).or_default() += 1;
        *in_deg.entry(p.to.as_str()).or_default() += 1;
    }

    let sources: Vec<&str> = out_deg
        .keys()
        .copied()
        .filter(|n| in_deg.get(n).copied().unwrap_or(0) == 0)
        .collect();
    let sinks: Vec<&str> = in_deg
        .keys()
        .copied()
        .filter(|n| out_deg.get(n).copied().unwrap_or(0) == 0)
        .collect();

    if sources.len() != 1 || sinks.len() != 1 {
        bail!("PDE MVP requires a single source-to-sink chain");
    }

    for node in in_deg.keys().chain(out_deg.keys()) {
        let indeg = in_deg.get(node).copied().unwrap_or(0);
        let outdeg = out_deg.get(node).copied().unwrap_or(0);
        if indeg > 1 || outdeg > 1 {
            bail!("PDE MVP does not support branched topology");
        }
    }

    let mut ordered = Vec::with_capacity(active.len());
    let mut current = sources[0].to_string();
    let pipe_by_from: HashMap<&str, &Pipe> = active.iter().map(|p| (p.from.as_str(), *p)).collect();

    while let Some(pipe) = pipe_by_from.get(current.as_str()) {
        ordered.push(pipe.id.clone());
        current = pipe.to.clone();
        if current == sinks[0] {
            break;
        }
    }

    if ordered.len() != active.len() {
        bail!("PDE MVP requires a connected pipe chain");
    }

    Ok(ordered)
}

fn build_pipe_contexts(
    network: &GasNetwork,
    ordered_pipe_ids: &[String],
    node_pressures: &HashMap<String, f64>,
    demands: &HashMap<String, f64>,
    config: &TransientConfig,
) -> Result<Vec<ActivePipeContext>> {
    let mut contexts = Vec::with_capacity(ordered_pipe_ids.len());

    for pipe_id in ordered_pipe_ids {
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
        let sink_flow = demands.get(&pipe.to).copied().unwrap_or(0.0);
        let source_p = network
            .nodes()
            .find(|n| n.id == pipe.from)
            .and_then(|n| n.pressure_fixed_bar)
            .unwrap_or(p_from);

        let n_cells = config.n_cells_per_pipe.or_else(|| Some(default_n_cells(pipe.length_km)));
        let mesh = PipeMesh::from_pipe(&pipe, n_cells);
        let state = TransientPipeState::from_endpoint_pressures(&mesh, p_from, p_to, -sink_flow);

        contexts.push(ActivePipeContext {
            pipe,
            mesh,
            state,
            source: SourceBoundary::fixed_pressure(source_p),
            sink: SinkBoundary::fixed_flow(sink_flow),
        });
    }

  Ok(contexts)
}

fn refresh_pde_boundaries(
    contexts: &mut [ActivePipeContext],
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
) {
    for ctx in contexts.iter_mut() {
        if let Some(node) = network.nodes().find(|n| n.id == ctx.pipe.from)
            && let Some(p) = node.pressure_fixed_bar
        {
            ctx.source = SourceBoundary::fixed_pressure(p);
        }
        if let Some(&flow) = demands.get(&ctx.pipe.to) {
            ctx.sink = SinkBoundary::fixed_flow(flow);
        }
    }
}

fn sync_chain_junctions(contexts: &mut [ActivePipeContext]) {
    for i in 0..contexts.len().saturating_sub(1) {
        let junction_p = *contexts[i].state.pressures.last().unwrap_or(&0.0);
        if let Some(first) = contexts[i + 1].state.pressures.first_mut() {
            *first = junction_p;
        }
        contexts[i + 1].source = SourceBoundary::fixed_pressure(junction_p);
    }
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

fn snapshot_pipe_flows(
    contexts: &[ActivePipeContext],
    fallback: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut flows = fallback.clone();
    for ctx in contexts {
        let q = ctx
            .state
            .flows
            .iter()
            .copied()
            .fold(0.0_f64, |acc, v| if v.abs() > acc.abs() { v } else { acc });
        flows.insert(ctx.pipe.id.clone(), q);
    }
    flows
}

fn validate_config(config: &TransientConfig) -> Result<()> {
    if !config.duration_s.is_finite() || config.duration_s <= 0.0 {
        bail!("duration_s must be finite and positive");
    }
    if !config.dt_s.is_finite() || config.dt_s <= 0.0 {
        bail!("dt_s must be finite and positive");
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
        }
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
            demand_m3s: -8.0,
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
    fn test_pde_single_pipe_pressure_step_response_monotonic() {
        let net = two_node_network();
        let mut demands = HashMap::new();
        demands.insert("SK".to_string(), -5.0);

        let events = vec![TransientEvent::DemandChange {
            time_s: 0.0,
            node_id: "SK".to_string(),
            demand_m3s: -10.0,
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
        assert!(
            p_final < p0 - 0.01,
            "demand increase should depressurize sink: p0={p0}, p_final={p_final}"
        );
    }

    #[test]
    fn test_pde_fallback_on_branched_network() {
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
        for (id, to) in [("P1", "J"), ("P2", "SK1"), ("P3", "SK2")] {
            net.add_pipe(Pipe {
                id: id.into(),
                from: if id == "P1" { "SRC" } else { "J" }.into(),
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

        let cfg = transient_cfg(600.0, 300.0);
        let result = simulate_transient_with_mode(
            &net,
            &demands,
            &[],
            &cfg,
            TransientMode::Pde,
        )
        .expect("fallback transient");

        assert!(
            result.limitation.contains("Fallback to quasi-steady"),
            "branched network should fallback: {}",
            result.limitation
        );
    }
}
