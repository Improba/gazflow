use std::collections::HashMap;
use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use gazflow_back::api;
use gazflow_back::graph::{ConnectionKind, GasNetwork, Node, Pipe};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

fn build_test_network(with_isolated: bool) -> GasNetwork {
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
    if with_isolated {
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
    }
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
    });
    net
}

async fn spawn_server(with_isolated: bool) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut default_demands = HashMap::new();
    default_demands.insert("sink".to_string(), -5.0);
    let app = api::create_router_with_limits(build_test_network(with_isolated), default_demands, 4);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

#[tokio::test]
async fn test_api_network_count() {
    let (addr, _server) = spawn_server(false).await;
    let url = format!("http://{addr}/api/network");
    let json: Value = reqwest::get(url)
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(json.get("node_count").and_then(Value::as_u64), Some(2));
    assert_eq!(json.get("edge_count").and_then(Value::as_u64), Some(1));
}

#[tokio::test]
async fn test_ws_start_simulation() {
    let (addr, _server) = spawn_server(false).await;
    let url = format!("ws://{addr}/api/ws/sim");
    let (mut ws, _) = connect_async(url).await.expect("connect ws");

    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "start_simulation",
            "run_id": "it-ws-start",
            "options": {"max_iter": 200, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
        })
        .to_string(),
    ))
    .await
    .expect("send start");

    let mut got_iteration = false;
    let mut got_converged = false;
    for _ in 0..200 {
        let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
            continue;
        };
        let msg: Value = serde_json::from_str(&txt).expect("json");
        match msg.get("type").and_then(Value::as_str) {
            Some("iteration") => got_iteration = true,
            Some("converged") => {
                got_converged = true;
                break;
            }
            _ => {}
        }
    }
    assert!(got_iteration, "expected at least one iteration");
    assert!(got_converged, "expected converged message");
}

#[tokio::test]
async fn test_ws_cancel_simulation() {
    let (addr, _server) = spawn_server(true).await;
    let url = format!("ws://{addr}/api/ws/sim");
    let (mut ws, _) = connect_async(url).await.expect("connect ws");

    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "start_simulation",
            "run_id": "it-ws-cancel",
            "demands": {"sink": -5.0, "isolated": -1.0},
            "options": {"max_iter": 1_000_000, "tolerance": 1e-12, "iteration_every": 1000, "snapshot_every": 1000, "timeout_ms": 60_000}
        })
        .to_string(),
    ))
    .await
    .expect("send start");

    let mut got_cancelled = false;
    for _ in 0..200 {
        let Some(Ok(WsMessage::Text(txt))) = ws.next().await else {
            continue;
        };
        let msg: Value = serde_json::from_str(&txt).expect("json");
        match msg.get("type").and_then(Value::as_str) {
            Some("started") => {
                ws.send(WsMessage::Text(
                    serde_json::json!({
                        "type": "cancel_simulation",
                        "run_id": "it-ws-cancel"
                    })
                    .to_string(),
                ))
                .await
                .expect("send cancel");
            }
            Some("cancelled") => {
                got_cancelled = msg.get("reason").and_then(Value::as_str) == Some("client_request");
                if got_cancelled {
                    break;
                }
            }
            _ => {}
        }
    }
    assert!(got_cancelled, "expected cancelled(client_request)");
}

#[tokio::test]
async fn test_concurrent_simulations() {
    let (addr, _server) = spawn_server(false).await;
    let url = format!("ws://{addr}/api/ws/sim");
    let (mut ws1, _) = connect_async(url.clone()).await.expect("connect ws1");
    let (mut ws2, _) = connect_async(url).await.expect("connect ws2");

    ws1.send(WsMessage::Text(
        serde_json::json!({
            "type": "start_simulation",
            "run_id": "it-concurrent-1",
            "options": {"max_iter": 400, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
        })
        .to_string(),
    ))
    .await
    .expect("send start ws1");

    ws2.send(WsMessage::Text(
        serde_json::json!({
            "type": "start_simulation",
            "run_id": "it-concurrent-2",
            "options": {"max_iter": 400, "tolerance": 1e-4, "snapshot_every": 10, "timeout_ms": 10_000}
        })
        .to_string(),
    ))
    .await
    .expect("send start ws2");

    let mut got_converged_1 = false;
    for _ in 0..200 {
        let Some(Ok(WsMessage::Text(txt))) = ws1.next().await else {
            continue;
        };
        let msg: Value = serde_json::from_str(&txt).expect("json ws1");
        if msg.get("type").and_then(Value::as_str) == Some("converged") {
            got_converged_1 = true;
            break;
        }
    }

    let mut got_converged_2 = false;
    for _ in 0..200 {
        let Some(Ok(WsMessage::Text(txt))) = ws2.next().await else {
            continue;
        };
        let msg: Value = serde_json::from_str(&txt).expect("json ws2");
        if msg.get("type").and_then(Value::as_str) == Some("converged") {
            got_converged_2 = true;
            break;
        }
    }

    assert!(
        got_converged_1,
        "first websocket simulation should converge"
    );
    assert!(
        got_converged_2,
        "second websocket simulation should converge"
    );
}
