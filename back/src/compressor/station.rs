#[derive(Debug, Clone, PartialEq)]
pub struct TurboMeasurement {
    pub speed_rpm: f64,
    pub head_kj_per_kg: f64,
    pub flow_m3_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompressorConfiguration {
    pub nr_of_serial_stages: usize,
}

#[derive(Debug, Clone, Default)]
pub struct StationModel {
    /// Carte opératoire (prioritaire pour l'interpolation).
    pub characteristic_measurements: Vec<TurboMeasurement>,
    /// Surgeline (repli + contrôle faisabilité futur).
    pub surgeline_measurements: Vec<TurboMeasurement>,
    pub speed_min_rpm: Option<f64>,
    pub speed_max_rpm: Option<f64>,
    pub configurations: Vec<CompressorConfiguration>,
}

impl StationModel {
    pub fn push_configuration(&mut self, nr_of_serial_stages: usize) {
        self.configurations.push(CompressorConfiguration {
            nr_of_serial_stages: nr_of_serial_stages.max(1),
        });
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
    fn test_station_model_max_serial_stages_defaults_to_one() {
        let mut station = StationModel::default();
        assert_eq!(station.max_serial_stages(), 1);

        station.push_configuration(3);
        station.push_configuration(2);
        assert_eq!(station.max_serial_stages(), 3);
    }
}
