//! Modèles d'équation d'état (EOS) pour mélanges gaz.

pub mod pr78;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EosModel {
    #[default]
    PapayKay,
    /// Peng-Robinson 1978 (recommandé H₂ > 20 %).
    Pr78,
}

impl EosModel {
    pub fn auto_for_composition(h2_fraction: f64) -> Self {
        if h2_fraction > 0.20 + 1e-9 {
            Self::Pr78
        } else {
            Self::PapayKay
        }
    }
}
