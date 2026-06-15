//! Import de réseaux depuis GeoJSON, CSV ou GasLib via un modèle intermédiaire.

mod csv;
mod demand_profiles;
mod geojson;
pub mod mapping;
mod shapefile;
pub mod validation;
mod weather;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
pub use demand_profiles::{load_demand_profiles_csv, parse_demand_profiles_csv};
pub use mapping::{MappingConfig, load_mapping, load_mapping_from_str};
pub use validation::{ValidationError, validate_topology};
pub use weather::{load_weather_csv, parse_weather_csv};

use crate::graph::RawNetwork;

pub use csv::{import_csv, import_csv_str};
pub use geojson::{import_geojson, import_geojson_str};
pub use shapefile::{import_shapefile_pair, import_shapefile_pair_bytes};

#[derive(Debug, Clone, Default)]
pub struct ImportRequest {
    pub mapping_path: PathBuf,
    pub nodes_path: Option<PathBuf>,
    pub pipes_path: Option<PathBuf>,
    pub geojson_paths: Vec<PathBuf>,
    pub nodes_shp_path: Option<PathBuf>,
    pub nodes_dbf_path: Option<PathBuf>,
    pub pipes_shp_path: Option<PathBuf>,
    pub pipes_dbf_path: Option<PathBuf>,
    pub gaslib_net_path: Option<PathBuf>,
}

pub trait NetworkImporter {
    fn import(&self, request: &ImportRequest) -> Result<RawNetwork>;
}

pub struct GeoJsonImporter;
pub struct CsvImporter;
pub struct ShapefileImporter;
pub struct GasLibImporter;

impl NetworkImporter for GeoJsonImporter {
    fn import(&self, request: &ImportRequest) -> Result<RawNetwork> {
        if request.geojson_paths.is_empty() {
            bail!("GeoJsonImporter: geojson_paths vide");
        }
        let mapping = mapping::load_mapping(&request.mapping_path)?;
        let paths: Vec<&Path> = request.geojson_paths.iter().map(|p| p.as_path()).collect();
        import_geojson(&paths, &mapping)
    }
}

impl NetworkImporter for CsvImporter {
    fn import(&self, request: &ImportRequest) -> Result<RawNetwork> {
        let nodes = request
            .nodes_path
            .as_ref()
            .context("CsvImporter: nodes_path requis")?;
        let pipes = request
            .pipes_path
            .as_ref()
            .context("CsvImporter: pipes_path requis")?;
        let mapping = mapping::load_mapping(&request.mapping_path)?;
        import_csv(nodes, pipes, &mapping)
    }
}

impl NetworkImporter for ShapefileImporter {
    fn import(&self, request: &ImportRequest) -> Result<RawNetwork> {
        let mapping = mapping::load_mapping(&request.mapping_path)?;
        let nodes_shp = request
            .nodes_shp_path
            .as_ref()
            .context("ShapefileImporter: nodes_shp_path requis")?;
        let nodes_dbf = request
            .nodes_dbf_path
            .as_ref()
            .context("ShapefileImporter: nodes_dbf_path requis")?;
        let pipes_shp = request
            .pipes_shp_path
            .as_ref()
            .context("ShapefileImporter: pipes_shp_path requis")?;
        let pipes_dbf = request
            .pipes_dbf_path
            .as_ref()
            .context("ShapefileImporter: pipes_dbf_path requis")?;
        import_shapefile_pair(nodes_shp, nodes_dbf, pipes_shp, pipes_dbf, &mapping)
    }
}

impl NetworkImporter for GasLibImporter {
    fn import(&self, request: &ImportRequest) -> Result<RawNetwork> {
        let path = request
            .gaslib_net_path
            .as_ref()
            .context("GasLibImporter: gaslib_net_path requis")?;
        crate::gaslib::load_network_raw(path)
    }
}

pub fn import_with(format: &str, request: &ImportRequest) -> Result<RawNetwork> {
    match format.to_ascii_lowercase().as_str() {
        "geojson" => GeoJsonImporter.import(request),
        "csv" => CsvImporter.import(request),
        "shapefile" => ShapefileImporter.import(request),
        "gaslib" => GasLibImporter.import(request),
        other => bail!("format d'import inconnu: {other}"),
    }
}

/// Chemin racine du corpus de tests (P6+).
pub fn test_corpus_root() -> PathBuf {
    std::env::var("GAZFLOW_TEST_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/testing/corpus")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GasNetwork, RawNodeRole};
    use crate::solver::solve_steady_state;
    use std::collections::HashMap;

    fn corpus() -> PathBuf {
        test_corpus_root()
    }

    #[test]
    fn test_geojson_import_minimal() {
        let root = corpus();
        let mapping = root.join("synthetic/minimal-line/mapping.yaml");
        let request = ImportRequest {
            mapping_path: mapping,
            nodes_path: None,
            pipes_path: None,
            geojson_paths: vec![
                root.join("synthetic/minimal-line/nodes.geojson"),
                root.join("synthetic/minimal-line/pipes.geojson"),
            ],
            ..Default::default()
        };
        let raw = GeoJsonImporter.import(&request).expect("import geojson");
        assert_eq!(raw.nodes.len(), 3);
        assert_eq!(raw.pipes.len(), 2);
        validate_topology(&raw).expect("topologie valide");
    }

    #[test]
    fn test_csv_import_with_mapping() {
        let root = corpus();
        let request = ImportRequest {
            mapping_path: root.join("synthetic/gravity-pipe/mapping.yaml"),
            nodes_path: Some(root.join("synthetic/gravity-pipe/nodes.csv")),
            pipes_path: Some(root.join("synthetic/gravity-pipe/pipes.csv")),
            geojson_paths: vec![],
            ..Default::default()
        };
        let raw = CsvImporter.import(&request).expect("import csv");
        assert_eq!(raw.nodes.len(), 2);
        assert_eq!(raw.pipes.len(), 1);
        assert!((raw.nodes[1].height_m - raw.nodes[0].height_m - 150.0).abs() < 1e-6);
    }

    #[test]
    fn test_csv_gravity_downhill_altitudes() {
        let root = corpus();
        let request = ImportRequest {
            mapping_path: root.join("synthetic/gravity-pipe/mapping.yaml"),
            nodes_path: Some(root.join("synthetic/gravity-pipe/nodes-downhill.csv")),
            pipes_path: Some(root.join("synthetic/gravity-pipe/pipes.csv")),
            geojson_paths: vec![],
            ..Default::default()
        };
        let raw = CsvImporter.import(&request).expect("import csv downhill");
        let up = raw.nodes.iter().find(|n| n.id == "UP").expect("UP");
        let down = raw.nodes.iter().find(|n| n.id == "DOWN").expect("DOWN");
        assert!(
            up.height_m > down.height_m,
            "écoulement aval = altitude décroissante"
        );
        assert!((up.height_m - down.height_m - 150.0).abs() < 1e-6);
    }

    #[test]
    fn test_geojson_minimal_line_operateur_fields() {
        let root = corpus();
        let mapping =
            mapping::load_mapping(&root.join("synthetic/minimal-line/mapping.yaml")).unwrap();
        let raw = import_geojson(
            &[
                root.join("synthetic/minimal-line/nodes.geojson").as_path(),
                root.join("synthetic/minimal-line/pipes.geojson").as_path(),
            ],
            &mapping,
        )
        .expect("import");
        let src = raw.nodes.iter().find(|n| n.id == "SRC01").expect("SRC01");
        let sink = raw.nodes.iter().find(|n| n.id == "LVR01").expect("LVR01");
        assert_eq!(src.role, RawNodeRole::Source);
        assert_eq!(sink.role, RawNodeRole::Sink);
        assert_eq!(src.pressure_fixed_bar, Some(70.0));
        assert!((src.lon.unwrap() - 2.3522).abs() < 1e-4);
        assert!((src.lat.unwrap() - 48.8566).abs() < 1e-4);
        assert!((src.height_m - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_validation_detects_orphan_node() {
        let root = corpus();
        let mapping =
            mapping::load_mapping(&root.join("synthetic/minimal-line/mapping.yaml")).unwrap();
        let raw = import_geojson(
            &[root
                .join("synthetic/topo-errors/orphan-node.geojson")
                .as_path()],
            &mapping,
        )
        .unwrap();
        assert!(matches!(
            validate_topology(&raw),
            Err(ValidationError::OrphanNode { .. })
        ));
    }

    #[test]
    fn test_validation_detects_no_slack() {
        let root = corpus();
        let mapping =
            mapping::load_mapping(&root.join("synthetic/minimal-line/mapping.yaml")).unwrap();
        let raw = import_geojson(
            &[root
                .join("synthetic/topo-errors/no-slack.geojson")
                .as_path()],
            &mapping,
        )
        .unwrap();
        assert_eq!(validate_topology(&raw), Err(ValidationError::NoSlack));
    }

    #[test]
    fn test_validation_detects_disconnected_graph() {
        let root = corpus();
        let mapping =
            mapping::load_mapping(&root.join("synthetic/minimal-line/mapping.yaml")).unwrap();
        let raw = import_geojson(
            &[root
                .join("synthetic/topo-errors/disconnected.geojson")
                .as_path()],
            &mapping,
        )
        .unwrap();
        assert!(matches!(
            validate_topology(&raw),
            Err(ValidationError::DisconnectedGraph { components: 2 })
        ));
    }

    #[test]
    fn test_gaslib_as_network_importer() {
        let net_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dat/GasLib-11.net");
        if !net_path.exists() {
            eprintln!("skip test_gaslib_as_network_importer: GasLib-11 absent");
            return;
        }
        let via_raw = GasLibImporter
            .import(&ImportRequest {
                mapping_path: PathBuf::new(),
                nodes_path: None,
                pipes_path: None,
                geojson_paths: vec![],
                gaslib_net_path: Some(net_path.clone()),
                ..Default::default()
            })
            .expect("raw gaslib");
        let direct = crate::gaslib::load_network(&net_path).expect("load_network");
        let from_raw = GasNetwork::from_raw(via_raw).expect("from_raw");
        assert_eq!(from_raw.node_count(), direct.node_count());
        assert_eq!(from_raw.edge_count(), direct.edge_count());
    }

    #[test]
    fn test_import_then_solve() {
        let root = corpus();
        let request = ImportRequest {
            mapping_path: root.join("synthetic/minimal-line/mapping.yaml"),
            nodes_path: None,
            pipes_path: None,
            geojson_paths: vec![
                root.join("synthetic/minimal-line/nodes.geojson"),
                root.join("synthetic/minimal-line/pipes.geojson"),
            ],
            ..Default::default()
        };
        let raw = GeoJsonImporter.import(&request).expect("import");
        validate_topology(&raw).expect("valid");
        let network = GasNetwork::from_raw(raw).expect("graph");
        let mut demands = HashMap::new();
        demands.insert("LVR01".to_string(), -50.0);
        let result = solve_steady_state(&network, &demands, 200, 1e-6);
        assert!(result.is_ok(), "solve failed: {:?}", result.err());
    }

    #[test]
    fn test_import_gravity_corpus_then_solve() {
        let root = corpus();
        let request = ImportRequest {
            mapping_path: root.join("synthetic/gravity-pipe/mapping.yaml"),
            nodes_path: Some(root.join("synthetic/gravity-pipe/nodes.csv")),
            pipes_path: Some(root.join("synthetic/gravity-pipe/pipes.csv")),
            geojson_paths: vec![],
            ..Default::default()
        };
        let raw = CsvImporter.import(&request).expect("import");
        validate_topology(&raw).expect("valid");
        let network = GasNetwork::from_raw(raw).expect("graph");
        let up = network.nodes().find(|n| n.id == "UP").expect("UP");
        let down = network.nodes().find(|n| n.id == "DOWN").expect("DOWN");
        assert!(down.height_m > up.height_m);

        let mut demands = HashMap::new();
        demands.insert("DOWN".to_string(), -30.0);
        let result = solve_steady_state(&network, &demands, 500, 1e-6).expect("solve");
        assert!(result.pressures["DOWN"] < up.pressure_fixed_bar.unwrap());
    }
}
