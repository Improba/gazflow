use serde::{Deserialize, Serialize};

/// Événements métier appliqués au fil du transitoire MVP.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransientEvent {
    ValveClose {
        time_s: f64,
        pipe_id: String,
    },
    DemandChange {
        time_s: f64,
        node_id: String,
        demand_m3s: f64,
    },
    RegulatorSetpoint {
        time_s: f64,
        pipe_id: String,
        setpoint_bar: f64,
    },
}

impl TransientEvent {
    pub fn time_s(&self) -> f64 {
        match self {
            Self::ValveClose { time_s, .. }
            | Self::DemandChange { time_s, .. }
            | Self::RegulatorSetpoint { time_s, .. } => *time_s,
        }
    }
}
