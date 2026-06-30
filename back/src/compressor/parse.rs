use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::catalog::CompressorCatalog;
use super::station::TurboMeasurement;

#[derive(Debug, Default)]
struct PendingMeasurement {
    speed_rpm: Option<f64>,
    head_kj_per_kg: Option<f64>,
    flow_m3_s: Option<f64>,
}

impl PendingMeasurement {
    fn build(self) -> Option<TurboMeasurement> {
        let speed_rpm = self.speed_rpm?;
        let head_kj_per_kg = self.head_kj_per_kg?;
        let flow_m3_s = self.flow_m3_s?;
        if !speed_rpm.is_finite() || !head_kj_per_kg.is_finite() || !flow_m3_s.is_finite() {
            return None;
        }
        Some(TurboMeasurement {
            speed_rpm,
            head_kj_per_kg,
            flow_m3_s,
        })
    }
}

pub fn load_compressor_catalog<P: AsRef<Path>>(path: P) -> Result<CompressorCatalog> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref).with_context(|| format!("lecture de {:?}", path_ref))?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    parse_compressor_catalog(&mut reader)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasurementBlock {
    None,
    Surgeline,
    Characteristic,
}

fn parse_compressor_catalog<R: BufRead>(reader: &mut Reader<R>) -> Result<CompressorCatalog> {
    let mut catalog = CompressorCatalog::default();
    let mut buf = Vec::new();
    let mut current_station: Option<String> = None;
    let mut current_turbo: Option<String> = None;
    let mut measurement_block = MeasurementBlock::None;
    let mut current_measurement: Option<PendingMeasurement> = None;

    fn push_measurement(
        catalog: &mut CompressorCatalog,
        station_id: &str,
        turbo_id: Option<&str>,
        block: MeasurementBlock,
        measurement: TurboMeasurement,
    ) {
        let station = catalog.station_mut(station_id);
        match block {
            MeasurementBlock::Characteristic => {
                station.push_characteristic_measurement(measurement.clone());
                if let Some(turbo_id) = turbo_id {
                    station
                        .turbo_mut(turbo_id)
                        .push_characteristic_measurement(measurement);
                }
            }
            MeasurementBlock::Surgeline => {
                station.push_surgeline_measurement(measurement.clone());
                if let Some(turbo_id) = turbo_id {
                    station
                        .turbo_mut(turbo_id)
                        .push_surgeline_measurement(measurement);
                }
            }
            MeasurementBlock::None => {}
        }
    }

    fn parse_coeff_idx(name: &str, prefix: &str, max_len: usize) -> Option<usize> {
        name.strip_prefix(prefix)?
            .parse::<usize>()
            .ok()?
            .checked_sub(1)
            .filter(|idx| *idx < max_len)
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "compressorstation" => {
                        if let Some(station_id) = read_attr_string(reader, &e, "id") {
                            catalog.station_mut(&station_id);
                            current_station = Some(station_id);
                        }
                    }
                    "turbocompressor" => {
                        if let (Some(station_id), Some(turbo_id)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "id"),
                        ) {
                            catalog.station_mut(station_id).turbo_mut(&turbo_id);
                            current_turbo = Some(turbo_id);
                        }
                    }
                    "surgelinemeasurements" => {
                        measurement_block = MeasurementBlock::Surgeline;
                    }
                    "characteristicdiagrammeasurements" => {
                        measurement_block = MeasurementBlock::Characteristic;
                    }
                    "measurement" => {
                        if measurement_block != MeasurementBlock::None && current_station.is_some()
                        {
                            current_measurement = Some(PendingMeasurement::default());
                        }
                    }
                    "configuration" => {
                        if let (Some(station_id), Some(stages)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "nrOfSerialStages").and_then(parse_usize),
                        ) {
                            let conf_id = read_attr_string(reader, &e, "confId");
                            catalog
                                .station_mut(station_id)
                                .push_configuration(conf_id, stages);
                        }
                    }
                    "speedmin" => {
                        if let (Some(station_id), Some(value)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                        ) {
                            let station = catalog.station_mut(station_id);
                            station.update_speed_min(value);
                            if let Some(turbo_id) = current_turbo.as_deref() {
                                station.turbo_mut(turbo_id).update_speed_min(value);
                            }
                        }
                    }
                    "speedmax" => {
                        if let (Some(station_id), Some(value)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                        ) {
                            let station = catalog.station_mut(station_id);
                            station.update_speed_max(value);
                            if let Some(turbo_id) = current_turbo.as_deref() {
                                station.turbo_mut(turbo_id).update_speed_max(value);
                            }
                        }
                    }
                    "speed" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.speed_rpm = Some(value);
                            }
                        }
                    }
                    "adiabatichead" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.head_kj_per_kg = Some(value);
                            }
                        }
                    }
                    "volumetricflowrate" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.flow_m3_s = Some(value);
                            }
                        }
                    }
                    _ if name.starts_with("n_isoline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "n_isoline_coeff_", 9),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_biquadratic_coeff(coeff_idx, value);
                        }
                    }
                    _ if name.starts_with("surgeline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "surgeline_coeff_", 3),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_surgeline_coeff(coeff_idx, value);
                        }
                    }
                    _ if name.starts_with("chokeline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "chokeline_coeff_", 3),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_chokeline_coeff(coeff_idx, value);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "compressorstation" => {
                        if let Some(station_id) = read_attr_string(reader, &e, "id") {
                            catalog.station_mut(&station_id);
                        }
                    }
                    "turbocompressor" => {
                        if let (Some(station_id), Some(turbo_id)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "id"),
                        ) {
                            catalog.station_mut(station_id).turbo_mut(&turbo_id);
                            current_turbo = Some(turbo_id);
                        }
                    }
                    "configuration" => {
                        if let (Some(station_id), Some(stages)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "nrOfSerialStages").and_then(parse_usize),
                        ) {
                            let conf_id = read_attr_string(reader, &e, "confId");
                            catalog
                                .station_mut(station_id)
                                .push_configuration(conf_id, stages);
                        }
                    }
                    "speedmin" => {
                        if let (Some(station_id), Some(value)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                        ) {
                            let station = catalog.station_mut(station_id);
                            station.update_speed_min(value);
                            if let Some(turbo_id) = current_turbo.as_deref() {
                                station.turbo_mut(turbo_id).update_speed_min(value);
                            }
                        }
                    }
                    "speedmax" => {
                        if let (Some(station_id), Some(value)) = (
                            current_station.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                        ) {
                            let station = catalog.station_mut(station_id);
                            station.update_speed_max(value);
                            if let Some(turbo_id) = current_turbo.as_deref() {
                                station.turbo_mut(turbo_id).update_speed_max(value);
                            }
                        }
                    }
                    "speed" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.speed_rpm = Some(value);
                            }
                        }
                    }
                    "adiabatichead" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.head_kj_per_kg = Some(value);
                            }
                        }
                    }
                    "volumetricflowrate" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(measurement), Some(value)) = (
                                current_measurement.as_mut(),
                                read_attr_string(reader, &e, "value").and_then(parse_f64),
                            ) {
                                measurement.flow_m3_s = Some(value);
                            }
                        }
                    }
                    _ if name.starts_with("n_isoline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "n_isoline_coeff_", 9),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_biquadratic_coeff(coeff_idx, value);
                        }
                    }
                    _ if name.starts_with("surgeline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "surgeline_coeff_", 3),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_surgeline_coeff(coeff_idx, value);
                        }
                    }
                    _ if name.starts_with("chokeline_coeff_") => {
                        if let (Some(station_id), Some(turbo_id), Some(value), Some(coeff_idx)) = (
                            current_station.as_deref(),
                            current_turbo.as_deref(),
                            read_attr_string(reader, &e, "value").and_then(parse_f64),
                            parse_coeff_idx(name.as_str(), "chokeline_coeff_", 3),
                        ) {
                            catalog
                                .station_mut(station_id)
                                .turbo_mut(turbo_id)
                                .set_chokeline_coeff(coeff_idx, value);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "measurement" => {
                        if measurement_block != MeasurementBlock::None {
                            if let (Some(station_id), Some(measurement)) =
                                (current_station.as_deref(), current_measurement.take())
                            {
                                if let Some(measurement) = measurement.build() {
                                    push_measurement(
                                        &mut catalog,
                                        station_id,
                                        current_turbo.as_deref(),
                                        measurement_block,
                                        measurement,
                                    );
                                }
                            }
                        } else {
                            current_measurement = None;
                        }
                    }
                    "surgelinemeasurements" | "characteristicdiagrammeasurements" => {
                        measurement_block = MeasurementBlock::None;
                    }
                    "compressorstation" => {
                        current_station = None;
                        current_turbo = None;
                    }
                    "turbocompressor" => {
                        current_turbo = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err).with_context(|| "parsing XML compressor stations (.cs)"),
        }
        buf.clear();
    }

    Ok(catalog)
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_ascii_lowercase();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

fn read_attr_string<R: BufRead>(
    reader: &Reader<R>,
    e: &BytesStart<'_>,
    key: &str,
) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        let attr_key = local_name(attr.key.as_ref());
        if attr_key != key.to_ascii_lowercase() {
            return None;
        }
        attr.decode_and_unescape_value(reader.decoder())
            .ok()
            .map(|value| value.to_string())
    })
}

fn parse_f64(raw: String) -> Option<f64> {
    raw.parse::<f64>().ok()
}

fn parse_usize(raw: String) -> Option<usize> {
    raw.parse::<usize>().ok()
}

#[cfg(test)]
pub(crate) fn parse_compressor_catalog_from_str(raw: &str) -> Result<CompressorCatalog> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);
    parse_compressor_catalog(&mut reader)
}

#[cfg(test)]
mod tests {
    use super::parse_compressor_catalog_from_str;

    #[test]
    fn test_parse_catalog_extracts_surgeline_measurements_and_stages() {
        let xml = r#"
<compressorStations>
  <compressorStation id="CS-A">
    <compressors>
      <turboCompressor id="turbo-1">
        <speedMin value="4700" unit="per_min"/>
        <speedMax value="6500" unit="per_min"/>
        <surgelineMeasurements>
          <measurement>
            <speed value="4700" unit="per_min"/>
            <adiabaticHead value="63.6" unit="kJ_per_kg"/>
            <volumetricFlowrate value="0.20" unit="m_cube_per_s"/>
          </measurement>
          <measurement>
            <speed value="6500" unit="per_min"/>
            <adiabaticHead value="87.5" unit="kJ_per_kg"/>
            <volumetricFlowrate value="0.40" unit="m_cube_per_s"/>
          </measurement>
        </surgelineMeasurements>
        <characteristicDiagramMeasurements>
          <measurement>
            <speed value="4700" unit="per_min"/>
            <adiabaticHead value="999.0" unit="kJ_per_kg"/>
            <volumetricFlowrate value="9.99" unit="m_cube_per_s"/>
          </measurement>
        </characteristicDiagramMeasurements>
      </turboCompressor>
    </compressors>
    <configurations>
      <configuration confId="1" nrOfSerialStages="2"/>
      <configuration confId="2" nrOfSerialStages="1"/>
    </configurations>
  </compressorStation>
  <compressorStation id="CS-B"/>
</compressorStations>
"#;

        let catalog = parse_compressor_catalog_from_str(xml).expect("catalog parse");
        assert!(catalog.stations.contains_key("CS-A"));
        assert!(catalog.stations.contains_key("CS-B"));

        let station = catalog.stations.get("CS-A").expect("CS-A");
        assert_eq!(station.max_serial_stages(), 2);
        assert_eq!(station.speed_bounds(), Some((4_700.0, 6_500.0)));
        assert_eq!(station.characteristic_measurements.len(), 1);
        assert_eq!(station.surgeline_measurements.len(), 2);
        assert_eq!(station.turbos.len(), 1);
        assert!(station.turbos.contains_key("turbo-1"));
        assert_eq!(
            station
                .turbo("turbo-1")
                .expect("turbo")
                .surgeline_measurements
                .len(),
            2
        );
        assert_eq!(station.map_measurements().len(), 1);
        assert!((station.map_measurements()[0].head_kj_per_kg - 999.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_catalog_handles_namespaced_elements() {
        let xml = r#"
<framework:compressorStations xmlns:framework="http://gaslib.zib.de/CompressorStations">
  <framework:compressorStation id="CS-C">
    <framework:compressors>
      <framework:turboCompressor id="turbo-1">
        <framework:surgelineMeasurements>
          <framework:measurement>
            <framework:speed value="5000"/>
            <framework:adiabaticHead value="70.0"/>
            <framework:volumetricFlowrate value="0.3"/>
          </framework:measurement>
        </framework:surgelineMeasurements>
      </framework:turboCompressor>
    </framework:compressors>
  </framework:compressorStation>
</framework:compressorStations>
"#;

        let catalog = parse_compressor_catalog_from_str(xml).expect("catalog parse");
        let station = catalog.stations.get("CS-C").expect("CS-C");
        assert_eq!(station.surgeline_measurements.len(), 1);
        assert!(station.characteristic_measurements.is_empty());
        assert!(station.turbo("turbo-1").is_some());
    }

    #[test]
    fn test_parse_catalog_extracts_biquadratic_and_bound_curves_per_turbo() {
        let xml = r#"
<compressorStations>
  <compressorStation id="CS-A">
    <compressors>
      <turboCompressor id="config_2">
        <n_isoline_coeff_1 value="10.0"/>
        <n_isoline_coeff_2 value="1.0"/>
        <n_isoline_coeff_3 value="2.0"/>
        <n_isoline_coeff_4 value="3.0"/>
        <n_isoline_coeff_5 value="4.0"/>
        <n_isoline_coeff_6 value="5.0"/>
        <n_isoline_coeff_7 value="6.0"/>
        <n_isoline_coeff_8 value="7.0"/>
        <n_isoline_coeff_9 value="8.0"/>
        <surgeline_coeff_1 value="0.1"/>
        <surgeline_coeff_2 value="0.2"/>
        <surgeline_coeff_3 value="0.3"/>
        <chokeline_coeff_1 value="1.1"/>
        <chokeline_coeff_2 value="1.2"/>
        <chokeline_coeff_3 value="1.3"/>
      </turboCompressor>
    </compressors>
  </compressorStation>
</compressorStations>
"#;

        let catalog = parse_compressor_catalog_from_str(xml).expect("catalog parse");
        let station = catalog.stations.get("CS-A").expect("CS-A");
        let turbo = station.turbo("config_2").expect("config_2 turbo");
        assert_eq!(
            turbo.biquadratic_head_coeffs,
            Some([10.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0])
        );
        assert_eq!(turbo.surgeline_curve, Some([0.1, 0.2, 0.3]));
        assert_eq!(turbo.chokeline_curve, Some([1.1, 1.2, 1.3]));
    }

    #[test]
    fn test_load_gaslib_582_catalog_if_present() {
        let path = std::path::Path::new("dat/GasLib-582.cs");
        if !path.exists() {
            eprintln!("skip: GasLib-582.cs not found");
            return;
        }
        let catalog = super::load_compressor_catalog(path).expect("582 catalog");
        assert_eq!(catalog.stations.len(), 5);
        for id in [
            "compressorStation_1",
            "compressorStation_2",
            "compressorStation_3",
            "compressorStation_4",
            "compressorStation_5",
        ] {
            let station = catalog.station(id).expect(id);
            assert!(
                !station.characteristic_measurements.is_empty()
                    || !station.surgeline_measurements.is_empty()
            );
        }
    }
}
