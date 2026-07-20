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

        let step_flows = solved.flows.clone();
        steps.push(TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: solved.pressures,
            flows: step_flows.clone(),
            flows_in: step_flows.clone(),
            flows_out: step_flows,
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
    let (initial_flows_in, initial_flows_out) =
        snapshot_pipe_boundary_flows(&pipe_contexts, &initial.flows);
    let mut steps = vec![TransientStepResult {
        time_s: 0.0,
        demands: demands.clone(),
        pressures: snapshot_node_pressures(&pipe_contexts, &initial.pressures),
        flows: initial_flows_out.clone(),
        flows_in: initial_flows_in,
        flows_out: initial_flows_out,
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

        let (flows_in, flows_out) =
            snapshot_pipe_boundary_flows(&pipe_contexts, &initial.flows);
        steps.push(TransientStepResult {
            time_s,
            demands: demands.clone(),
            pressures: snapshot_node_pressures(&pipe_contexts, &initial.pressures),
            flows: flows_out.clone(),
            flows_in,
            flows_out,
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

fn snapshot_pipe_boundary_flows(
    contexts: &[ActivePipeContext],
    fallback: &HashMap<String, f64>,
) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let mut flows_in = fallback.clone();
    let mut flows_out = fallback.clone();
    for ctx in contexts {
        let q_in = ctx.state.flows.first().copied().unwrap_or(0.0);
        let q_out = ctx.state.flows.last().copied().unwrap_or(0.0);
        flows_in.insert(ctx.pipe.id.clone(), q_in);
        flows_out.insert(ctx.pipe.id.clone(), q_out);
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
        }
    }

    /// Exécute le PDE et retourne (time_s, q_in, q_out, linepack_kg) par pas.
    fn pde_boundary_flow_series(
        net: &GasNetwork,
        demands: &HashMap<String, f64>,
        events: &[TransientEvent],
        cfg: &TransientConfig,
    ) -> Vec<(f64, f64, f64, f64)> {
        use crate::solver::steady_state::{SolverControl, solve_steady_state_with_progress};
        use super::SteadyStateConfig;
        use time_integration::advance_one_step;

        let steady_cfg = SteadyStateConfig {
            gas_composition: cfg.gas_composition,
            ..SteadyStateConfig::default()
        };
        let initial =
            solve_steady_state_with_progress(net, demands, None, steady_cfg, |_| {
                SolverControl::Continue
            })
            .expect("steady init");

        let ordered_pipes = pde_pipe_chain(net).expect("pde chain");
        let mut pipe_contexts = build_pipe_contexts(
            net,
            &ordered_pipes,
            &initial.pressures,
            demands,
            cfg,
        )
        .expect("pipe contexts");

        for ctx in pipe_contexts.iter_mut() {
            system::update_flows(
                &ctx.mesh,
                &mut ctx.state,
                &ctx.pipe,
                &ctx.source,
                &ctx.sink,
                &cfg.gas_composition,
            );
        }

        let mut network_state = net.clone();
        let mut demands = demands.clone();
        let mut ordered_events = events.to_vec();
        ordered_events.sort_by(|a, b| {
            a.time_s()
                .partial_cmp(&b.time_s())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut event_cursor = 0usize;
        while event_cursor < ordered_events.len() && ordered_events[event_cursor].time_s() <= 1e-9
        {
            apply_event(
                &mut network_state,
                &mut demands,
                &ordered_events[event_cursor],
            )
            .expect("event");
            refresh_pde_boundaries(&mut pipe_contexts, &network_state, &demands);
            for ctx in pipe_contexts.iter_mut() {
                system::update_flows(
                    &ctx.mesh,
                    &mut ctx.state,
                    &ctx.pipe,
                    &ctx.source,
                    &ctx.sink,
                    &cfg.gas_composition,
                );
            }
            event_cursor += 1;
        }

        let mut series = Vec::new();
        let lp0 = total_pde_linepack(&pipe_contexts, &cfg.gas_composition);
        let ctx0 = &pipe_contexts[0];
        let n_ifaces = ctx0.mesh.n_cells;
        series.push((
            0.0,
            ctx0.state.flows[0],
            ctx0.state.flows[n_ifaces],
            lp0,
        ));

        let total_steps = (cfg.duration_s / cfg.dt_s).ceil() as usize;

        for step_idx in 1..=total_steps {
            let time_s = ((step_idx as f64) * cfg.dt_s).min(cfg.duration_s);
            while event_cursor < ordered_events.len()
                && ordered_events[event_cursor].time_s() <= time_s + 1e-9
            {
                apply_event(
                    &mut network_state,
                    &mut demands,
                    &ordered_events[event_cursor],
                )
                .expect("event");
                refresh_pde_boundaries(&mut pipe_contexts, &network_state, &demands);
                event_cursor += 1;
            }

            advance_one_step(&mut pipe_contexts, cfg.dt_s, &cfg.gas_composition);
            sync_chain_junctions(&mut pipe_contexts);

            let lp = total_pde_linepack(&pipe_contexts, &cfg.gas_composition);
            let ctx = &pipe_contexts[0];
            let n = ctx.mesh.n_cells;
            series.push((time_s, ctx.state.flows[0], ctx.state.flows[n], lp));
        }

        series
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
