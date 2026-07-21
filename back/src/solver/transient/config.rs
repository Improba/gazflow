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
    /// Si vrai, réduit `dt` selon C/G et un hint CFL (précision ; schéma déjà implicite).
    #[serde(default)]
    pub adaptive_dt: bool,
    /// Relaxation Picard nodale (cycles). `None` → 0,35 par défaut.
    #[serde(default)]
    pub picard_relax: Option<f64>,
}

impl Default for TransientConfig {
    fn default() -> Self {
        Self {
            duration_s: 3600.0,
            dt_s: 300.0,
            gas_composition: GasComposition::default(),
            n_cells_per_pipe: None,
            adaptive_dt: false,
            picard_relax: None,
        }
    }
}
