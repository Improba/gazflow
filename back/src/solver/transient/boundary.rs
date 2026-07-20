/// Condition amont : pression fixe [bar].
#[derive(Debug, Clone, Copy)]
pub struct SourceBoundary {
    pub pressure_bar: f64,
}

/// Condition aval : débit normal imposé [Nm³/s] (négatif = prélèvement),
/// ou pression Dirichlet si `pressure_bar` est renseigné (jonctions / arbres PDE).
#[derive(Debug, Clone, Copy)]
pub struct SinkBoundary {
    pub flow_m3s: f64,
    /// Si `Some`, Dirichlet aval (prioritaire sur le débit).
    pub pressure_bar: Option<f64>,
}

impl SourceBoundary {
    pub fn fixed_pressure(pressure_bar: f64) -> Self {
        Self { pressure_bar }
    }
}

impl SinkBoundary {
    pub fn fixed_flow(flow_m3s: f64) -> Self {
        Self {
            flow_m3s,
            pressure_bar: None,
        }
    }

    pub fn fixed_pressure(pressure_bar: f64) -> Self {
        Self {
            flow_m3s: 0.0,
            pressure_bar: Some(pressure_bar),
        }
    }

    pub fn is_dirichlet(&self) -> bool {
        self.pressure_bar.is_some()
    }
}
