use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::graph::{ConnectionKind, EquipmentSpec, RawNode, RawNodeRole, RawPipe};
use crate::solver::GasComposition;

#[derive(Debug, Clone, Deserialize)]
pub struct MappingConfig {
    pub format: String,
    #[serde(default)]
    pub nodes: NodeMapping,
    #[serde(default)]
    pub pipes: PipeMapping,
    #[serde(default)]
    pub defaults: MappingDefaults,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeMapping {
    pub id_field: Option<String>,
    pub type_field: Option<String>,
    #[serde(default)]
    pub type_mapping: HashMap<String, String>,
    pub pressure_fixed_field: Option<String>,
    pub lon_field: Option<String>,
    pub lat_field: Option<String>,
    pub elevation_field: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipeMapping {
    pub file: Option<String>,
    pub id_field: Option<String>,
    pub from_field: Option<String>,
    pub to_field: Option<String>,
    pub length_field: Option<String>,
    pub length_unit: Option<String>,
    pub diameter_field: Option<String>,
    pub diameter_unit: Option<String>,
    pub roughness_field: Option<String>,
    pub roughness_unit: Option<String>,
    pub roughness_default_mm: Option<f64>,
    pub material_field: Option<String>,
    pub type_field: Option<String>,
    #[serde(default)]
    pub type_mapping: HashMap<String, String>,
    pub regulator_setpoint_field: Option<String>,
    pub regulator_delta_p_min_field: Option<String>,
    pub control_valve_cv_field: Option<String>,
    pub control_valve_opening_field: Option<String>,
    pub delivery_min_pressure_field: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MappingDefaults {
    pub sink_demand_m3s: Option<f64>,
    pub gas_composition: Option<GasComposition>,
}

impl MappingDefaults {
    pub fn resolved_gas_composition(&self) -> GasComposition {
        self.gas_composition.unwrap_or_default().normalize()
    }
}

pub fn load_mapping(path: &Path) -> Result<MappingConfig> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("lecture mapping {:?}", path))?;
    load_mapping_from_str(&raw)
}

pub fn load_mapping_from_str(raw: &str) -> Result<MappingConfig> {
    serde_yaml::from_str(raw).context("parsing YAML mapping")
}

pub fn resolve_field<'a>(props: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = props;
    for segment in path.split('.') {
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            let idx: usize = segment[bracket + 1..].trim_end_matches(']').parse().ok()?;
            if !key.is_empty() {
                current = current.get(key)?;
            }
            current = current.get(idx)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current)
}

pub fn resolve_string(props: &Value, path: &str) -> Option<String> {
    resolve_field(props, path).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

pub fn resolve_f64(props: &Value, path: &str) -> Option<f64> {
    resolve_field(props, path).and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn parse_role(type_value: &str, mapping: &NodeMapping) -> RawNodeRole {
    if let Some(mapped) = mapping.type_mapping.get(type_value) {
        return RawNodeRole::from_label(mapped);
    }
    RawNodeRole::from_label(type_value)
}

fn length_to_km(value: f64, unit: Option<&str>) -> f64 {
    match unit {
        Some("m") | Some("meter") | Some("meters") => value / 1000.0,
        Some("km") | None => value,
        _ => value,
    }
}

fn diameter_to_mm(value: f64, unit: Option<&str>) -> f64 {
    match unit {
        Some("m") => value * 1000.0,
        Some("mm") | None => value,
        _ => value,
    }
}

fn roughness_to_mm(value: f64, unit: Option<&str>) -> f64 {
    match unit {
        Some("m") => value * 1000.0,
        Some("mm") | None => value,
        _ => value,
    }
}

pub fn raw_node_from_properties(props: &Value, mapping: &MappingConfig) -> Result<RawNode> {
    let nm = &mapping.nodes;
    let id_field = nm
        .id_field
        .as_deref()
        .context("nodes.id_field requis dans le mapping")?;
    let id = resolve_string(props, id_field).with_context(|| format!("champ nœud {id_field}"))?;

    let role = nm
        .type_field
        .as_deref()
        .and_then(|f| resolve_string(props, f))
        .map(|t| parse_role(&t, nm))
        .unwrap_or(RawNodeRole::Innode);

    let lon = nm.lon_field.as_deref().and_then(|f| resolve_f64(props, f));
    let lat = nm.lat_field.as_deref().and_then(|f| resolve_f64(props, f));
    let height_m = nm
        .elevation_field
        .as_deref()
        .and_then(|f| resolve_f64(props, f))
        .unwrap_or(0.0);
    let pressure_fixed_bar = nm
        .pressure_fixed_field
        .as_deref()
        .and_then(|f| resolve_f64(props, f));

    Ok(RawNode {
        id,
        role,
        x: lon.unwrap_or(0.0),
        y: lat.unwrap_or(0.0),
        lon,
        lat,
        height_m,
        pressure_lower_bar: None,
        pressure_upper_bar: None,
        pressure_fixed_bar,
        flow_min_m3s: None,
        flow_max_m3s: None,
    })
}

fn parse_pipe_kind(type_value: &str, mapping: &PipeMapping) -> ConnectionKind {
    let label = mapping
        .type_mapping
        .get(type_value)
        .map(String::as_str)
        .unwrap_or(type_value);
    ConnectionKind::from_label(label)
}

fn equipment_from_properties(props: &Value, pm: &PipeMapping) -> EquipmentSpec {
    EquipmentSpec {
        regulator_setpoint_bar: pm
            .regulator_setpoint_field
            .as_deref()
            .and_then(|f| resolve_f64(props, f)),
        regulator_delta_p_min_bar: pm
            .regulator_delta_p_min_field
            .as_deref()
            .and_then(|f| resolve_f64(props, f)),
        control_valve_cv: pm
            .control_valve_cv_field
            .as_deref()
            .and_then(|f| resolve_f64(props, f)),
        control_valve_opening_pct: pm
            .control_valve_opening_field
            .as_deref()
            .and_then(|f| resolve_f64(props, f)),
        delivery_min_pressure_bar: pm
            .delivery_min_pressure_field
            .as_deref()
            .and_then(|f| resolve_f64(props, f)),
        compressor_nominal_ratio: None,
        compressor_pressure_cap_ratio: None,
        compressor_pressure_out_max_bar: None,
        control_valve_pressure_out_max_bar: None,
        internal_bypass_required: None,
    }
}

pub fn raw_pipe_from_properties(props: &Value, mapping: &MappingConfig) -> Result<RawPipe> {
    let pm = &mapping.pipes;
    let id_field = pm.id_field.as_deref().context("pipes.id_field requis")?;
    let from_field = pm
        .from_field
        .as_deref()
        .context("pipes.from_field requis")?;
    let to_field = pm.to_field.as_deref().context("pipes.to_field requis")?;
    let length_field = pm
        .length_field
        .as_deref()
        .context("pipes.length_field requis")?;
    let diameter_field = pm
        .diameter_field
        .as_deref()
        .context("pipes.diameter_field requis")?;
    let id = resolve_string(props, id_field).with_context(|| format!("pipe {id_field}"))?;
    let from = resolve_string(props, from_field).with_context(|| format!("pipe {from_field}"))?;
    let to = resolve_string(props, to_field).with_context(|| format!("pipe {to_field}"))?;
    let length_raw = resolve_f64(props, length_field).context("longueur pipe")?;
    let diameter_raw = resolve_f64(props, diameter_field).context("diamètre pipe")?;
    let roughness_mm = pm
        .roughness_field
        .as_deref()
        .and_then(|f| resolve_f64(props, f))
        .map(|v| roughness_to_mm(v, pm.roughness_unit.as_deref()))
        .or(pm.roughness_default_mm)
        .unwrap_or(0.05);

    let kind = pm
        .type_field
        .as_deref()
        .and_then(|f| resolve_string(props, f))
        .map(|t| parse_pipe_kind(&t, pm))
        .unwrap_or(ConnectionKind::Pipe);

    let mut equipment = equipment_from_properties(props, pm);
    if kind == ConnectionKind::DeliveryStation
        && equipment.regulator_setpoint_bar.is_some()
        && equipment.delivery_min_pressure_bar.is_some()
        && equipment.regulator_delta_p_min_bar.is_none()
    {
        equipment.regulator_delta_p_min_bar = Some(0.3);
    }

    Ok(RawPipe {
        id,
        from,
        to,
        kind,
        is_open: true,
        length_km: length_to_km(length_raw, pm.length_unit.as_deref()),
        diameter_mm: diameter_to_mm(diameter_raw, pm.diameter_unit.as_deref()),
        roughness_mm,
        compressor_ratio_max: None,
        flow_min_m3s: None,
        flow_max_m3s: None,
        equipment,
    })
}

pub fn raw_node_from_csv_row(
    headers: &[&str],
    row: &[String],
    mapping: &MappingConfig,
) -> Result<RawNode> {
    let props = csv_row_to_json(headers, row);
    raw_node_from_properties(&props, mapping)
}

pub fn raw_pipe_from_csv_row(
    headers: &[&str],
    row: &[String],
    mapping: &MappingConfig,
) -> Result<RawPipe> {
    let props = csv_row_to_json(headers, row);
    raw_pipe_from_properties(&props, mapping)
}

fn csv_row_to_json(headers: &[&str], row: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for (h, v) in headers.iter().zip(row.iter()) {
        if let Ok(n) = v.parse::<f64>() {
            map.insert(
                (*h).to_string(),
                Value::Number(serde_json::Number::from_f64(n).unwrap_or_else(|| 0.into())),
            );
        } else {
            map.insert((*h).to_string(), Value::String(v.clone()));
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::RawNodeRole;
    use serde_json::json;

    fn minimal_geojson_mapping() -> MappingConfig {
        MappingConfig {
            format: "geojson".into(),
            nodes: NodeMapping {
                id_field: Some("ID_NOEUD".into()),
                type_field: Some("TYPE".into()),
                type_mapping: [
                    ("ALIM".into(), "source".into()),
                    ("LIVR".into(), "sink".into()),
                ]
                .into_iter()
                .collect(),
                pressure_fixed_field: Some("P_CONSIGNE_BAR".into()),
                lon_field: Some("geometry.coordinates[0]".into()),
                lat_field: Some("geometry.coordinates[1]".into()),
                elevation_field: Some("ALTITUDE_M".into()),
            },
            pipes: PipeMapping {
                id_field: Some("ID_CANA".into()),
                from_field: Some("NOEUD_AMONT".into()),
                to_field: Some("NOEUD_AVAL".into()),
                length_field: Some("LONGUEUR_M".into()),
                length_unit: Some("m".into()),
                diameter_field: Some("DIAMETRE_MM".into()),
                diameter_unit: Some("mm".into()),
                roughness_field: None,
                roughness_unit: None,
                roughness_default_mm: Some(0.05),
                material_field: None,
                file: None,
                ..Default::default()
            },
            defaults: MappingDefaults {
                sink_demand_m3s: Some(-50.0),
                gas_composition: None,
            },
        }
    }

    #[test]
    fn resolve_nested_geometry_coordinates() {
        let props = json!({
            "geometry": { "coordinates": [2.3522, 48.8566] }
        });
        assert_eq!(resolve_f64(&props, "geometry.coordinates[0]"), Some(2.3522));
        assert_eq!(
            resolve_f64(&props, "geometry.coordinates[1]"),
            Some(48.8566)
        );
    }

    #[test]
    fn type_mapping_sig_operateur() {
        let mapping = minimal_geojson_mapping();
        let props = json!({
            "ID_NOEUD": "SRC01",
            "TYPE": "ALIM",
            "P_CONSIGNE_BAR": 70.0,
            "ALTITUDE_M": 120.0,
            "geometry": { "coordinates": [2.35, 48.86] }
        });
        let node = raw_node_from_properties(&props, &mapping).expect("node");
        assert_eq!(node.role, RawNodeRole::Source);
        assert_eq!(node.pressure_fixed_bar, Some(70.0));
        assert!((node.height_m - 120.0).abs() < 1e-9);
        assert_eq!(node.lon, Some(2.35));
        assert_eq!(node.lat, Some(48.86));
    }

    #[test]
    fn pipe_length_meters_converted_to_km() {
        let mapping = minimal_geojson_mapping();
        let props = json!({
            "ID_CANA": "P1",
            "NOEUD_AMONT": "SRC01",
            "NOEUD_AVAL": "LVR01",
            "LONGUEUR_M": 12500.0,
            "DIAMETRE_MM": 800.0
        });
        let pipe = raw_pipe_from_properties(&props, &mapping).expect("pipe");
        assert!((pipe.length_km - 12.5).abs() < 1e-9);
        assert!((pipe.diameter_mm - 800.0).abs() < 1e-9);
        assert!((pipe.roughness_mm - 0.05).abs() < 1e-9);
    }

    #[test]
    fn pipe_regulator_type_and_setpoint_from_mapping() {
        let mut mapping = minimal_geojson_mapping();
        mapping.pipes.type_field = Some("TYPE_ORGANE".into());
        mapping.pipes.type_mapping = [("REG".into(), "pressure_regulator".into())]
            .into_iter()
            .collect();
        mapping.pipes.regulator_setpoint_field = Some("P_CONSIGNE_BAR".into());
        mapping.pipes.regulator_delta_p_min_field = Some("DP_MIN_BAR".into());

        let props = json!({
            "ID_CANA": "REG1",
            "NOEUD_AMONT": "MP",
            "NOEUD_AVAL": "SK",
            "LONGUEUR_M": 10.0,
            "DIAMETRE_MM": 400.0,
            "TYPE_ORGANE": "REG",
            "P_CONSIGNE_BAR": 20.0,
            "DP_MIN_BAR": 0.4
        });
        let pipe = raw_pipe_from_properties(&props, &mapping).expect("pipe");
        assert_eq!(pipe.kind, ConnectionKind::PressureRegulator);
        assert_eq!(pipe.equipment.regulator_setpoint_bar, Some(20.0));
        assert_eq!(pipe.equipment.regulator_delta_p_min_bar, Some(0.4));
    }

    #[test]
    fn load_mapping_from_str_parses_defaults() {
        let yaml = r#"
format: csv
nodes:
  id_field: id
defaults:
  sink_demand_m3s: -30.0
"#;
        let cfg = load_mapping_from_str(yaml).expect("yaml");
        assert_eq!(cfg.defaults.sink_demand_m3s, Some(-30.0));
    }
}
