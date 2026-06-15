use csv::ReaderBuilder;
use serde::Deserialize;

use super::{MeasurementType, ScadaMeasurement};

#[derive(Debug, Deserialize)]
struct RawScadaRecord {
    #[serde(alias = "node_id", alias = "pipe_id", alias = "asset_id")]
    id: String,
    #[serde(alias = "type", alias = "kind")]
    measurement_type: String,
    #[serde(alias = "measurement", alias = "measured_value")]
    value: f64,
    #[serde(default, alias = "ts")]
    timestamp: Option<String>,
    #[serde(default, alias = "sigma")]
    uncertainty: Option<f64>,
}

pub fn parse_scada_csv(content: &str) -> Vec<ScadaMeasurement> {
    let mut reader = ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());

    reader
        .deserialize::<RawScadaRecord>()
        .filter_map(Result::ok)
        .filter_map(|record| {
            let measurement_type = parse_measurement_type(&record.measurement_type)?;
            if !record.value.is_finite() {
                return None;
            }
            Some(ScadaMeasurement {
                id: record.id.trim().to_string(),
                measurement_type,
                value: record.value,
                timestamp: record.timestamp.and_then(clean_optional_text),
                uncertainty: record.uncertainty.and_then(|u| {
                    if u.is_finite() && u > 0.0 {
                        Some(u)
                    } else {
                        None
                    }
                }),
            })
        })
        .collect()
}

fn parse_measurement_type(raw: &str) -> Option<MeasurementType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pressure" | "pression" | "p" => Some(MeasurementType::Pressure),
        "flow" | "debit" | "débit" | "q" => Some(MeasurementType::Flow),
        _ => None,
    }
}

fn clean_optional_text(raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{MeasurementType, parse_scada_csv};

    #[test]
    fn parse_scada_csv_reads_measurements() {
        let csv = r#"id,measurement_type,value,timestamp,uncertainty
N1,pressure,67.2,2026-01-01T00:00:00Z,0.2
P42,flow,12.5,,1.5
"#;
        let parsed = parse_scada_csv(csv);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "N1");
        assert_eq!(parsed[0].measurement_type, MeasurementType::Pressure);
        assert_eq!(parsed[0].timestamp.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(parsed[1].measurement_type, MeasurementType::Flow);
        assert_eq!(parsed[1].uncertainty, Some(1.5));
    }

    #[test]
    fn parse_scada_csv_skips_unknown_types() {
        let csv = r#"id,measurement_type,value
N1,temperature,15.0
N2,pressure,64.0
"#;
        let parsed = parse_scada_csv(csv);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "N2");
    }
}
