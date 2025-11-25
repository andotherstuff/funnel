//! Funnel REST API Server
//!
//! Provides custom endpoints for video stats, search, and feeds.

use std::env;
use std::time::Instant;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use funnel_clickhouse::ClickHouseClient;
use funnel_observability::{api, init_tracing_dev};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    clickhouse: ClickHouseClient,
}

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

    let state = AppState { clickhouse };

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(move || async move { metrics_handle.render() }))
        .route("/api/videos/{id}/stats", get(get_video_stats))
        .route("/api/videos", get(list_videos))
        .route("/api/users/{pubkey}/videos", get(get_user_videos))
        .route("/api/search", get(search_videos))
        .route("/api/stats", get(get_stats))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Listening on {}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Debug, Deserialize)]
struct VideoStatsPath {
    id: String,
}

async fn get_video_stats(
    State(state): State<AppState>,
    Path(params): Path<VideoStatsPath>,
) -> impl IntoResponse {
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "video_stats").increment(1);

    match state.clickhouse.get_video_stats(&params.id).await {
        Ok(Some(stats)) => {
            histogram!(api::QUERY_DURATION, "endpoint" => "video_stats")
                .record(start.elapsed().as_secs_f64());
            (StatusCode::OK, Json(serde_json::to_value(stats).unwrap())).into_response()
        }
        Ok(None) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Video not found" })))
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to get video stats");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct ListVideosQuery {
    sort: Option<String>,
    kind: Option<u16>,
    limit: Option<u32>,
}

async fn list_videos(
    State(state): State<AppState>,
    Query(params): Query<ListVideosQuery>,
) -> impl IntoResponse {
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "list_videos").increment(1);

    let limit = params.limit.unwrap_or(50).min(100);
    let sort = params.sort.as_deref().unwrap_or("recent");

    let result = match sort {
        "popular" | "trending" => state.clickhouse.get_trending_videos(limit).await,
        _ => state
            .clickhouse
            .get_recent_videos(params.kind, limit)
            .await
            .map(|v| {
                v.into_iter()
                    .map(|s| funnel_clickhouse::queries::TrendingVideo {
                        id: s.id,
                        pubkey: s.pubkey,
                        created_at: s.created_at,
                        kind: s.kind,
                        d_tag: s.d_tag,
                        title: s.title,
                        thumbnail: s.thumbnail,
                        reactions: s.reactions,
                        comments: s.comments,
                        reposts: s.reposts,
                        engagement_score: s.engagement_score,
                        trending_score: 0.0,
                    })
                    .collect()
            }),
    };

    histogram!(api::QUERY_DURATION, "endpoint" => "list_videos")
        .record(start.elapsed().as_secs_f64());

    match result {
        Ok(videos) => Json(serde_json::to_value(videos).unwrap()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to list videos");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct UserVideosPath {
    pubkey: String,
}

#[derive(Debug, Deserialize)]
struct UserVideosQuery {
    limit: Option<u32>,
}

async fn get_user_videos(
    State(state): State<AppState>,
    Path(params): Path<UserVideosPath>,
    Query(query): Query<UserVideosQuery>,
) -> impl IntoResponse {
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "user_videos").increment(1);

    let limit = query.limit.unwrap_or(50).min(100);

    match state
        .clickhouse
        .get_videos_by_author(&params.pubkey, limit)
        .await
    {
        Ok(videos) => {
            histogram!(api::QUERY_DURATION, "endpoint" => "user_videos")
                .record(start.elapsed().as_secs_f64());
            Json(serde_json::to_value(videos).unwrap()).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to get user videos");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    tag: Option<String>,
    q: Option<String>,
    limit: Option<u32>,
}

async fn search_videos(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "search").increment(1);

    let limit = params.limit.unwrap_or(50).min(100);

    // Search by hashtag if provided
    if let Some(tag) = params.tag {
        match state.clickhouse.search_by_hashtag(&tag, limit).await {
            Ok(videos) => {
                histogram!(api::QUERY_DURATION, "endpoint" => "search")
                    .record(start.elapsed().as_secs_f64());
                return Json(serde_json::to_value(videos).unwrap()).into_response();
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to search by hashtag");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response();
            }
        }
    }

    // Full-text search by query string
    if let Some(q) = params.q {
        match state.clickhouse.search_by_text(&q, limit).await {
            Ok(videos) => {
                histogram!(api::QUERY_DURATION, "endpoint" => "search")
                    .record(start.elapsed().as_secs_f64());
                return Json(serde_json::to_value(videos).unwrap()).into_response();
            }
            Err(e) => {
                tracing::error!(error = %e, query = %q, "Failed to search by text");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response();
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "Search requires 'tag' or 'q' parameter" })),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
struct Stats {
    total_events: u64,
    total_videos: u64,
}

async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "stats").increment(1);

    let events = state.clickhouse.get_event_count().await.unwrap_or(0);
    let videos = state.clickhouse.get_video_count().await.unwrap_or(0);

    histogram!(api::QUERY_DURATION, "endpoint" => "stats")
        .record(start.elapsed().as_secs_f64());

    Json(Stats {
        total_events: events,
        total_videos: videos,
    })
}
