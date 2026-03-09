mod gaslib;
mod graph;
mod solver;
mod api;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("OpenGasSim backend starting…");

    let network = gaslib::load_network("dat/GasLib-11.net")?;
    tracing::info!(
        "Réseau chargé : {} nœuds, {} arêtes",
        network.node_count(),
        network.edge_count()
    );

    let app = api::create_router(network);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    tracing::info!("API disponible sur http://localhost:3001");
    axum::serve(listener, app).await?;

    Ok(())
}
