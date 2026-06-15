/// Condition amont : pression fixe [bar].
#[derive(Debug, Clone, Copy)]
pub struct SourceBoundary {
    pub pressure_bar: f64,
}

/// Condition aval : débit normal imposé [Nm³/s] (négatif = prélèvement).
#[derive(Debug, Clone, Copy)]
pub struct SinkBoundary {
    pub flow_m3s: f64,
}

impl SourceBoundary {
    pub fn fixed_pressure(pressure_bar: f64) -> Self {
        Self { pressure_bar }
    }
}

impl SinkBoundary {
    pub fn fixed_flow(flow_m3s: f64) -> Self {
        Self { flow_m3s }
    }
}
