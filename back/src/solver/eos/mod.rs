//! Modèles d'équation d'état (EOS) pour mélanges gaz.

pub mod gerg;
pub mod pr78;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EosModel {
    #[default]
    PapayKay,
    /// Peng-Robinson 1978 (recommandé H₂ > 20 %).
    Pr78,
    /// GERG-2008 (précision élevée, H₂ élevé / calage).
    Gerg2008,
}

impl EosModel {
    /// Sélection auto : GERG si H₂ > 50 %, PR-78 si H₂ > 20 %, sinon Papay+Kay.
    pub fn auto_for_composition(h2_fraction: f64) -> Self {
        if h2_fraction > 0.50 + 1e-9 {
            Self::Gerg2008
        } else if h2_fraction > 0.20 + 1e-9 {
            Self::Pr78
        } else {
            Self::PapayKay
        }
    }
}
