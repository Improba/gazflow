use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task};

use crate::solver::{self, SolverControl, SolverProgress, SolverResult};

use super::{CapacityBoundDto, ContingencyScope, SimulationMode};

const CANCEL_NONE: u8 = 0;
const CANCEL_CLIENT_REQUEST: u8 = 1;
const CANCEL_TIMEOUT: u8 = 2;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    StartSimulation {
        run_id: Option<String>,
        demands: Option<HashMap<String, f64>>,
        options: Option<Box<StartOptions>>,
        capacity_bounds: Option<HashMap<String, CapacityBoundDto>>,
        mode: Option<SimulationMode>,
        equipment_overrides: Option<HashMap<String, crate::graph::EquipmentSpec>>,
    },
    StartTimeseriesSimulation {
        run_id: Option<String>,
        profiles: HashMap<String, solver::DemandProfile>,
        weather: Vec<solver::WeatherStep>,
        options: Option<Box<TimeseriesOptions>>,
    },
    StartContingencySimulation {
        run_id: Option<String>,
        scope: ContingencyScope,
        #[serde(default)]
        demands: Option<HashMap<String, f64>>,
        #[serde(default)]
        custom_cases: Option<Vec<solver::ContingencyCase>>,
    },
    CancelSimulation {
        run_id: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct StartOptions {
    #[serde(default = "default_max_iter")]
    max_iter: usize,
    #[serde(default = "default_tolerance")]
    tolerance: f64,
    #[serde(default = "default_iteration_every")]
    iteration_every: usize,
    #[serde(default = "default_snapshot_every")]
    snapshot_every: usize,
    #[serde(default = "default_timeout_ms")]
    /// 0 = pas de limite de durée.
    timeout_ms: u64,
    #[serde(default)]
    initial_pressures: Option<HashMap<String, f64>>,
    #[serde(default)]
    gas_composition: Option<solver::GasComposition>,
    /// Force continuation de charge (sinon auto selon taille réseau).
    #[serde(default)]
    robust_mode: bool,
    #[serde(default)]
    continuation_scales: Option<Vec<f64>>,
    /// Identifiant de scénario NoVa (ex. `nomination_mild_618`) pour activer les diagnostics
    /// pression (slips, alimentation amont, verdict). Absent → run hors-NoVa inchangé.
    #[serde(default)]
    scenario_id: Option<String>,
}

impl StartOptions {
    fn steady_state_config(&self) -> solver::SteadyStateConfig {
        solver::SteadyStateConfig {
            gas_composition: self.gas_composition.unwrap_or_default(),
            max_iter: self.max_iter,
            tolerance: self.tolerance,
            snapshot_every: self.snapshot_every,
            enable_compressor_outer_loop: true,
            disable_compressor_r2_cap: false,
            accept_partial_solution: false,
        }
    }

    fn solver_preset(&self, node_count: usize) -> solver::SolverPreset {
        solver::preset_from_request(
            node_count,
            self.robust_mode,
            self.max_iter,
            self.tolerance,
            self.timeout_ms,
            self.snapshot_every,
            self.continuation_scales.clone(),
        )
    }
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            max_iter: default_max_iter(),
            tolerance: default_tolerance(),
            iteration_every: default_iteration_every(),
            snapshot_every: default_snapshot_every(),
            timeout_ms: default_timeout_ms(),
            initial_pressures: None,
            gas_composition: None,
            robust_mode: false,
            continuation_scales: None,
            scenario_id: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TimeseriesOptions {
    #[serde(default = "default_ts_warm_start")]
    warm_start: bool,
    #[serde(default = "default_ts_max_iter")]
    max_iter: usize,
    #[serde(default = "default_ts_tolerance")]
    tolerance: f64,
    #[serde(default)]
    gas_composition: Option<solver::GasComposition>,
    #[serde(default)]
    robust_mode: bool,
}

impl TimeseriesOptions {
    fn to_config(&self) -> solver::TimeseriesConfig {
        solver::TimeseriesConfig {
            gas_composition: self.gas_composition.unwrap_or_default(),
            max_iter: self.max_iter,
            tolerance: self.tolerance,
            warm_start: self.warm_start,
            warm_start_max_demand_rel_change: 3.0,
            robust_solver: self.robust_mode,
        }
    }
}

impl Default for TimeseriesOptions {
    fn default() -> Self {
        Self {
            warm_start: default_ts_warm_start(),
            max_iter: default_ts_max_iter(),
            tolerance: default_ts_tolerance(),
            gas_composition: None,
            robust_mode: false,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Started {
        run_id: String,
        seq: u64,
    },
    Iteration {
        run_id: String,
        seq: u64,
        iter: usize,
        residual: f64,
        elapsed_ms: u64,
    },
    ContinuationStep {
        run_id: String,
        seq: u64,
        step: usize,
        total_steps: usize,
        scale: f64,
    },
    Snapshot {
        run_id: String,
        seq: u64,
        iter: usize,
        pressures: HashMap<String, f64>,
        flows: HashMap<String, f64>,
    },
    Converged {
        run_id: String,
        seq: u64,
        result: SolverResult,
        total_ms: u64,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        capacity_violations: Vec<solver::CapacityViolation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        adjusted_demands: Option<HashMap<String, f64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        active_bounds: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        objective_value: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outer_iterations: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        infeasibility_diagnostic: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pressure_slips: Vec<solver::ScenarioPressureSlip>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pressure_margins: Vec<solver::ScenarioPressureMargin>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        boundary_supply: Vec<solver::BoundaryPressureSupplyReport>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        sink_diagnostics: Vec<solver::SinkDiagnostic>,
        #[serde(skip_serializing_if = "Option::is_none")]
        nova_verdict: Option<solver::NovaVerdict>,
    },
    Cancelled {
        run_id: String,
        seq: u64,
        reason: String,
    },
    Error {
        run_id: String,
        seq: u64,
        message: String,
        fatal: bool,
    },
    TimeseriesStarted {
        run_id: String,
        seq: u64,
        total_hours: usize,
    },
    TimeseriesStep {
        run_id: String,
        seq: u64,
        step: solver::TimeseriesStepResult,
    },
    TimeseriesFinished {
        run_id: String,
        seq: u64,
        result: solver::TimeseriesResult,
        total_ms: u64,
    },
    ContingencyStarted {
        run_id: String,
        seq: u64,
        total_cases: usize,
    },
    ContingencyCase {
        run_id: String,
        seq: u64,
        index: usize,
        result: solver::ContingencyResult,
    },
    ContingencyFinished {
        run_id: String,
        seq: u64,
        report: solver::ContingencyReport,
    },
}

pub(super) async fn ws_simulation_handler(
    ws: WebSocketUpgrade,
    State(state): State<super::SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(socket, state))
}

async fn ws_session(socket: WebSocket, state: super::SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(64);

    let mut active_run_id: Option<String> = None;
    let mut cancel_flag: Option<Arc<AtomicBool>> = None;
    let mut cancel_reason: Option<Arc<AtomicU8>> = None;

    loop {
        tokio::select! {
            biased;
            inbound = receiver.next() => {
                let Some(inbound) = inbound else {
                    break;
                };
                let Ok(message) = inbound else {
                    break;
                };

                match message {
                    Message::Text(text) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(ClientMessage::StartSimulation { run_id, demands, options, capacity_bounds, mode, equipment_overrides }) => {
                                if active_run_id.is_some() {
                                    let run_id_for_error = run_id.unwrap_or_else(|| "active-run".to_string());
                                    let _ = tx.send(ServerMessage::Error {
                                        run_id: run_id_for_error,
                                        seq: 0,
                                        message: "a simulation is already running".to_string(),
                                        fatal: false,
                                    }).await;
                                    continue;
                                }

                                let run_id = run_id.unwrap_or_else(default_run_id);
                                let (mut network, network_id, default_demands) = {
                                    let network = state
                                        .network
                                        .read()
                                        .expect("network lock should not be poisoned")
                                        .as_ref()
                                        .clone();
                                    let network_id = state
                                        .active_dataset
                                        .read()
                                        .expect("active dataset lock should not be poisoned")
                                        .clone();
                                    let default_demands = state
                                        .default_demands
                                        .read()
                                        .expect("default demands lock should not be poisoned")
                                        .clone();
                                    (network, network_id, default_demands)
                                };
                                if let Some(ref overrides) = equipment_overrides {
                                    network.apply_equipment_overrides(overrides);
                                }
                                let network = Arc::new(network);
                                let demands = demands.unwrap_or_else(|| (*default_demands).clone());
                                let mut options = options.map(|o| *o).unwrap_or_default();
                                if options.gas_composition.is_none() {
                                    options.gas_composition =
                                        Some(super::active_gas_composition(&state));
                                }
                                let scenario = match options.scenario_id.as_deref() {
                                    Some(scenario_id) => {
                                        match super::load_scenario_demands_by_id(
                                            &state,
                                            &network_id,
                                            scenario_id,
                                        ) {
                                            Ok(mut sc) => {
                                                crate::gaslib::enrich_scenario_with_balance_hub(&network, &mut sc);
                                                Some(sc)
                                            }
                                            Err(err) => {
                                                let _ = tx.send(ServerMessage::Error {
                                                    run_id: run_id.clone(),
                                                    seq: 0,
                                                    message: err,
                                                    fatal: false,
                                                }).await;
                                                None
                                            }
                                        }
                                    }
                                    None => None,
                                };
                                let permit = match state.simulation_slots.clone().try_acquire_owned() {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        let _ = tx.send(ServerMessage::Error {
                                            run_id: run_id.clone(),
                                            seq: 0,
                                            message: "simulation capacity reached, retry later".to_string(),
                                            fatal: false,
                                        }).await;
                                        continue;
                                    }
                                };
                                let run_cancel = Arc::new(AtomicBool::new(false));
                                let run_cancel_reason = Arc::new(AtomicU8::new(CANCEL_NONE));

                                active_run_id = Some(run_id.clone());
                                cancel_flag = Some(run_cancel.clone());
                                cancel_reason = Some(run_cancel_reason.clone());

                                let _ = tx.send(ServerMessage::Started {
                                    run_id: run_id.clone(),
                                    seq: 1,
                                }).await;

                                let tx_for_solver = tx.clone();
                                let state_for_solver = state.clone();
                                task::spawn_blocking(move || {
                                    let _permit = permit;
                                    run_solver_stream(SolverStreamContext {
                                        state: state_for_solver,
                                        network,
                                        network_id,
                                        demands,
                                        options,
                                        run_id,
                                        cancel_flag: run_cancel,
                                        cancel_reason: run_cancel_reason,
                                        tx: tx_for_solver,
                                        capacity_bounds,
                                        mode,
                                        scenario,
                                    });
                                });
                            }
                            Ok(ClientMessage::StartTimeseriesSimulation {
                                run_id,
                                profiles,
                                weather,
                                options,
                            }) => {
                                if active_run_id.is_some() {
                                    let run_id_for_error =
                                        run_id.unwrap_or_else(|| "active-run".to_string());
                                    let _ = tx
                                        .send(ServerMessage::Error {
                                            run_id: run_id_for_error,
                                            seq: 0,
                                            message: "a simulation is already running".to_string(),
                                            fatal: false,
                                        })
                                        .await;
                                    continue;
                                }

                                let run_id = run_id.unwrap_or_else(default_run_id);
                                let network = {
                                    state
                                        .network
                                        .read()
                                        .expect("network lock should not be poisoned")
                                        .as_ref()
                                        .clone()
                                };
                                let network = Arc::new(network);
                                let mut options = options.map(|o| *o).unwrap_or_default();
                                if options.gas_composition.is_none() {
                                    options.gas_composition =
                                        Some(super::active_gas_composition(&state));
                                }
                                let total_hours = weather.len();
                                let permit = match state.simulation_slots.clone().try_acquire_owned() {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        let _ = tx
                                            .send(ServerMessage::Error {
                                                run_id: run_id.clone(),
                                                seq: 0,
                                                message:
                                                    "simulation capacity reached, retry later"
                                                        .to_string(),
                                                fatal: false,
                                            })
                                            .await;
                                        continue;
                                    }
                                };
                                let run_cancel = Arc::new(AtomicBool::new(false));
                                let run_cancel_reason = Arc::new(AtomicU8::new(CANCEL_NONE));

                                active_run_id = Some(run_id.clone());
                                cancel_flag = Some(run_cancel.clone());
                                cancel_reason = Some(run_cancel_reason.clone());

                                let _ = tx
                                    .send(ServerMessage::TimeseriesStarted {
                                        run_id: run_id.clone(),
                                        seq: 1,
                                        total_hours,
                                    })
                                    .await;

                                let tx_for_solver = tx.clone();
                                let state_for_solver = state.clone();
                                task::spawn_blocking(move || {
                                    let _permit = permit;
                                    run_timeseries_stream(TimeseriesStreamContext {
                                        state: state_for_solver,
                                        network,
                                        profiles,
                                        weather,
                                        options,
                                        run_id,
                                        cancel_flag: run_cancel,
                                        cancel_reason: run_cancel_reason,
                                        tx: tx_for_solver,
                                    });
                                });
                            }
                            Ok(ClientMessage::StartContingencySimulation {
                                run_id,
                                scope,
                                demands,
                                custom_cases,
                            }) => {
                                if active_run_id.is_some() {
                                    let run_id_for_error =
                                        run_id.unwrap_or_else(|| "active-run".to_string());
                                    let _ = tx
                                        .send(ServerMessage::Error {
                                            run_id: run_id_for_error,
                                            seq: 0,
                                            message: "a simulation is already running".to_string(),
                                            fatal: false,
                                        })
                                        .await;
                                    continue;
                                }

                                let run_id = run_id.unwrap_or_else(default_run_id);
                                let network = {
                                    state
                                        .network
                                        .read()
                                        .expect("network lock should not be poisoned")
                                        .as_ref()
                                        .clone()
                                };
                                let default_demands = state
                                    .default_demands
                                    .read()
                                    .expect("default demands lock should not be poisoned")
                                    .clone();
                                let demands = demands.unwrap_or_else(|| (*default_demands).clone());
                                let cases =
                                    match super::resolve_contingency_cases(&network, scope, custom_cases)
                                    {
                                        Ok(cases) => cases,
                                        Err((_, api_error)) => {
                                            let _ = tx
                                                .send(ServerMessage::Error {
                                                    run_id: run_id.clone(),
                                                    seq: 0,
                                                    message: api_error.0.error,
                                                    fatal: false,
                                                })
                                                .await;
                                            continue;
                                        }
                                    };

                                let total_cases = cases.len();
                                let permit = match state.simulation_slots.clone().try_acquire_owned() {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        let _ = tx
                                            .send(ServerMessage::Error {
                                                run_id: run_id.clone(),
                                                seq: 0,
                                                message:
                                                    "simulation capacity reached, retry later"
                                                        .to_string(),
                                                fatal: false,
                                            })
                                            .await;
                                        continue;
                                    }
                                };
                                let run_cancel = Arc::new(AtomicBool::new(false));
                                let run_cancel_reason = Arc::new(AtomicU8::new(CANCEL_NONE));

                                active_run_id = Some(run_id.clone());
                                cancel_flag = Some(run_cancel.clone());
                                cancel_reason = Some(run_cancel_reason.clone());

                                let _ = tx
                                    .send(ServerMessage::ContingencyStarted {
                                        run_id: run_id.clone(),
                                        seq: 1,
                                        total_cases,
                                    })
                                    .await;

                                let tx_for_solver = tx.clone();
                                let state_for_solver = state.clone();
                                task::spawn_blocking(move || {
                                    let _permit = permit;
                                    run_contingency_stream(ContingencyStreamContext {
                                        state: state_for_solver,
                                        network: Arc::new(network),
                                        demands,
                                        cases,
                                        run_id,
                                        cancel_flag: run_cancel,
                                        cancel_reason: run_cancel_reason,
                                        tx: tx_for_solver,
                                    });
                                });
                            }
                            Ok(ClientMessage::CancelSimulation { run_id }) => {
                                if let (Some(active_id), Some(flag), Some(reason)) =
                                    (&active_run_id, &cancel_flag, &cancel_reason)
                                    && run_id
                                        .as_deref()
                                        .map(|rid| rid == active_id)
                                        .unwrap_or(true)
                                {
                                    reason.store(CANCEL_CLIENT_REQUEST, Ordering::Relaxed);
                                    flag.store(true, Ordering::Relaxed);
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(ServerMessage::Error {
                                    run_id: active_run_id.clone().unwrap_or_else(|| "no-run".to_string()),
                                    seq: 0,
                                    message: format!("invalid client message: {err}"),
                                    fatal: false,
                                }).await;
                            }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            outbound = rx.recv() => {
                let Some(outbound) = outbound else {
                    break;
                };
                let is_terminal = matches!(
                    outbound,
                    ServerMessage::Converged { .. }
                        | ServerMessage::Cancelled { .. }
                        | ServerMessage::Error { fatal: true, .. }
                        | ServerMessage::TimeseriesFinished { .. }
                        | ServerMessage::ContingencyFinished { .. }
                );

                let Ok(text) = serde_json::to_string(&outbound) else {
                    break;
                };
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }

                if is_terminal {
                    active_run_id = None;
                    cancel_flag = None;
                    cancel_reason = None;
                }
            }
        }
    }
}

struct SolverStreamContext {
    state: super::SharedState,
    network: Arc<crate::graph::GasNetwork>,
    network_id: String,
    demands: HashMap<String, f64>,
    options: StartOptions,
    run_id: String,
    cancel_flag: Arc<AtomicBool>,
    cancel_reason: Arc<AtomicU8>,
    tx: mpsc::Sender<ServerMessage>,
    capacity_bounds: Option<HashMap<String, CapacityBoundDto>>,
    mode: Option<SimulationMode>,
    scenario: Option<crate::gaslib::ScenarioDemands>,
}

fn run_solver_stream(ctx: SolverStreamContext) {
    let SolverStreamContext {
        state,
        network,
        network_id,
        demands,
        options,
        run_id,
        cancel_flag,
        cancel_reason,
        tx,
        capacity_bounds,
        mode,
        scenario,
    } = ctx;
    let steady_config = options.steady_state_config();
    let started = Instant::now();
    let timeout = Duration::from_millis(options.timeout_ms);
    let seq = std::sync::atomic::AtomicU64::new(1);

    let progress_cb = |progress: SolverProgress| -> SolverControl {
        if options.timeout_ms > 0 && started.elapsed() >= timeout {
            cancel_reason.store(CANCEL_TIMEOUT, Ordering::Relaxed);
            cancel_flag.store(true, Ordering::Relaxed);
        }
        if cancel_flag.load(Ordering::Relaxed) {
            return SolverControl::Cancel;
        }

        if progress.iter == 1 || progress.iter.is_multiple_of(options.iteration_every.max(1)) {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.blocking_send(ServerMessage::Iteration {
                run_id: run_id.clone(),
                seq: s,
                iter: progress.iter,
                residual: progress.residual,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }

        if let (Some(pressures), Some(flows)) = (progress.pressures, progress.flows) {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let snapshot = ServerMessage::Snapshot {
                run_id: run_id.clone(),
                seq: s,
                iter: progress.iter,
                pressures,
                flows,
            };
            let _ = tx.try_send(snapshot);
        }

        SolverControl::Continue
    };

    enum SolveOutcome {
        Normal(anyhow::Result<SolverResult>),
        Constrained(anyhow::Result<solver::ConstrainedSolverResult>),
        Check {
            result: anyhow::Result<SolverResult>,
            bounds: solver::CapacityBounds,
        },
    }

    let preset_for_routing = options.solver_preset(network.node_count());
    let network_path = state.data_dir.join(format!("{network_id}.net"));
    let mut network_prepared = (*network).clone();
    let routing_outcome = state.rayon_pool.install(|| {
        crate::gaslib::resolve_and_apply_cdf_routing(
            &mut network_prepared,
            &network_path,
            &demands,
            &preset_for_routing,
        )
    });
    if let Err(err) = routing_outcome {
        let _ = tx.blocking_send(ServerMessage::Error {
            run_id: run_id.clone(),
            seq: 1,
            message: format!("routage transport `.cdf`: {err:#}"),
            fatal: true,
        });
        return;
    }

    // Diagnostics NoVa : évalue le résultat contre les bornes contractuelles scénario.
    // Renvoie None si aucun scénario n'est fourni (run hors-NoVa, comportement inchangé).
    let compute_nova = |result: &SolverResult| -> Option<solver::NovaDiagnostics> {
        scenario
            .as_ref()
            .map(|sc| solver::compute_nova_diagnostics(&network_prepared, sc, result))
    };

    let outcome = match (&capacity_bounds, &mode) {
        (Some(api_bounds), Some(SimulationMode::Optimize)) => {
            let bounds = super::api_bounds_to_solver(api_bounds, &network_prepared);
            let result = state.rayon_pool.install(|| {
                solver::capacity::solve_steady_state_constrained(
                    &network_prepared,
                    &demands,
                    &bounds,
                    options.initial_pressures.as_ref(),
                    solver::capacity::ConstrainedSolverConfig {
                        inner_max_iter: options.max_iter,
                        inner_tolerance: options.tolerance,
                        inner_snapshot_every: options.snapshot_every,
                        inner_gas_composition: steady_config.gas_composition,
                        ..Default::default()
                    },
                    |cp| progress_cb(cp.inner_progress),
                )
            });
            SolveOutcome::Constrained(result)
        }
        (Some(api_bounds), _) => {
            let bounds = super::api_bounds_to_solver(api_bounds, &network_prepared);
            let preset = options.solver_preset(network_prepared.node_count());
            let gas = steady_config.gas_composition;
            let result = state.rayon_pool.install(|| {
                solver::solve_steady_state_with_preset(
                    &network_prepared,
                    &demands,
                    options.initial_pressures.as_ref(),
                    &preset,
                    gas,
                    &progress_cb,
                    Some(|ev: solver::ContinuationStepEvent| {
                        let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = tx.blocking_send(ServerMessage::ContinuationStep {
                            run_id: run_id.clone(),
                            seq: s,
                            step: ev.step,
                            total_steps: ev.total_steps,
                            scale: ev.scale,
                        });
                    }),
                )
            });
            SolveOutcome::Check { result, bounds }
        }
        _ => {
            let preset = options.solver_preset(network_prepared.node_count());
            let gas = steady_config.gas_composition;
            let result = state.rayon_pool.install(|| {
                solver::solve_steady_state_with_preset(
                    &network_prepared,
                    &demands,
                    options.initial_pressures.as_ref(),
                    &preset,
                    gas,
                    &progress_cb,
                    Some(|ev: solver::ContinuationStepEvent| {
                        let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = tx.blocking_send(ServerMessage::ContinuationStep {
                            run_id: run_id.clone(),
                            seq: s,
                            step: ev.step,
                            total_steps: ev.total_steps,
                            scale: ev.scale,
                        });
                    }),
                )
            });
            SolveOutcome::Normal(result)
        }
    };

    match outcome {
        SolveOutcome::Normal(Ok(final_result)) => {
            super::export::store_export_record(
                &state,
                super::export::new_export_record(
                    run_id.clone(),
                    network_id,
                    &network_prepared,
                    demands.clone(),
                    final_result.clone(),
                    started.elapsed().as_millis() as u64,
                ),
            );
            let tol_m3s = options.tolerance;
            let (pressure_slips, pressure_margins, boundary_supply, sink_diagnostics, nova_verdict) =
                match compute_nova(&final_result) {
                    Some(d) => {
                        let converged = final_result.residual <= tol_m3s;
                        let v = solver::nova_verdict(&d, converged, tol_m3s, &final_result);
                        (
                            d.pressure_slips,
                            d.pressure_margins,
                            d.boundary_supply,
                            d.sink_diagnostics,
                            Some(v),
                        )
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None),
                };
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.blocking_send(ServerMessage::Converged {
                run_id,
                seq: s,
                result: final_result,
                total_ms: started.elapsed().as_millis() as u64,
                capacity_violations: Vec::new(),
                adjusted_demands: None,
                active_bounds: None,
                objective_value: None,
                outer_iterations: None,
                infeasibility_diagnostic: None,
                pressure_slips,
                pressure_margins,
                boundary_supply,
                sink_diagnostics,
                nova_verdict,
            });
        }
        SolveOutcome::Check {
            result: Ok(final_result),
            bounds,
        } => {
            let violations = solver::capacity::check_capacity_violations(
                &network_prepared,
                &final_result,
                &demands,
                &bounds,
            );
            super::export::store_export_record(
                &state,
                super::export::new_export_record(
                    run_id.clone(),
                    network_id,
                    &network_prepared,
                    demands.clone(),
                    final_result.clone(),
                    started.elapsed().as_millis() as u64,
                ),
            );
            let tol_m3s = options.tolerance;
            let (pressure_slips, pressure_margins, boundary_supply, sink_diagnostics, nova_verdict) =
                match compute_nova(&final_result) {
                    Some(d) => {
                        let converged = final_result.residual <= tol_m3s;
                        let v = solver::nova_verdict(&d, converged, tol_m3s, &final_result);
                        (
                            d.pressure_slips,
                            d.pressure_margins,
                            d.boundary_supply,
                            d.sink_diagnostics,
                            Some(v),
                        )
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None),
                };
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.blocking_send(ServerMessage::Converged {
                run_id,
                seq: s,
                result: final_result,
                total_ms: started.elapsed().as_millis() as u64,
                capacity_violations: violations,
                adjusted_demands: None,
                active_bounds: None,
                objective_value: None,
                outer_iterations: None,
                infeasibility_diagnostic: None,
                pressure_slips,
                pressure_margins,
                boundary_supply,
                sink_diagnostics,
                nova_verdict,
            });
        }
        SolveOutcome::Constrained(Ok(constrained)) => {
            let total_ms = started.elapsed().as_millis() as u64;
            let ws_result = SolverResult::from_core(
                constrained.pressures.clone(),
                constrained.flows.clone(),
                constrained.iterations,
                constrained.residual,
            );
            let ws_violations = constrained.capacity_violations.clone();
            let ws_adjusted = constrained.adjusted_demands.clone();
            let ws_active = constrained.active_bounds.clone();
            let ws_obj = constrained.objective_value;
            let ws_outer = constrained.outer_iterations;
            let ws_diag = constrained.infeasibility_diagnostic.clone();
            let tol_m3s = options.tolerance;
            let (pressure_slips, pressure_margins, boundary_supply, sink_diagnostics, nova_verdict) =
                match compute_nova(&ws_result) {
                    Some(d) => {
                        let converged = ws_result.residual <= tol_m3s;
                        let v = solver::nova_verdict(&d, converged, tol_m3s, &ws_result);
                        (
                            d.pressure_slips,
                            d.pressure_margins,
                            d.boundary_supply,
                            d.sink_diagnostics,
                            Some(v),
                        )
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None),
                };
            super::export::store_export_record(
                &state,
                super::export::new_constrained_export_record(
                    run_id.clone(),
                    network_id,
                    &network,
                    demands.clone(),
                    constrained,
                    total_ms,
                ),
            );
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.blocking_send(ServerMessage::Converged {
                run_id,
                seq: s,
                result: ws_result,
                total_ms,
                capacity_violations: ws_violations,
                adjusted_demands: Some(ws_adjusted),
                active_bounds: Some(ws_active),
                objective_value: Some(ws_obj),
                outer_iterations: Some(ws_outer),
                infeasibility_diagnostic: ws_diag,
                pressure_slips,
                pressure_margins,
                boundary_supply,
                sink_diagnostics,
                nova_verdict,
            });
        }
        SolveOutcome::Normal(Err(err))
        | SolveOutcome::Check {
            result: Err(err), ..
        }
        | SolveOutcome::Constrained(Err(err)) => {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            if cancel_flag.load(Ordering::Relaxed) {
                let reason = match cancel_reason.load(Ordering::Relaxed) {
                    CANCEL_CLIENT_REQUEST => "client_request",
                    CANCEL_TIMEOUT => "timeout",
                    _ => "cancelled",
                };
                let _ = tx.blocking_send(ServerMessage::Cancelled {
                    run_id,
                    seq: s,
                    reason: reason.to_string(),
                });
            } else if err.to_string().contains("did not converge") {
                let _ = tx.blocking_send(ServerMessage::Cancelled {
                    run_id,
                    seq: s,
                    reason: "diverged".to_string(),
                });
            } else {
                let _ = tx.blocking_send(ServerMessage::Error {
                    run_id,
                    seq: s,
                    message: err.to_string(),
                    fatal: true,
                });
            }
        }
    }
}

struct TimeseriesStreamContext {
    state: super::SharedState,
    network: Arc<crate::graph::GasNetwork>,
    profiles: HashMap<String, solver::DemandProfile>,
    weather: Vec<solver::WeatherStep>,
    options: TimeseriesOptions,
    run_id: String,
    cancel_flag: Arc<AtomicBool>,
    cancel_reason: Arc<AtomicU8>,
    tx: mpsc::Sender<ServerMessage>,
}

fn run_timeseries_stream(ctx: TimeseriesStreamContext) {
    let TimeseriesStreamContext {
        state,
        network,
        profiles,
        weather,
        options,
        run_id,
        cancel_flag,
        cancel_reason,
        tx,
    } = ctx;
    let started = Instant::now();
    let config = options.to_config();
    let seq = std::sync::atomic::AtomicU64::new(1);

    let progress_cb = |step: &solver::TimeseriesStepResult| -> solver::TimeseriesControl {
        if cancel_flag.load(Ordering::Relaxed) {
            return solver::TimeseriesControl::Cancel;
        }
        let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = tx.blocking_send(ServerMessage::TimeseriesStep {
            run_id: run_id.clone(),
            seq: s,
            step: step.clone(),
        });
        solver::TimeseriesControl::Continue
    };

    let result = state.rayon_pool.install(|| {
        solver::simulate_timeseries_with_progress(
            &network,
            &profiles,
            &weather,
            &config,
            Some(&progress_cb),
        )
    });

    match result {
        Ok(final_result) => {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.blocking_send(ServerMessage::TimeseriesFinished {
                run_id,
                seq: s,
                result: final_result,
                total_ms: started.elapsed().as_millis() as u64,
            });
        }
        Err(err) => {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            if cancel_flag.load(Ordering::Relaxed) {
                let reason = match cancel_reason.load(Ordering::Relaxed) {
                    CANCEL_CLIENT_REQUEST => "client_request",
                    CANCEL_TIMEOUT => "timeout",
                    _ => "cancelled",
                };
                let _ = tx.blocking_send(ServerMessage::Cancelled {
                    run_id,
                    seq: s,
                    reason: reason.to_string(),
                });
            } else {
                let _ = tx.blocking_send(ServerMessage::Error {
                    run_id,
                    seq: s,
                    message: err.to_string(),
                    fatal: true,
                });
            }
        }
    }
}

struct ContingencyStreamContext {
    state: super::SharedState,
    network: Arc<crate::graph::GasNetwork>,
    demands: HashMap<String, f64>,
    cases: Vec<solver::ContingencyCase>,
    run_id: String,
    cancel_flag: Arc<AtomicBool>,
    cancel_reason: Arc<AtomicU8>,
    tx: mpsc::Sender<ServerMessage>,
}

fn run_contingency_stream(ctx: ContingencyStreamContext) {
    let ContingencyStreamContext {
        state,
        network,
        demands,
        cases,
        run_id,
        cancel_flag,
        cancel_reason,
        tx,
    } = ctx;
    let seq = std::sync::atomic::AtomicU64::new(1);
    let config = solver::SteadyStateConfig {
        gas_composition: super::active_gas_composition(&state),
        max_iter: default_max_iter(),
        tolerance: default_tolerance(),
        ..solver::SteadyStateConfig::default()
    };

    let mut results = Vec::with_capacity(cases.len());
    for (idx, case) in cases.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
            let reason = match cancel_reason.load(Ordering::Relaxed) {
                CANCEL_CLIENT_REQUEST => "client_request",
                CANCEL_TIMEOUT => "timeout",
                _ => "cancelled",
            };
            let _ = tx.blocking_send(ServerMessage::Cancelled {
                run_id,
                seq: s,
                reason: reason.to_string(),
            });
            return;
        }

        let result = state
            .rayon_pool
            .install(|| solver::evaluate_contingency_case(&network, &demands, case, config));
        results.push(result.clone());
        let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = tx.blocking_send(ServerMessage::ContingencyCase {
            run_id: run_id.clone(),
            seq: s,
            index: idx + 1,
            result,
        });
    }

    let report = solver::finalize_contingency_report(results);
    let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = tx.blocking_send(ServerMessage::ContingencyFinished {
        run_id,
        seq: s,
        report,
    });
}

fn default_max_iter() -> usize {
    1000
}

fn default_tolerance() -> f64 {
    5e-4
}

fn default_snapshot_every() -> usize {
    5
}

fn default_iteration_every() -> usize {
    1
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn default_ts_warm_start() -> bool {
    true
}

fn default_ts_max_iter() -> usize {
    800
}

fn default_ts_tolerance() -> f64 {
    1e-3
}

fn default_run_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("run-{ts}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

    use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};

    fn test_router_with_capacity(max_concurrent_simulations: usize) -> axum::Router {
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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        super::super::create_router_with_limits(net, defaults, max_concurrent_simulations)
    }

    fn test_router() -> axum::Router {
        test_router_with_capacity(4)
    }

    fn test_router_with_runtime_limits(
        max_concurrent_simulations: usize,
        rayon_threads: usize,
    ) -> axum::Router {
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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        super::super::create_router_with_runtime_limits(
            net,
            defaults,
            max_concurrent_simulations,
            rayon_threads,
        )
    }

    fn test_router_with_isolated() -> axum::Router {
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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
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
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_pipe(Pipe {
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });

        let mut defaults = HashMap::new();
        defaults.insert("sink".to_string(), -5.0);
        super::super::create_router(net, defaults)
    }

    async fn spawn_test_server(app: axum::Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn test_ws_start_simulation() {
        let (addr, _server) = spawn_test_server(test_router()).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws, _) = connect_async(url).await.expect("connect ws");

        let start = serde_json::json!({
            "type": "start_simulation",
            "run_id": "r1",
            "options": {"max_iter": 200, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
        });
        ws.send(WsMessage::Text(start.to_string()))
            .await
            .expect("send start");

        let mut got_iteration = false;
        let mut got_converged = false;
        for _ in 0..200 {
            let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            match v.get("type").and_then(|x| x.as_str()) {
                Some("iteration") => got_iteration = true,
                Some("converged") => {
                    got_converged = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(
            got_iteration,
            "should receive at least one iteration message"
        );
        assert!(got_converged, "should receive converged message");
    }

    #[tokio::test]
    async fn test_ws_cancel_simulation() {
        let (addr, _server) = spawn_test_server(test_router_with_isolated()).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws, _) = connect_async(url).await.expect("connect ws");

        let start = serde_json::json!({
            "type": "start_simulation",
            "run_id": "r2",
            "demands": {"sink": -5.0, "isolated": -1.0},
            "options": {"max_iter": 1_000_000, "tolerance": 1e-12, "iteration_every": 1000, "snapshot_every": 1000, "timeout_ms": 60_000}
        });
        ws.send(WsMessage::Text(start.to_string()))
            .await
            .expect("send start");

        let mut got_cancelled = false;
        for _ in 0..200 {
            let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            if v.get("type").and_then(|x| x.as_str()) == Some("started") {
                ws.send(WsMessage::Text(
                    serde_json::json!({"type":"cancel_simulation","run_id":"r2"}).to_string(),
                ))
                .await
                .expect("send cancel");
                continue;
            }
            if v.get("type").and_then(|x| x.as_str()) == Some("cancelled") {
                let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("");
                got_cancelled = reason == "client_request";
                break;
            }
        }
        assert!(got_cancelled, "should receive cancelled(client_request)");
    }

    #[tokio::test]
    async fn test_ws_timeout_diverged() {
        let (addr, _server) = spawn_test_server(test_router()).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws, _) = connect_async(url).await.expect("connect ws");

        let start = serde_json::json!({
            "type": "start_simulation",
            "run_id": "r3",
            "options": {"max_iter": 10_000, "tolerance": 1e-12, "iteration_every": 1000, "snapshot_every": 1, "timeout_ms": 1}
        });
        ws.send(WsMessage::Text(start.to_string()))
            .await
            .expect("send start");

        let mut got_timeout = false;
        for _ in 0..20 {
            let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            if v.get("type").and_then(|x| x.as_str()) == Some("cancelled") {
                let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("");
                got_timeout = reason == "timeout";
                break;
            }
        }
        assert!(got_timeout, "should receive cancelled(timeout)");
    }

    #[tokio::test]
    async fn test_semaphore_rejects_overflow() {
        let (addr, _server) = spawn_test_server(test_router_with_capacity(1)).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws1, _) = connect_async(url.clone()).await.expect("connect ws1");
        let (mut ws2, _) = connect_async(url).await.expect("connect ws2");

        let start1 = serde_json::json!({
            "type": "start_simulation",
            "run_id": "cap-1",
            "options": {"max_iter": 1_000_000, "tolerance": 1e-12, "iteration_every": 1000, "snapshot_every": 1000, "timeout_ms": 60_000}
        });
        ws1.send(WsMessage::Text(start1.to_string()))
            .await
            .expect("send start1");

        let mut ws1_started = false;
        for _ in 0..50 {
            let Some(Ok(WsMessage::Text(txt))) = ws1.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            if v.get("type").and_then(|x| x.as_str()) == Some("started") {
                ws1_started = true;
                break;
            }
        }
        assert!(ws1_started, "first simulation should start");

        let start2 = serde_json::json!({
            "type": "start_simulation",
            "run_id": "cap-2",
            "options": {"max_iter": 200, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 5_000}
        });
        ws2.send(WsMessage::Text(start2.to_string()))
            .await
            .expect("send start2");

        let mut got_capacity_error = false;
        for _ in 0..50 {
            let Some(Ok(WsMessage::Text(txt))) = ws2.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            if v.get("type").and_then(|x| x.as_str()) == Some("error") {
                let message = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
                got_capacity_error = message.contains("capacity");
                break;
            }
        }
        assert!(
            got_capacity_error,
            "second simulation should be rejected when semaphore is full"
        );

        ws1.send(WsMessage::Text(
            serde_json::json!({"type":"cancel_simulation","run_id":"cap-1"}).to_string(),
        ))
        .await
        .expect("send cancel cap-1");
    }

    #[tokio::test]
    async fn test_ws_concurrent_with_single_rayon_thread_no_deadlock() {
        let (addr, _server) = spawn_test_server(test_router_with_runtime_limits(2, 1)).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws1, _) = connect_async(url.clone()).await.expect("connect ws1");
        let (mut ws2, _) = connect_async(url).await.expect("connect ws2");

        ws1.send(WsMessage::Text(
            serde_json::json!({
                "type": "start_simulation",
                "run_id": "rt-1",
                "options": {"max_iter": 400, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
            })
            .to_string(),
        ))
        .await
        .expect("send start ws1");
        ws2.send(WsMessage::Text(
            serde_json::json!({
                "type": "start_simulation",
                "run_id": "rt-2",
                "options": {"max_iter": 400, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
            })
            .to_string(),
        ))
        .await
        .expect("send start ws2");

        let mut conv1 = false;
        let mut conv2 = false;
        for _ in 0..300 {
            if !conv1 {
                if let Ok(Some(Ok(WsMessage::Text(txt)))) =
                    tokio::time::timeout(Duration::from_millis(100), ws1.next()).await
                {
                    let msg: serde_json::Value = serde_json::from_str(&txt).expect("json ws1");
                    conv1 = msg.get("type").and_then(|x| x.as_str()) == Some("converged");
                }
            }
            if !conv2 {
                if let Ok(Some(Ok(WsMessage::Text(txt)))) =
                    tokio::time::timeout(Duration::from_millis(100), ws2.next()).await
                {
                    let msg: serde_json::Value = serde_json::from_str(&txt).expect("json ws2");
                    conv2 = msg.get("type").and_then(|x| x.as_str()) == Some("converged");
                }
            }
            if conv1 && conv2 {
                break;
            }
        }

        assert!(conv1, "first run should converge");
        assert!(conv2, "second run should converge");
    }

    #[test]
    fn test_parse_start_timeseries_simulation_message() {
        let weather: Vec<_> = (0u8..3)
            .map(|hour| serde_json::json!({ "hour": hour, "t_ext_c": -3.0 }))
            .collect();
        let raw = serde_json::json!({
            "type": "start_timeseries_simulation",
            "run_id": "ts-1",
            "profiles": {
                "sink": {
                    "q0_m3h": 45.0,
                    "alpha_m3h_per_c": 7.5,
                    "t_threshold_c": 17.0,
                    "category": "residential"
                }
            },
            "weather": weather,
            "options": {
                "warm_start": true,
                "max_iter": 400,
                "tolerance": 1e-3
            }
        });
        let msg: ClientMessage =
            serde_json::from_value(raw).expect("parse start_timeseries_simulation");
        match msg {
            ClientMessage::StartTimeseriesSimulation {
                run_id,
                profiles,
                weather,
                options,
            } => {
                assert_eq!(run_id.as_deref(), Some("ts-1"));
                assert_eq!(profiles.len(), 1);
                assert_eq!(weather.len(), 3);
                let opts = options.expect("options present").to_config();
                assert!(opts.warm_start);
                assert_eq!(opts.max_iter, 400);
            }
            _ => panic!("expected StartTimeseriesSimulation"),
        }
    }

    #[test]
    fn test_parse_start_contingency_simulation_message() {
        let raw = serde_json::json!({
            "type": "start_contingency_simulation",
            "run_id": "ct-1",
            "scope": "sources_only",
            "demands": { "sink": -8.0 }
        });
        let msg: ClientMessage =
            serde_json::from_value(raw).expect("parse start_contingency_simulation");
        match msg {
            ClientMessage::StartContingencySimulation {
                run_id,
                scope,
                demands,
                custom_cases,
            } => {
                assert_eq!(run_id.as_deref(), Some("ct-1"));
                assert!(matches!(scope, super::ContingencyScope::SourcesOnly));
                assert_eq!(
                    demands.as_ref().and_then(|d| d.get("sink")).copied(),
                    Some(-8.0)
                );
                assert!(custom_cases.is_none());
            }
            _ => panic!("expected StartContingencySimulation"),
        }
    }

    #[tokio::test]
    async fn test_ws_start_timeseries_simulation() {
        let (addr, _server) = spawn_test_server(test_router()).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws, _) = connect_async(url).await.expect("connect ws");

        let weather: Vec<_> = (0u8..4)
            .map(|hour| serde_json::json!({ "hour": hour, "t_ext_c": -3.0 }))
            .collect();
        let start = serde_json::json!({
            "type": "start_timeseries_simulation",
            "run_id": "ts-ws-1",
            "profiles": {
                "sink": {
                    "q0_m3h": 45.0,
                    "alpha_m3h_per_c": 7.5,
                    "t_threshold_c": 17.0,
                    "category": "residential"
                }
            },
            "weather": weather,
            "options": {"warm_start": true, "max_iter": 400, "tolerance": 1e-3}
        });
        ws.send(WsMessage::Text(start.to_string()))
            .await
            .expect("send start");

        let mut got_started = false;
        let mut step_count = 0;
        let mut got_finished = false;
        for _ in 0..100 {
            let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            match v.get("type").and_then(|x| x.as_str()) {
                Some("timeseries_started") => {
                    got_started = true;
                    assert_eq!(v.get("total_hours").and_then(|x| x.as_u64()), Some(4));
                }
                Some("timeseries_step") => step_count += 1,
                Some("timeseries_finished") => {
                    got_finished = true;
                    let steps = v
                        .pointer("/result/steps")
                        .and_then(|x| x.as_array())
                        .expect("result.steps");
                    assert_eq!(steps.len(), 4);
                    break;
                }
                _ => {}
            }
        }

        assert!(got_started, "should receive timeseries_started");
        assert_eq!(step_count, 4, "should receive one step per hour");
        assert!(got_finished, "should receive timeseries_finished");
    }

    #[tokio::test]
    async fn test_ws_cancel_timeseries_simulation() {
        let (addr, _server) = spawn_test_server(test_router()).await;
        let url = format!("ws://{addr}/api/ws/sim");
        let (mut ws, _) = connect_async(url).await.expect("connect ws");

        let weather: Vec<_> = (0u8..24)
            .map(|hour| serde_json::json!({ "hour": hour, "t_ext_c": -3.0 }))
            .collect();
        let start = serde_json::json!({
            "type": "start_timeseries_simulation",
            "run_id": "ts-cancel",
            "profiles": {
                "sink": {
                    "q0_m3h": 45.0,
                    "alpha_m3h_per_c": 7.5,
                    "t_threshold_c": 17.0,
                    "category": "residential"
                }
            },
            "weather": weather,
            "options": {"warm_start": true, "max_iter": 400, "tolerance": 1e-3}
        });
        ws.send(WsMessage::Text(start.to_string()))
            .await
            .expect("send start");

        let mut got_cancelled = false;
        for _ in 0..200 {
            let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&txt).expect("json");
            if v.get("type").and_then(|x| x.as_str()) == Some("timeseries_started") {
                ws.send(WsMessage::Text(
                    serde_json::json!({"type":"cancel_simulation","run_id":"ts-cancel"})
                        .to_string(),
                ))
                .await
                .expect("send cancel");
                continue;
            }
            if v.get("type").and_then(|x| x.as_str()) == Some("cancelled") {
                let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("");
                got_cancelled = reason == "client_request";
                break;
            }
        }
        assert!(got_cancelled, "should receive cancelled(client_request)");
    }
}
