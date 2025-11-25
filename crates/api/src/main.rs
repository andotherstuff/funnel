//! Funnel REST API Server
//!
//! Provides custom endpoints for video stats, search, and feeds.

use std::env;

use funnel_api::{AppState, create_router};
use funnel_clickhouse::ClickHouseClient;
use funnel_observability::init_tracing_dev;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_dev();

    let clickhouse_url =
        env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let database = env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "nostr".to_string());
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    tracing::info!(
        clickhouse_url = %clickhouse_url,
        database = %database,
        bind_addr = %bind_addr,
        "Starting API server"
    );

    // Initialize metrics
    let metrics_handle = funnel_observability::init_metrics();

    // Connect to ClickHouse
    let clickhouse = ClickHouseClient::new(&clickhouse_url, &database)?;
    clickhouse.ping().await?;

    let version = clickhouse.version().await?;
    tracing::info!(version = %version, "Connected to ClickHouse");

    let state = AppState::new(clickhouse);
    let app = create_router(state, metrics_handle);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Listening on {}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
