//! Router configuration for the API.

use axum::{Router, http::header, routing::get};
use funnel_clickhouse::{StatsQueries, VideoQueries};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::handlers::{
    AppState, get_stats, get_user_videos, get_video_stats, health, list_videos, search_videos,
};

/// Create the API router with the given storage backend and metrics handle.
pub fn create_router<S>(state: AppState<S>, metrics_handle: PrometheusHandle) -> Router
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health))
        .route(
            "/metrics",
            get(move || async move {
                (
                    [(header::CACHE_CONTROL, "no-store")],
                    metrics_handle.render(),
                )
            }),
        )
        .route("/api/videos/{id}/stats", get(get_video_stats::<S>))
        .route("/api/videos", get(list_videos::<S>))
        .route("/api/users/{pubkey}/videos", get(get_user_videos::<S>))
        .route("/api/search", get(search_videos::<S>))
        .route("/api/stats", get(get_stats::<S>))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Create a router for testing without metrics endpoint.
#[cfg(test)]
pub fn create_test_router<S>(state: AppState<S>) -> Router
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health))
        .route("/api/videos/{id}/stats", get(get_video_stats::<S>))
        .route("/api/videos", get(list_videos::<S>))
        .route("/api/users/{pubkey}/videos", get(get_user_videos::<S>))
        .route("/api/search", get(search_videos::<S>))
        .route("/api/stats", get(get_stats::<S>))
        .with_state(state)
}
