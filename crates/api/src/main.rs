//! Funnel REST API Server
//!
//! Provides custom endpoints for video stats, search, and feeds.

use std::env;

use funnel_api::{AppState, AuthConfig, create_router};
use funnel_clickhouse::{ClickHouseClient, ClickHouseConfig};
use funnel_observability::init_tracing_dev;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_dev();

    let ch_config = ClickHouseConfig::from_env()?;
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    // Load auth config from environment (optional)
    let auth_config = AuthConfig::from_env();
    if auth_config.is_some() {
        tracing::info!("API authentication enabled");
    } else {
        tracing::warn!("API authentication disabled - set API_TOKEN to enable");
    }

    tracing::info!(
        clickhouse_url = %ch_config.safe_url(),
        database = %ch_config.database,
        bind_addr = %bind_addr,
        "Starting API server"
    );

    // Initialize metrics
    let metrics_handle = funnel_observability::init_metrics();

    // Connect to ClickHouse
    let clickhouse = ClickHouseClient::from_config(&ch_config)?;
    clickhouse.ping().await?;

    let version = clickhouse.version().await?;
    tracing::info!(version = %version, "Connected to ClickHouse");

    let state = AppState::new(clickhouse);
    let app = create_router(state, metrics_handle, auth_config);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Listening on {}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
