//! Observability setup for Funnel services.
//!
//! Provides tracing subscriber configuration and Prometheus metrics export.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize tracing with JSON output and env filter.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

/// Initialize tracing with human-readable output (for development).
pub fn init_tracing_dev() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Initialize Prometheus metrics exporter.
/// Returns a handle that can render metrics in Prometheus format.
pub fn init_metrics() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

/// Common metrics labels.
pub mod labels {
    pub const KIND: &str = "kind";
    pub const ENDPOINT: &str = "endpoint";
    pub const STATUS: &str = "status";
}

/// Metric names for the ingestion service.
pub mod ingestion {
    pub const EVENTS_RECEIVED: &str = "ingestion_events_received_total";
    pub const EVENTS_WRITTEN: &str = "ingestion_events_written_total";
    pub const BATCH_SIZE: &str = "ingestion_batch_size";
    pub const WRITE_LATENCY: &str = "ingestion_clickhouse_write_latency_seconds";
    pub const LAG: &str = "ingestion_lag_seconds";
}

/// Metric names for the API service.
pub mod api {
    pub const REQUESTS: &str = "api_requests_total";
    pub const REQUEST_DURATION: &str = "api_request_duration_seconds";
    pub const QUERY_DURATION: &str = "api_clickhouse_query_duration_seconds";
}
