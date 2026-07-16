//! API compresseur : mode carte session + points de fonctionnement.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::solver::{self, CompressorMapMode, CompressorOperatingPoint};

use super::{
    ApiError, ApiResult, LastSimulationSnapshot, SharedState, active_network,
    sync_compressor_map_mode_for_solve,
};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct MapModeResponse {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetMapModeRequest {
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub(super) struct OperatingPointsResponse {
    pub points: Vec<CompressorOperatingPoint>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

fn effective_map_mode(state: &SharedState) -> CompressorMapMode {
    state
        .compressor_map_mode_override
        .read()
        .expect("compressor map mode lock should not be poisoned")
        .unwrap_or_else(solver::compressor_map_mode_from_env)
}

fn map_mode_response(mode: CompressorMapMode) -> MapModeResponse {
    MapModeResponse {
        mode: mode.api_label().to_string(),
    }
}

pub(super) async fn get_map_mode(State(state): State<SharedState>) -> ApiResult<MapModeResponse> {
    Ok(Json(map_mode_response(effective_map_mode(&state))))
}

pub(super) async fn put_map_mode(
    State(state): State<SharedState>,
    Json(payload): Json<SetMapModeRequest>,
) -> ApiResult<MapModeResponse> {
    let Some(mode) = CompressorMapMode::parse_api(&payload.mode) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "mode invalide : attendu legacy, measurement ou biquadratic (reçu {:?})",
                payload.mode
            ),
        ));
    };
    {
        let mut guard = state
            .compressor_map_mode_override
            .write()
            .expect("compressor map mode lock should not be poisoned");
        *guard = Some(mode);
    }
    sync_compressor_map_mode_for_solve(&state);
    Ok(Json(map_mode_response(mode)))
}

pub(super) async fn get_operating_points(
    State(state): State<SharedState>,
) -> ApiResult<OperatingPointsResponse> {
    let snapshot = state
        .last_simulation
        .read()
        .expect("last simulation lock should not be poisoned")
        .clone();
    let Some(LastSimulationSnapshot { demands, result }) = snapshot else {
        return Ok(Json(OperatingPointsResponse { points: Vec::new() }));
    };
    let network = active_network(&state);
    let demand_scale = result.demand_scale_achieved.unwrap_or(1.0);
    let points = solver::compressor_operating_points(&network, &result, &demands, demand_scale);
    Ok(Json(OperatingPointsResponse { points }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::graph::{ConnectionKind, EquipmentSpec, GasNetwork, Node, Pipe};
    use crate::solver::{CompressorMapMode, SolverResult, compressor_operating_points};

    use super::CompressorMapMode as MapMode;

    fn network_with_compressor() -> GasNetwork {
        let mut net = GasNetwork::new();
        net.add_node(Node {
            id: "in".into(),
            x: 0.0,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: Some(50.0),
            flow_min_m3s: None,
            flow_max_m3s: None,
        });
        net.add_node(Node {
            id: "out".into(),
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
            id: "CS1".into(),
            from: "in".into(),
            to: "out".into(),
            kind: ConnectionKind::CompressorStation,
            is_open: true,
            length_km: 0.1,
            diameter_mm: 800.0,
            roughness_mm: 0.05,
            compressor_ratio_max: Some(1.2),
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        });
        net
    }

    #[test]
    fn map_mode_parse_rejects_unknown() {
        assert!(MapMode::parse_api("turbo").is_none());
        assert_eq!(
            MapMode::parse_api("measurement"),
            Some(CompressorMapMode::Measurement)
        );
    }

    #[test]
    fn operating_points_empty_without_compressor_stations() {
        let net = GasNetwork::new();
        let result = SolverResult::default();
        let points = compressor_operating_points(&net, &result, &HashMap::new(), 1.0);
        assert!(points.is_empty());
    }

    #[test]
    fn operating_points_lists_active_compressor() {
        let net = network_with_compressor();
        let mut result = SolverResult::from_core(
            HashMap::from([("in".to_string(), 48.0), ("out".to_string(), 55.0)]),
            HashMap::from([("CS1".to_string(), 12.0)]),
            3,
            1e-7,
        );
        result.demand_scale_achieved = Some(1.0);
        let demands = HashMap::from([("out".to_string(), -12.0)]);
        let points = compressor_operating_points(&net, &result, &demands, 1.0);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].station_id, "CS1");
        assert!(points[0].q_m3s > 0.0);
        assert!(points[0].ratio >= 1.0);
        assert!(points[0].p_in_bar > 1.0);
        assert!(points[0].p_out_bar > points[0].p_in_bar);
    }
}
