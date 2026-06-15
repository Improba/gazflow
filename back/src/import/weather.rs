//! Import CSV de pas météo horaires (P9).

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::solver::timeseries::WeatherStep;

/// Parse un CSV météo en pas horaires (`hour,t_ext_c` avec alias de colonnes).
pub fn parse_weather_csv(content: &str) -> Result<Vec<WeatherStep>> {
    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader.headers()?.clone();
    let hour_col = find_column(&headers, &["hour", "heure", "h"])?;
    let t_ext_col = find_column(&headers, &["t_ext_c", "temperature", "t_ext", "t"])?;

    let mut weather = Vec::new();
    let mut seen_hours = std::collections::HashSet::new();
    for row in reader.records() {
        let row = row?;
        let hour: u8 = field(&row, hour_col, "hour")?.parse().context("hour")?;
        if hour > 23 {
            bail!("invalid hour {hour} (expected 0-23)");
        }
        if !seen_hours.insert(hour) {
            bail!("duplicate hour {hour} in weather csv");
        }
        let t_ext_c: f64 = field(&row, t_ext_col, "t_ext_c")?
            .parse()
            .context("t_ext_c")?;
        if !t_ext_c.is_finite() {
            bail!("non-finite t_ext_c at hour {hour}");
        }
        weather.push(WeatherStep { hour, t_ext_c });
    }
    if weather.is_empty() {
        bail!("weather csv must contain at least one row");
    }
    Ok(weather)
}

pub fn load_weather_csv(path: &Path) -> Result<Vec<WeatherStep>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open weather csv: {}", path.display()))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    parse_weather_csv(&content)
}

fn find_column(headers: &csv::StringRecord, aliases: &[&str]) -> Result<usize> {
    for (idx, header) in headers.iter().enumerate() {
        let h = header.trim().to_ascii_lowercase();
        if aliases.iter().any(|a| h == *a) {
            return Ok(idx);
        }
    }
    bail!(
        "missing CSV column (expected one of: {})",
        aliases.join(", ")
    )
}

fn field(record: &csv::StringRecord, idx: usize, name: &str) -> Result<String> {
    record
        .get(idx)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing field {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::test_corpus_root;

    #[test]
    fn test_parse_weather_csv_with_aliases() {
        let csv = "heure,temperature\n0,-6\n12,-2.5\n23,-5\n";
        let weather = parse_weather_csv(csv).expect("parse");
        assert_eq!(weather.len(), 3);
        assert_eq!(weather[1].hour, 12);
        assert!((weather[1].t_ext_c + 2.5).abs() < 1e-9);
    }

    #[test]
    fn test_parse_weather_csv_rejects_duplicate_hour() {
        let csv = "hour,t_ext_c\n0,-6\n0,-5\n";
        let err = parse_weather_csv(csv).expect_err("duplicate");
        assert!(err.to_string().contains("duplicate hour"));
    }

    #[test]
    fn test_parse_weather_csv_rejects_invalid_hour() {
        let csv = "h,t\n24,-4\n";
        let err = parse_weather_csv(csv).expect_err("invalid hour");
        assert!(err.to_string().contains("invalid hour"));
    }

    #[test]
    fn test_load_weather_winter_day_corpus() {
        let path = test_corpus_root().join("synthetic/demand/weather-winter-day.csv");
        let weather = load_weather_csv(&path).expect("load");
        assert_eq!(weather.len(), 24);
        assert_eq!(weather[0].hour, 0);
        assert_eq!(weather[23].hour, 23);
    }
}
