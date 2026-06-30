use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

mod catalog;
mod flow_topology;
mod map;
mod parse;
mod station;

pub use catalog::CompressorCatalog;
pub use map::{
    CompressorOperatingContext, OperatingPoint, effective_ratio_from_flow,
    effective_ratio_from_operating_point, effective_ratio_with_nominal,
    effective_ratio_with_nominal_for_mode, eval_biquadratic_head, eval_quadratic,
    find_operating_point, find_operating_point_for_mode, had_to_pressure_ratio, interpolate_head,
};
pub use flow_topology::estimated_map_flow_m3s;
pub use parse::load_compressor_catalog;
pub use station::{
    BiquadraticCoeffs, CompressorConfiguration, QuadraticCurve, StationModel, TurboCompressorModel,
    TurboMeasurement,
};

pub fn load_compressor_ratios<P: AsRef<Path>>(path: P) -> Result<HashMap<String, f64>> {
    let catalog = load_compressor_catalog(path)?;
    Ok(ratios_from_catalog(&catalog))
}

pub fn ratios_from_catalog(catalog: &CompressorCatalog) -> HashMap<String, f64> {
    catalog
        .stations
        .iter()
        .map(|(station_id, station)| {
            (
                station_id.clone(),
                stage_ratio_heuristic(station.max_serial_stages()),
            )
        })
        .collect()
}

fn stage_ratio_heuristic(stages: usize) -> f64 {
    (1.08_f64).powi(stages.max(1) as i32).clamp(1.0, 5.0)
}

#[cfg(test)]
mod tests {
    use super::parse::parse_compressor_catalog_from_str;
    use super::ratios_from_catalog;

    #[test]
    fn test_backward_compatible_ratio_heuristic() {
        let xml = r#"
<compressorStations>
  <compressorStation id="CS-A">
    <configurations>
      <configuration nrOfSerialStages="2"/>
      <configuration nrOfSerialStages="1"/>
    </configurations>
  </compressorStation>
  <compressorStation id="CS-B">
    <configurations>
      <configuration nrOfSerialStages="1"/>
    </configurations>
  </compressorStation>
</compressorStations>
"#;
        let catalog = parse_compressor_catalog_from_str(xml).expect("catalog parse");
        let ratios = ratios_from_catalog(&catalog);

        assert!(ratios.contains_key("CS-A"));
        assert!(ratios.contains_key("CS-B"));
        assert!(ratios["CS-A"] > ratios["CS-B"]);
        assert!(ratios["CS-B"] >= 1.0);
    }
}
