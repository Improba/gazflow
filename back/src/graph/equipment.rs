//! Paramètres des organes de régulation (P8).

use serde::{Deserialize, Serialize};

/// Paramètres optionnels d'un arc réseau (détendeur, vanne Cv, poste livraison).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EquipmentSpec {
    /// Consigne pression aval [bar] (détendeur / poste livraison).
    pub regulator_setpoint_bar: Option<f64>,
    /// Marge minimale amont − consigne pour régulation active [bar].
    pub regulator_delta_p_min_bar: Option<f64>,
    /// Coefficient Cv ISA (vanne de régulation).
    pub control_valve_cv: Option<f64>,
    /// Ouverture relative [%] (0 = fermée, 100 = pleine ouverture).
    pub control_valve_opening_pct: Option<f64>,
    /// Pression contractuelle minimale aval [bar] (poste livraison).
    pub delivery_min_pressure_bar: Option<f64>,
}

impl EquipmentSpec {
    pub fn pressure_regulator(setpoint_bar: f64, delta_p_min_bar: f64) -> Self {
        Self {
            regulator_setpoint_bar: Some(setpoint_bar),
            regulator_delta_p_min_bar: Some(delta_p_min_bar),
            ..Self::default()
        }
    }

    pub fn delivery_station(
        setpoint_bar: f64,
        min_pressure_bar: f64,
        delta_p_min_bar: f64,
    ) -> Self {
        Self {
            regulator_setpoint_bar: Some(setpoint_bar),
            regulator_delta_p_min_bar: Some(delta_p_min_bar),
            delivery_min_pressure_bar: Some(min_pressure_bar),
            ..Self::default()
        }
    }

    pub fn control_valve(cv: f64, opening_pct: f64) -> Self {
        Self {
            control_valve_cv: Some(cv),
            control_valve_opening_pct: Some(opening_pct),
            ..Self::default()
        }
    }

    pub fn effective_setpoint_bar(&self) -> Option<f64> {
        // La consigne de régulation est distincte du minimum contractuel (poste livraison).
        self.regulator_setpoint_bar
    }

    pub fn is_empty(&self) -> bool {
        self.regulator_setpoint_bar.is_none()
            && self.regulator_delta_p_min_bar.is_none()
            && self.control_valve_cv.is_none()
            && self.control_valve_opening_pct.is_none()
            && self.delivery_min_pressure_bar.is_none()
    }

    /// Fusionne les champs définis de `patch` (simulation / édition UI).
    pub fn merge_from(&mut self, patch: &EquipmentSpec) {
        if let Some(v) = patch.regulator_setpoint_bar {
            self.regulator_setpoint_bar = Some(v);
        }
        if let Some(v) = patch.regulator_delta_p_min_bar {
            self.regulator_delta_p_min_bar = Some(v);
        }
        if let Some(v) = patch.control_valve_cv {
            self.control_valve_cv = Some(v);
        }
        if let Some(v) = patch.control_valve_opening_pct {
            self.control_valve_opening_pct = Some(v);
        }
        if let Some(v) = patch.delivery_min_pressure_bar {
            self.delivery_min_pressure_bar = Some(v);
        }
    }
}
