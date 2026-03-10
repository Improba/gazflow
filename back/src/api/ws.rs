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

const CANCEL_NONE: u8 = 0;
const CANCEL_CLIENT_REQUEST: u8 = 1;
const CANCEL_TIMEOUT: u8 = 2;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    StartSimulation {
        run_id: Option<String>,
        demands: Option<HashMap<String, f64>>,
        options: Option<StartOptions>,
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
    timeout_ms: u64,
    #[serde(default)]
    initial_pressures: Option<HashMap<String, f64>>,
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
                            Ok(ClientMessage::StartSimulation { run_id, demands, options }) => {
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
                                let demands = demands.unwrap_or_else(|| (*state.default_demands).clone());
                                let options = options.unwrap_or_default();
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
                                    run_solver_stream(
                                        state_for_solver,
                                        demands,
                                        options,
                                        run_id,
                                        run_cancel,
                                        run_cancel_reason,
                                        tx_for_solver,
                                    );
                                });
                            }
                            Ok(ClientMessage::CancelSimulation { run_id }) => {
                                if let (Some(active_id), Some(flag), Some(reason)) =
                                    (&active_run_id, &cancel_flag, &cancel_reason)
                                {
                                    if run_id
                                        .as_deref()
                                        .map(|rid| rid == active_id)
                                        .unwrap_or(true)
                                    {
                                        reason.store(CANCEL_CLIENT_REQUEST, Ordering::Relaxed);
                                        flag.store(true, Ordering::Relaxed);
                                    }
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

fn run_solver_stream(
    state: super::SharedState,
    demands: HashMap<String, f64>,
    options: StartOptions,
    run_id: String,
    cancel_flag: Arc<AtomicBool>,
    cancel_reason: Arc<AtomicU8>,
    tx: mpsc::Sender<ServerMessage>,
) {
    let started = Instant::now();
    let timeout = Duration::from_millis(options.timeout_ms);
    let mut seq = 1_u64;

    let result = state.rayon_pool.install(|| {
        solver::solve_steady_state_with_progress(
            &state.network,
            &demands,
            options.initial_pressures.as_ref(),
            options.max_iter,
            options.tolerance,
            options.snapshot_every,
            |progress: SolverProgress| {
                if options.timeout_ms == 0
                    || (options.timeout_ms > 0 && started.elapsed() >= timeout)
                {
                    cancel_reason.store(CANCEL_TIMEOUT, Ordering::Relaxed);
                    cancel_flag.store(true, Ordering::Relaxed);
                }
                if cancel_flag.load(Ordering::Relaxed) {
                    return SolverControl::Cancel;
                }

                if progress.iter == 1 || progress.iter % options.iteration_every.max(1) == 0 {
                    seq += 1;
                    let _ = tx.blocking_send(ServerMessage::Iteration {
                        run_id: run_id.clone(),
                        seq,
                        iter: progress.iter,
                        residual: progress.residual,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
                }

                if let (Some(pressures), Some(flows)) = (progress.pressures, progress.flows) {
                    seq += 1;
                    let snapshot = ServerMessage::Snapshot {
                        run_id: run_id.clone(),
                        seq,
                        iter: progress.iter,
                        pressures,
                        flows,
                    };
                    let _ = tx.try_send(snapshot);
                }

                SolverControl::Continue
            },
        )
    });

    match result {
        Ok(final_result) => {
            super::export::store_export_record(
                &state,
                super::export::new_export_record(
                    run_id.clone(),
                    demands.clone(),
                    final_result.clone(),
                    started.elapsed().as_millis() as u64,
                ),
            );
            seq += 1;
            let _ = tx.blocking_send(ServerMessage::Converged {
                run_id,
                seq,
                result: final_result,
                total_ms: started.elapsed().as_millis() as u64,
            });
        }
        Err(err) => {
            seq += 1;
            if cancel_flag.load(Ordering::Relaxed) {
                let reason = match cancel_reason.load(Ordering::Relaxed) {
                    CANCEL_CLIENT_REQUEST => "client_request",
                    CANCEL_TIMEOUT => "timeout",
                    _ => "cancelled",
                };
                let _ = tx.blocking_send(ServerMessage::Cancelled {
                    run_id,
                    seq,
                    reason: reason.to_string(),
                });
            } else if err.to_string().contains("did not converge") {
                let _ = tx.blocking_send(ServerMessage::Cancelled {
                    run_id,
                    seq,
                    reason: "diverged".to_string(),
                });
            } else {
                let _ = tx.blocking_send(ServerMessage::Error {
                    run_id,
                    seq,
                    message: err.to_string(),
                    fatal: true,
                });
            }
        }
    }
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

    use crate::graph::{ConnectionKind, GasNetwork, Node, Pipe};

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
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
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
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
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
            id: "p1".into(),
            from: "source".into(),
            to: "sink".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 500.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
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
            "options": {"max_iter": 10_000, "tolerance": 1e-12, "iteration_every": 1000, "snapshot_every": 1, "timeout_ms": 0}
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
}
