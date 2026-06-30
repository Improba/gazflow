use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct TurboMeasurement {
    pub speed_rpm: f64,
    pub head_kj_per_kg: f64,
    pub flow_m3_s: f64,
}

pub type BiquadraticCoeffs = [f64; 9];
pub type QuadraticCurve = [f64; 3];

#[derive(Debug, Clone, PartialEq)]
pub struct TurboCompressorModel {
    pub id: String,
    pub speed_min_rpm: Option<f64>,
    pub speed_max_rpm: Option<f64>,
    pub biquadratic_head_coeffs: Option<BiquadraticCoeffs>,
    pub surgeline_curve: Option<QuadraticCurve>,
    pub chokeline_curve: Option<QuadraticCurve>,
    pub characteristic_measurements: Vec<TurboMeasurement>,
    pub surgeline_measurements: Vec<TurboMeasurement>,
}

impl TurboCompressorModel {
    pub fn new(id: String) -> Self {
        Self {
            id,
            speed_min_rpm: None,
            speed_max_rpm: None,
            biquadratic_head_coeffs: None,
            surgeline_curve: None,
            chokeline_curve: None,
            characteristic_measurements: Vec::new(),
            surgeline_measurements: Vec::new(),
        }
    }

    pub fn push_characteristic_measurement(&mut self, measurement: TurboMeasurement) {
        self.characteristic_measurements.push(measurement);
    }

    pub fn push_surgeline_measurement(&mut self, measurement: TurboMeasurement) {
        self.surgeline_measurements.push(measurement);
    }

    pub fn map_measurements(&self) -> &[TurboMeasurement] {
        if !self.characteristic_measurements.is_empty() {
            &self.characteristic_measurements
        } else {
            &self.surgeline_measurements
        }
    }

    pub fn update_speed_min(&mut self, speed_rpm: f64) {
        if !speed_rpm.is_finite() {
            return;
        }
        self.speed_min_rpm = match self.speed_min_rpm {
            Some(current) => Some(current.min(speed_rpm)),
            None => Some(speed_rpm),
        };
    }

    pub fn update_speed_max(&mut self, speed_rpm: f64) {
        if !speed_rpm.is_finite() {
            return;
        }
        self.speed_max_rpm = match self.speed_max_rpm {
            Some(current) => Some(current.max(speed_rpm)),
            None => Some(speed_rpm),
        };
    }

    pub fn set_biquadratic_coeff(&mut self, coeff_idx: usize, value: f64) {
        if coeff_idx >= 9 || !value.is_finite() {
            return;
        }
        let coeffs = self.biquadratic_head_coeffs.get_or_insert([0.0; 9]);
        coeffs[coeff_idx] = value;
    }

    pub fn set_surgeline_coeff(&mut self, coeff_idx: usize, value: f64) {
        if coeff_idx >= 3 || !value.is_finite() {
            return;
        }
        let coeffs = self.surgeline_curve.get_or_insert([0.0; 3]);
        coeffs[coeff_idx] = value;
    }

    pub fn set_chokeline_coeff(&mut self, coeff_idx: usize, value: f64) {
        if coeff_idx >= 3 || !value.is_finite() {
            return;
        }
        let coeffs = self.chokeline_curve.get_or_insert([0.0; 3]);
        coeffs[coeff_idx] = value;
    }

    pub fn speed_bounds(&self) -> Option<(f64, f64)> {
        let from_measurements = || {
            self.map_measurements()
                .iter()
                .chain(self.surgeline_measurements.iter())
                .fold(None, |acc: Option<(f64, f64)>, m| match acc {
                    Some((min, max)) => Some((min.min(m.speed_rpm), max.max(m.speed_rpm))),
                    None => Some((m.speed_rpm, m.speed_rpm)),
                })
        };

        match (self.speed_min_rpm, self.speed_max_rpm) {
            (Some(min), Some(max)) => Some((min, max)),
            (Some(min), None) => {
                let max = from_measurements().map(|(_, max)| max).unwrap_or(min);
                Some((min, max))
            }
            (None, Some(max)) => {
                let min = from_measurements().map(|(min, _)| min).unwrap_or(max);
                Some((min, max))
            }
            (None, None) => from_measurements(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompressorConfiguration {
    pub conf_id: Option<String>,
    pub nr_of_serial_stages: usize,
    /// Turbos référencés par les étages (premier compresseur de chaque étage série).
    pub turbo_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StationModel {
    /// Turbos indexés par id (nouveau modèle détaillé).
    pub turbos: HashMap<String, TurboCompressorModel>,
    /// Carte opératoire (prioritaire pour l'interpolation).
    pub characteristic_measurements: Vec<TurboMeasurement>,
    /// Surgeline (repli + contrôle faisabilité futur).
    pub surgeline_measurements: Vec<TurboMeasurement>,
    pub speed_min_rpm: Option<f64>,
    pub speed_max_rpm: Option<f64>,
    pub configurations: Vec<CompressorConfiguration>,
}

impl StationModel {
    pub fn turbo_mut(&mut self, turbo_id: &str) -> &mut TurboCompressorModel {
        self.turbos
            .entry(turbo_id.to_string())
            .or_insert_with(|| TurboCompressorModel::new(turbo_id.to_string()))
    }

    pub fn turbo(&self, turbo_id: &str) -> Option<&TurboCompressorModel> {
        self.turbos.get(turbo_id)
    }

    pub fn preferred_turbo(&self) -> Option<&TurboCompressorModel> {
        if self.turbos.is_empty() {
            return None;
        }

        if let Some(turbo_id) = self.default_conf_turbo_id() {
            if let Some(turbo) = self.turbos.get(turbo_id) {
                return Some(turbo);
            }
        }

        if let Some(default_conf_id) = self.default_conf_id() {
            if let Some(turbo) = self.turbos.get(default_conf_id) {
                return Some(turbo);
            }
        }

        if self.turbos.len() == 1 {
            return self.turbos.values().next();
        }

        self.turbos.values().min_by(|a, b| a.id.cmp(&b.id))
    }

    /// Turbo actif pour la configuration par défaut (config_2 si présente).
    pub fn default_conf_turbo_id(&self) -> Option<&str> {
        let conf_id = self.default_conf_id()?;
        self.turbo_id_for_conf(conf_id)
    }

    pub fn turbo_id_for_conf(&self, conf_id: &str) -> Option<&str> {
        self.configurations
            .iter()
            .find(|cfg| cfg.conf_id.as_deref() == Some(conf_id))
            .and_then(|cfg| cfg.turbo_ids.first())
            .map(String::as_str)
    }

    pub fn push_configuration(&mut self, conf_id: Option<String>, nr_of_serial_stages: usize) {
        self.configurations.push(CompressorConfiguration {
            conf_id,
            nr_of_serial_stages: nr_of_serial_stages.max(1),
            turbo_ids: Vec::new(),
        });
    }

    pub fn link_turbo_to_last_configuration(&mut self, turbo_id: &str) {
        if let Some(cfg) = self.configurations.last_mut() {
            if !cfg.turbo_ids.iter().any(|id| id == turbo_id) {
                cfg.turbo_ids.push(turbo_id.to_string());
            }
        }
    }

    pub fn push_characteristic_measurement(&mut self, measurement: TurboMeasurement) {
        self.characteristic_measurements.push(measurement);
    }

    pub fn push_surgeline_measurement(&mut self, measurement: TurboMeasurement) {
        self.surgeline_measurements.push(measurement);
    }

    pub fn map_measurements(&self) -> &[TurboMeasurement] {
        if !self.characteristic_measurements.is_empty() {
            &self.characteristic_measurements
        } else {
            &self.surgeline_measurements
        }
    }

    pub fn update_speed_min(&mut self, speed_rpm: f64) {
        if !speed_rpm.is_finite() {
            return;
        }
        self.speed_min_rpm = match self.speed_min_rpm {
            Some(current) => Some(current.min(speed_rpm)),
            None => Some(speed_rpm),
        };
    }

    pub fn update_speed_max(&mut self, speed_rpm: f64) {
        if !speed_rpm.is_finite() {
            return;
        }
        self.speed_max_rpm = match self.speed_max_rpm {
            Some(current) => Some(current.max(speed_rpm)),
            None => Some(speed_rpm),
        };
    }

    pub fn max_serial_stages(&self) -> usize {
        self.configurations
            .iter()
            .map(|cfg| cfg.nr_of_serial_stages.max(1))
            .max()
            .unwrap_or(1)
    }

    pub fn default_conf_id(&self) -> Option<&str> {
        self.configurations
            .iter()
            .find(|cfg| cfg.conf_id.as_deref() == Some("config_2"))
            .or_else(|| {
                self.configurations
                    .iter()
                    .max_by_key(|cfg| cfg.nr_of_serial_stages)
            })
            .and_then(|cfg| cfg.conf_id.as_deref())
    }

    pub fn serial_stages_for_conf(&self, conf_id: Option<&str>) -> usize {
        if let Some(id) = conf_id {
            if let Some(cfg) = self
                .configurations
                .iter()
                .find(|cfg| cfg.conf_id.as_deref() == Some(id))
            {
                return cfg.nr_of_serial_stages.max(1);
            }
        }
        self.max_serial_stages()
    }

    pub fn speed_bounds(&self) -> Option<(f64, f64)> {
        let from_measurements = || {
            self.map_measurements()
                .iter()
                .chain(self.surgeline_measurements.iter())
                .fold(None, |acc: Option<(f64, f64)>, m| match acc {
                    Some((min, max)) => Some((min.min(m.speed_rpm), max.max(m.speed_rpm))),
                    None => Some((m.speed_rpm, m.speed_rpm)),
                })
        };

        match (self.speed_min_rpm, self.speed_max_rpm) {
            (Some(min), Some(max)) => Some((min, max)),
            (Some(min), None) => {
                let max = from_measurements().map(|(_, max)| max).unwrap_or(min);
                Some((min, max))
            }
            (None, Some(max)) => {
                let min = from_measurements().map(|(min, _)| min).unwrap_or(max);
                Some((min, max))
            }
            (None, None) => from_measurements(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StationModel, TurboMeasurement};

    #[test]
    fn test_station_model_tracks_speed_bounds() {
        let mut station = StationModel::default();
        station.update_speed_min(4_700.0);
        station.update_speed_min(4_800.0);
        station.update_speed_max(6_200.0);
        station.update_speed_max(6_000.0);

        assert_eq!(station.speed_bounds(), Some((4_700.0, 6_200.0)));
    }

    #[test]
    fn test_station_model_prefers_characteristic_for_map() {
        let mut station = StationModel::default();
        station.push_surgeline_measurement(TurboMeasurement {
            speed_rpm: 4_700.0,
            head_kj_per_kg: 60.0,
            flow_m3_s: 0.20,
        });
        station.push_characteristic_measurement(TurboMeasurement {
            speed_rpm: 5_600.0,
            head_kj_per_kg: 75.0,
            flow_m3_s: 0.30,
        });

        assert_eq!(station.map_measurements().len(), 1);
        assert_eq!(station.map_measurements()[0].speed_rpm, 5_600.0);
    }

    #[test]
    fn test_default_conf_id_prefers_config_2() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_1".into()), 1);
        station.push_configuration(Some("config_2".into()), 1);
        assert_eq!(station.default_conf_id(), Some("config_2"));
    }

    #[test]
    fn test_station_model_max_serial_stages_defaults_to_one() {
        let mut station = StationModel::default();
        assert_eq!(station.max_serial_stages(), 1);

        station.push_configuration(None, 3);
        station.push_configuration(None, 2);
        assert_eq!(station.max_serial_stages(), 3);
    }

    #[test]
    fn test_preferred_turbo_uses_default_conf_id() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_1".into()), 1);
        station.link_turbo_to_last_configuration("turbo_a");
        station.push_configuration(Some("config_2".into()), 2);
        station.link_turbo_to_last_configuration("turbo_b");

        station.turbo_mut("turbo_a");
        station.turbo_mut("turbo_b");

        let preferred = station.preferred_turbo().expect("preferred turbo");
        assert_eq!(preferred.id, "turbo_b");
        assert_eq!(station.default_conf_turbo_id(), Some("turbo_b"));
    }

    #[test]
    fn test_preferred_turbo_legacy_conf_key_fallback() {
        let mut station = StationModel::default();
        station.push_configuration(Some("config_2".into()), 1);
        station.turbo_mut("config_2");
        station.turbo_mut("compressor_other");

        let preferred = station.preferred_turbo().expect("legacy key turbo");
        assert_eq!(preferred.id, "config_2");
    }
}
