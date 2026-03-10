use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

pub fn load_compressor_ratios<P: AsRef<Path>>(path: P) -> Result<HashMap<String, f64>> {
    let raw = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;
    parse_compressor_ratios_from_str(&raw)
}

fn parse_compressor_ratios_from_str(raw: &str) -> Result<HashMap<String, f64>> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_station: Option<String> = None;
    let mut station_stages: HashMap<String, usize> = HashMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "compressorstation" {
                    let station_id = read_attr_string(&reader, &e, "id");
                    if let Some(id) = station_id {
                        station_stages.entry(id.clone()).or_insert(1);
                        current_station = Some(id);
                    }
                } else if name == "configuration" {
                    let Some(station) = current_station.as_ref() else {
                        buf.clear();
                        continue;
                    };
                    if let Some(stages) =
                        read_attr_string(&reader, &e, "nrOfSerialStages").and_then(parse_usize)
                    {
                        let entry = station_stages.entry(station.clone()).or_insert(1);
                        *entry = (*entry).max(stages.max(1));
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "compressorstation" {
                    if let Some(id) = read_attr_string(&reader, &e, "id") {
                        station_stages.entry(id).or_insert(1);
                    }
                } else if name == "configuration" {
                    let Some(station) = current_station.as_ref() else {
                        buf.clear();
                        continue;
                    };
                    if let Some(stages) =
                        read_attr_string(&reader, &e, "nrOfSerialStages").and_then(parse_usize)
                    {
                        let entry = station_stages.entry(station.clone()).or_insert(1);
                        *entry = (*entry).max(stages.max(1));
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "compressorstation" {
                    current_station = None;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err).with_context(|| "parsing XML compressor stations (.cs)"),
        }
        buf.clear();
    }

    // Heuristique MVP: ratio max par étage série (clamp de sûreté).
    let ratios = station_stages
        .into_iter()
        .map(|(id, stages)| {
            let ratio = (1.08_f64).powi(stages as i32).clamp(1.0, 1.6);
            (id, ratio)
        })
        .collect();
    Ok(ratios)
}

fn parse_usize(s: String) -> Option<usize> {
    s.parse::<usize>().ok()
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_ascii_lowercase();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

fn read_attr_string(
    reader: &Reader<&[u8]>,
    e: &quick_xml::events::BytesStart<'_>,
    key: &str,
) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        let attr_key = local_name(attr.key.as_ref());
        if attr_key != key.to_ascii_lowercase() {
            return None;
        }
        attr.decode_and_unescape_value(reader.decoder())
            .ok()
            .map(|v| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compressor_ratios_from_cs_xml() {
        let xml = r#"
<compressorStations>
  <compressorStation id="CS-A">
    <configurations>
      <configuration confId="1" nrOfSerialStages="2"/>
      <configuration confId="2" nrOfSerialStages="1"/>
    </configurations>
  </compressorStation>
  <compressorStation id="CS-B">
    <configurations>
      <configuration confId="1" nrOfSerialStages="1"/>
    </configurations>
  </compressorStation>
</compressorStations>"#;

        let ratios = parse_compressor_ratios_from_str(xml).expect("parse ratios");
        assert!(ratios.contains_key("CS-A"));
        assert!(ratios.contains_key("CS-B"));
        assert!(
            ratios["CS-A"] > ratios["CS-B"],
            "two serial stages should increase max ratio"
        );
        assert!(ratios["CS-B"] >= 1.0);
    }
}
