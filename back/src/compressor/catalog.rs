use std::collections::HashMap;

use super::station::StationModel;

#[derive(Debug, Clone, Default)]
pub struct CompressorCatalog {
    pub stations: HashMap<String, StationModel>,
}

impl CompressorCatalog {
    pub fn station_mut(&mut self, station_id: &str) -> &mut StationModel {
        self.stations.entry(station_id.to_string()).or_default()
    }

    pub fn station(&self, station_id: &str) -> Option<&StationModel> {
        self.stations.get(station_id)
    }
}

#[cfg(test)]
mod tests {
    use super::CompressorCatalog;

    #[test]
    fn test_station_mut_creates_station() {
        let mut catalog = CompressorCatalog::default();
        catalog.station_mut("CS-1").push_configuration(2);

        let station = catalog.station("CS-1").expect("station to exist");
        assert_eq!(station.max_serial_stages(), 2);
    }
}
