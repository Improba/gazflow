use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceSolution {
    pub pressures: HashMap<String, f64>,
    pub flows: HashMap<String, f64>,
}

pub fn load_reference_solution<P: AsRef<Path>>(path: P) -> Result<ReferenceSolution> {
    let raw = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("lecture de {:?}", path.as_ref()))?;
    parse_reference_solution_from_str(&raw)
}

fn parse_reference_solution_from_str(raw: &str) -> Result<ReferenceSolution> {
    if let Ok(parsed) = parse_text_solution(raw) {
        return Ok(parsed);
    }
    if let Ok(parsed) = parse_xml_solution(raw) {
        return Ok(parsed);
    }
    bail!(
        "format de solution de référence non supporté (attendu: texte CSV-like ou XML avec node/pipe)"
    )
}

fn parse_text_solution(raw: &str) -> Result<ReferenceSolution> {
    let mut pressures = HashMap::new();
    let mut flows = HashMap::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        let tokens: Vec<&str> = line
            .split(|c: char| c == ',' || c == ';' || c.is_ascii_whitespace())
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.len() < 2 {
            continue;
        }

        match tokens.len() {
            2 => {
                // Fallback simple: "node_id pressure_bar"
                let id = tokens[0].to_string();
                let value = tokens[1]
                    .parse::<f64>()
                    .with_context(|| format!("valeur numérique invalide: {}", tokens[1]))?;
                pressures.insert(id, value);
            }
            _ => {
                // Formats tolérés:
                // - node,id,value
                // - pipe,id,value
                // - id,node,value
                // - id,pipe,value
                let (kind, id, value_token) =
                    if is_pressure_kind(tokens[0]) || is_flow_kind(tokens[0]) {
                        (tokens[0], tokens[1], tokens[2])
                    } else if is_pressure_kind(tokens[1]) || is_flow_kind(tokens[1]) {
                        (tokens[1], tokens[0], tokens[2])
                    } else {
                        continue;
                    };
                let value = value_token
                    .parse::<f64>()
                    .with_context(|| format!("valeur numérique invalide: {value_token}"))?;
                if is_pressure_kind(kind) {
                    pressures.insert(id.to_string(), value);
                } else {
                    flows.insert(id.to_string(), value);
                }
            }
        }
    }

    if pressures.is_empty() && flows.is_empty() {
        bail!("aucune donnée de solution trouvée dans le format texte");
    }
    Ok(ReferenceSolution { pressures, flows })
}

fn parse_xml_solution(raw: &str) -> Result<ReferenceSolution> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut pressures = HashMap::new();
    let mut flows = HashMap::new();
    let mut current_node_id: Option<String> = None;
    let mut current_pipe_id: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                handle_xml_start(
                    &reader,
                    &e,
                    &mut current_node_id,
                    &mut current_pipe_id,
                    &mut pressures,
                    &mut flows,
                )?;
            }
            Ok(Event::Empty(e)) => {
                handle_xml_start(
                    &reader,
                    &e,
                    &mut current_node_id,
                    &mut current_pipe_id,
                    &mut pressures,
                    &mut flows,
                )?;
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if is_node_element(&name) {
                    current_node_id = None;
                }
                if is_pipe_element(&name) {
                    current_pipe_id = None;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err).with_context(|| "parsing XML solution de référence"),
        }
        buf.clear();
    }

    if pressures.is_empty() && flows.is_empty() {
        bail!("aucune donnée de solution trouvée dans le format XML");
    }

    Ok(ReferenceSolution { pressures, flows })
}

fn handle_xml_start(
    reader: &Reader<&[u8]>,
    e: &BytesStart<'_>,
    current_node_id: &mut Option<String>,
    current_pipe_id: &mut Option<String>,
    pressures: &mut HashMap<String, f64>,
    flows: &mut HashMap<String, f64>,
) -> Result<()> {
    let name = local_name(e.name().as_ref());

    let mut id_attr: Option<String> = None;
    let mut pressure_attr: Option<f64> = None;
    let mut flow_attr: Option<f64> = None;
    let mut value_attr: Option<f64> = None;

    for attr in e.attributes().flatten() {
        let key = local_name(attr.key.as_ref());
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .with_context(|| "décodage attribut XML solution")?
            .to_string();

        if is_id_key(&key) {
            id_attr = Some(value);
            continue;
        }

        if let Ok(v) = value.parse::<f64>() {
            if is_pressure_key(&key) {
                pressure_attr = Some(v);
            } else if is_flow_key(&key) {
                flow_attr = Some(v);
            } else if key == "value" {
                value_attr = Some(v);
            }
        }
    }

    if is_node_element(&name) {
        *current_node_id = id_attr.clone().or_else(|| current_node_id.clone());
    }
    if is_pipe_element(&name) {
        *current_pipe_id = id_attr.clone().or_else(|| current_pipe_id.clone());
    }

    if let (Some(id), Some(value)) = (id_attr.clone(), pressure_attr) {
        pressures.insert(id, value);
    }
    if let (Some(id), Some(value)) = (id_attr.clone(), flow_attr) {
        flows.insert(id, value);
    }

    if is_pressure_element(&name) {
        if let (Some(id), Some(value)) = (current_node_id.as_ref(), pressure_attr.or(value_attr)) {
            pressures.insert(id.clone(), value);
        }
    }
    if is_flow_element(&name) {
        if let (Some(id), Some(value)) = (current_pipe_id.as_ref(), flow_attr.or(value_attr)) {
            flows.insert(id.clone(), value);
        }
    }

    Ok(())
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_ascii_lowercase();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

fn is_pressure_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "node" | "pressure" | "p"
    )
}

fn is_flow_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "pipe" | "flow" | "q" | "arc" | "edge" | "connection"
    )
}

fn is_node_element(name: &str) -> bool {
    name.contains("node") || name.contains("vertex")
}

fn is_pipe_element(name: &str) -> bool {
    name.contains("pipe")
        || name.contains("arc")
        || name.contains("edge")
        || name.contains("connection")
}

fn is_pressure_element(name: &str) -> bool {
    name == "pressure" || name == "p" || name.contains("pressure")
}

fn is_flow_element(name: &str) -> bool {
    name == "flow" || name == "q" || name.contains("flow")
}

fn is_id_key(key: &str) -> bool {
    matches!(
        key,
        "id" | "nodeid" | "pipeid" | "arcid" | "edgeid" | "name"
    )
}

fn is_pressure_key(key: &str) -> bool {
    key == "pressure" || key == "pressurebar" || key == "p" || key.contains("pressure")
}

fn is_flow_key(key: &str) -> bool {
    key == "flow" || key == "q" || key.contains("flow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_solution_csv_like() {
        let raw = r#"
node,N1,70.0
node,N2,65.5
pipe,P1,10.2
id,node,0.0
"#;
        let parsed = parse_reference_solution_from_str(raw).expect("text solution parsing");
        assert_eq!(parsed.pressures.get("N1"), Some(&70.0));
        assert_eq!(parsed.pressures.get("N2"), Some(&65.5));
        assert_eq!(parsed.flows.get("P1"), Some(&10.2));
        assert_eq!(parsed.pressures.get("id"), Some(&0.0));
    }

    #[test]
    fn test_parse_text_solution_two_columns_defaults_to_pressure() {
        let raw = "N1 70.0\nN2 68.5\n";
        let parsed = parse_reference_solution_from_str(raw).expect("text solution parsing");
        assert_eq!(parsed.pressures.get("N1"), Some(&70.0));
        assert_eq!(parsed.pressures.get("N2"), Some(&68.5));
        assert!(parsed.flows.is_empty());
    }

    #[test]
    fn test_parse_xml_solution() {
        let raw = r#"
<solution>
  <nodes>
    <node id="N1" pressure="70.0"/>
    <node id="N2">
      <pressure value="65.0"/>
    </node>
  </nodes>
  <pipes>
    <pipe id="P1" flow="10.5"/>
    <pipe id="P2">
      <flow value="-2.5"/>
    </pipe>
  </pipes>
</solution>
"#;
        let parsed = parse_reference_solution_from_str(raw).expect("xml solution parsing");
        assert_eq!(parsed.pressures.get("N1"), Some(&70.0));
        assert_eq!(parsed.pressures.get("N2"), Some(&65.0));
        assert_eq!(parsed.flows.get("P1"), Some(&10.5));
        assert_eq!(parsed.flows.get("P2"), Some(&-2.5));
    }

    #[test]
    fn test_parse_solution_unsupported_format() {
        let err = parse_reference_solution_from_str("not a solution")
            .expect_err("invalid content should be rejected");
        assert!(
            err.to_string().contains("non supporté"),
            "unexpected error: {err:#}"
        );
    }
}
