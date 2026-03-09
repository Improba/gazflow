mod api;
mod gaslib;
mod graph;
mod solver;

use std::collections::HashMap;
use std::path::Path;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("GazSim backend starting…");

    let network = gaslib::load_network("dat/GasLib-11.net")?;
    tracing::info!(
        "Réseau chargé : {} nœuds, {} arêtes",
        network.node_count(),
        network.edge_count()
    );

    let scenario_path = Path::new("dat/GasLib-11.scn");
    let default_demands: HashMap<String, f64> = if scenario_path.exists() {
        match gaslib::load_scenario_demands(scenario_path) {
            Ok(parsed) => {
                let scenario: gaslib::ScenarioDemands = parsed;
                tracing::info!(
                    "Scénario chargé : id={:?}, {} demandes",
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

    let app = api::create_router(network, default_demands);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    tracing::info!("API disponible sur http://localhost:3001");
    axum::serve(listener, app).await?;

    Ok(())
}
