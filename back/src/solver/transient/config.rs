use serde::{Deserialize, Serialize};

use crate::solver::gas_properties::GasComposition;

/// Configuration du simulateur transitoire MVP (quasi-stationnaire ou PDE).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TransientConfig {
    pub duration_s: f64,
    pub dt_s: f64,
    pub gas_composition: GasComposition,
    /// Nombre de cellules par conduite en mode PDE (défaut : adaptatif selon longueur).
    #[serde(default)]
    pub n_cells_per_pipe: Option<usize>,
}

impl Default for TransientConfig {
    fn default() -> Self {
        Self {
            duration_s: 3600.0,
            dt_s: 300.0,
            gas_composition: GasComposition::default(),
            n_cells_per_pipe: None,
        }
    }
}
