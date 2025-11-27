//! Router configuration for the API.

use axum::{Extension, Router, http::header, middleware, routing::get};
use funnel_clickhouse::{StatsQueries, VideoQueries};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{AuthConfig, require_auth};
use crate::handlers::{
    AppState, get_stats, get_user_videos, get_video_stats, health, list_videos, search_videos,
};

/// Create the API router with the given storage backend and metrics handle.
///
/// If `auth_config` is `Some`, bearer token authentication will be required for
/// all `/api/*` endpoints. The `/health` and `/metrics` endpoints remain public
/// for monitoring purposes.
pub fn create_router<S>(
    state: AppState<S>,
    metrics_handle: PrometheusHandle,
    auth_config: Option<AuthConfig>,
) -> Router
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    // Public routes (no auth required)
    let public_routes = Router::new().route("/health", get(health)).route(
        "/metrics",
        get(move || async move {
            (
                [(header::CACHE_CONTROL, "no-store")],
                metrics_handle.render(),
            )
        }),
    );

    // Protected API routes
    let api_routes = Router::new()
        .route("/api/videos/{id}/stats", get(get_video_stats::<S>))
        .route("/api/videos", get(list_videos::<S>))
        .route("/api/users/{pubkey}/videos", get(get_user_videos::<S>))
        .route("/api/search", get(search_videos::<S>))
        .route("/api/stats", get(get_stats::<S>));

    // Apply auth middleware only if auth is configured
    let api_routes = if let Some(config) = auth_config {
        api_routes
            .layer(middleware::from_fn(require_auth))
            .layer(Extension(config))
    } else {
        api_routes
    };

    public_routes
        .merge(api_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Create a router for testing without metrics endpoint.
///
/// If `auth_config` is `Some`, authentication will be required for API routes.
#[cfg(test)]
pub fn create_test_router<S>(state: AppState<S>, auth_config: Option<AuthConfig>) -> Router
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let public_routes = Router::new().route("/health", get(health));

    let api_routes = Router::new()
        .route("/api/videos/{id}/stats", get(get_video_stats::<S>))
        .route("/api/videos", get(list_videos::<S>))
        .route("/api/users/{pubkey}/videos", get(get_user_videos::<S>))
        .route("/api/search", get(search_videos::<S>))
        .route("/api/stats", get(get_stats::<S>));

    let api_routes = if let Some(config) = auth_config {
        api_routes
            .layer(middleware::from_fn(require_auth))
            .layer(Extension(config))
    } else {
        api_routes
    };

    public_routes.merge(api_routes).with_state(state)
}
