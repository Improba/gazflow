use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};

use super::mapping::{MappingConfig, raw_node_from_csv_row, raw_pipe_from_csv_row};
use crate::graph::{RawNetwork, RawPipe};

pub fn import_csv(
    nodes_path: &Path,
    pipes_path: &Path,
    mapping: &MappingConfig,
) -> Result<RawNetwork> {
    let nodes_raw = std::fs::read_to_string(nodes_path)
        .with_context(|| format!("lecture CSV nœuds {:?}", nodes_path))?;
    let pipes_raw = std::fs::read_to_string(pipes_path)
        .with_context(|| format!("lecture CSV pipes {:?}", pipes_path))?;
    import_csv_str(&nodes_raw, &pipes_raw, mapping)
}

pub fn import_csv_str(
    nodes_csv: &str,
    pipes_csv: &str,
    mapping: &MappingConfig,
) -> Result<RawNetwork> {
    let nodes = parse_nodes_csv(nodes_csv, mapping)?;
    let pipes = parse_pipes_csv(pipes_csv, mapping)?;
    Ok(RawNetwork {
        nodes,
        pipes,
        source: Some("csv:inline".to_string()),
        compressor_catalog: None,
    })
}

fn parse_nodes_csv(raw: &str, mapping: &MappingConfig) -> Result<Vec<crate::graph::RawNode>> {
    let mut rdr = csv::Reader::from_reader(Cursor::new(raw));
    let headers: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
    rdr.records()
        .map(|rec| {
            let rec = rec?;
            let row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
            raw_node_from_csv_row(&header_refs, &row, mapping)
        })
        .collect()
}

fn parse_pipes_csv(raw: &str, mapping: &MappingConfig) -> Result<Vec<RawPipe>> {
    let mut rdr = csv::Reader::from_reader(Cursor::new(raw));
    let headers: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
    rdr.records()
        .map(|rec| {
            let rec = rec?;
            let row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
            raw_pipe_from_csv_row(&header_refs, &row, mapping)
        })
        .collect()
}
