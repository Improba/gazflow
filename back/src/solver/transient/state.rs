use super::mesh::PipeMesh;

/// État transitoire 1D d'une conduite : pressions aux centres de cellule, débits aux interfaces.
#[derive(Debug, Clone)]
pub struct TransientPipeState {
    /// Pression [bar] par cellule (longueur `n_cells`).
    pub pressures: Vec<f64>,
    /// Débit normal [Nm³/s] aux interfaces (longueur `n_cells + 1`, indices 0 = amont).
    pub flows: Vec<f64>,
}

impl TransientPipeState {
    pub fn uniform_pressure(mesh: &PipeMesh, pressure_bar: f64, flow_m3s: f64) -> Self {
        Self {
            pressures: vec![pressure_bar; mesh.n_cells],
            flows: vec![flow_m3s; mesh.n_cells + 1],
        }
    }

    pub fn from_endpoint_pressures(
        mesh: &PipeMesh,
        pressure_from_bar: f64,
        pressure_to_bar: f64,
        flow_m3s: f64,
    ) -> Self {
        let n = mesh.n_cells;
        let mut pressures = Vec::with_capacity(n);
        if n == 1 {
            pressures.push(0.5 * (pressure_from_bar + pressure_to_bar));
        } else {
            for i in 0..n {
                // Centres de cellule : le bord amont (P_source) n'est pas une cellule.
                let frac = (i as f64 + 0.5) / n as f64;
                pressures.push(pressure_from_bar + frac * (pressure_to_bar - pressure_from_bar));
            }
        }
        Self {
            pressures,
            flows: vec![flow_m3s; n + 1],
        }
    }

    pub fn linepack_kg(
        &self,
        mesh: &PipeMesh,
        composition: &crate::solver::gas_properties::GasComposition,
        temperature_k: f64,
    ) -> f64 {
        self.pressures
            .iter()
            .map(|&p| {
                let rho = composition.density_kg_per_m3(p, temperature_k);
                rho * mesh.area_m2 * mesh.dx
            })
            .sum()
    }
}
