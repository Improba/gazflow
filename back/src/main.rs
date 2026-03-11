use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};
use gazflow_back::{api, gaslib};
use tracing_subscriber::EnvFilter;

const DEFAULT_DATASET: &str = "GasLib-11";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("GazFlow backend starting…");

    let data_dir = Path::new("dat");
    let available_datasets = discover_available_datasets(data_dir)?;
    if available_datasets.is_empty() {
        return Err(anyhow!(
            "Aucun dataset détecté dans {:?} (attendu: GasLib-<id>.net)",
            data_dir
        ));
    }

    let requested_dataset = std::env::var("GAZFLOW_DATASET")
        .ok()
        .unwrap_or_else(|| DEFAULT_DATASET.to_string());
    let active_dataset = if available_datasets.iter().any(|d| d == &requested_dataset) {
        requested_dataset
    } else {
        let fallback = available_datasets
            .first()
            .cloned()
            .expect("at least one dataset");
        tracing::warn!(
            "Dataset demandé {:?} indisponible, fallback vers {:?}",
            requested_dataset,
            fallback
        );
        fallback
    };

    let network_path = data_dir.join(format!("{active_dataset}.net"));
    let network = gaslib::load_network(&network_path)
        .with_context(|| format!("chargement réseau {:?}", network_path))?;
    tracing::info!(
        "Réseau actif {} chargé : {} nœuds, {} arêtes",
        active_dataset,
        network.node_count(),
        network.edge_count()
    );

    let scenario_path = data_dir.join(format!("{active_dataset}.scn"));
    let default_demands: HashMap<String, f64> = if scenario_path.exists() {
        match gaslib::load_scenario_demands(&scenario_path) {
            Ok(parsed) => {
                let scenario: gaslib::ScenarioDemands = parsed;
                tracing::info!(
                    "Scénario chargé pour {} : id={:?}, {} demandes",
                    active_dataset,
                    scenario.scenario_id,
                    scenario.demands.len()
                );
                scenario.demands
            }
            Err(err) => {
                tracing::warn!("Impossible de charger {:?}: {err:#}", scenario_path);
                HashMap::new()
            }
        }
    } else {
        tracing::warn!(
            "Fichier scénario absent: {:?} (utilisation des demandes par défaut)",
            scenario_path
        );
        HashMap::new()
    };

    let app = api::create_router_with_datasets(
        network,
        default_demands,
        active_dataset,
        available_datasets,
        data_dir.to_path_buf(),
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    tracing::info!("API disponible sur http://localhost:3001");
    axum::serve(listener, app).await?;

    Ok(())
}

fn discover_available_datasets(data_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut datasets: Vec<String> = fs::read_dir(data_dir)
        .with_context(|| format!("lecture du dossier datasets {:?}", data_dir))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            if !name.starts_with("GasLib-") || !name.ends_with(".net") || name.contains("-v") {
                return None;
            }
            let dataset_id = name.trim_end_matches(".net").to_string();
            let suffix = dataset_id.strip_prefix("GasLib-")?;
            if suffix.chars().all(|c| c.is_ascii_digit()) {
                Some(dataset_id)
            } else {
                None
            }
        })
        .collect();

    datasets.sort_by_key(|dataset_id| {
        dataset_id
            .trim_start_matches("GasLib-")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    datasets.dedup();
    Ok(datasets)
}
