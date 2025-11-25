//! API request handlers.
//!
//! These handlers are generic over the storage backend, allowing for easy testing
//! with mock implementations.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use funnel_clickhouse::{StatsQueries, VideoQueries};
use funnel_observability::api;
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};

/// Application state containing the storage backend.
#[derive(Clone)]
pub struct AppState<S>
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    pub storage: Arc<S>,
}

impl<S> AppState<S>
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    pub fn new(storage: S) -> Self {
        Self {
            storage: Arc::new(storage),
        }
    }
}

/// Health check response.
pub async fn health() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "status": "ok" })),
    )
}

/// Video stats path parameters.
#[derive(Debug, Deserialize)]
pub struct VideoStatsPath {
    pub id: String,
}

/// Get stats for a specific video.
pub async fn get_video_stats<S>(
    State(state): State<AppState<S>>,
    Path(params): Path<VideoStatsPath>,
) -> impl IntoResponse
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "video_stats").increment(1);

    match state.storage.get_video_stats(&params.id).await {
        Ok(Some(stats)) => {
            histogram!(api::QUERY_DURATION, "endpoint" => "video_stats")
                .record(start.elapsed().as_secs_f64());
            (
                StatusCode::OK,
                [(header::CACHE_CONTROL, "public, max-age=30")],
                Json(serde_json::to_value(stats).unwrap()),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "error": "Video not found" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to get video stats");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CACHE_CONTROL, "no-store")],
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

/// List videos query parameters.
#[derive(Debug, Deserialize)]
pub struct ListVideosQuery {
    pub sort: Option<String>,
    pub kind: Option<u16>,
    pub limit: Option<u32>,
}

/// List videos with optional sorting.
pub async fn list_videos<S>(
    State(state): State<AppState<S>>,
    Query(params): Query<ListVideosQuery>,
) -> impl IntoResponse
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "list_videos").increment(1);

    let limit = params.limit.unwrap_or(50).min(100);
    let sort = params.sort.as_deref().unwrap_or("recent");

    let result = match sort {
        "popular" | "trending" => state.storage.get_trending_videos(limit).await,
        _ => state
            .storage
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
        Ok(videos) => (
            [(header::CACHE_CONTROL, "public, max-age=60")],
            Json(serde_json::to_value(videos).unwrap()),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to list videos");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CACHE_CONTROL, "no-store")],
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

/// User videos path parameters.
#[derive(Debug, Deserialize)]
pub struct UserVideosPath {
    pub pubkey: String,
}

/// User videos query parameters.
#[derive(Debug, Deserialize)]
pub struct UserVideosQuery {
    pub limit: Option<u32>,
}

/// Get videos by a specific user.
pub async fn get_user_videos<S>(
    State(state): State<AppState<S>>,
    Path(params): Path<UserVideosPath>,
    Query(query): Query<UserVideosQuery>,
) -> impl IntoResponse
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "user_videos").increment(1);

    let limit = query.limit.unwrap_or(50).min(100);

    match state.storage.get_videos_by_author(&params.pubkey, limit).await {
        Ok(videos) => {
            histogram!(api::QUERY_DURATION, "endpoint" => "user_videos")
                .record(start.elapsed().as_secs_f64());
            (
                [(header::CACHE_CONTROL, "public, max-age=60")],
                Json(serde_json::to_value(videos).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to get user videos");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CACHE_CONTROL, "no-store")],
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

/// Search query parameters.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub tag: Option<String>,
    pub q: Option<String>,
    pub limit: Option<u32>,
}

/// Search videos by hashtag or text.
pub async fn search_videos<S>(
    State(state): State<AppState<S>>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "search").increment(1);

    let limit = params.limit.unwrap_or(50).min(100);

    // Search by hashtag if provided
    if let Some(tag) = params.tag {
        match state.storage.search_by_hashtag(&tag, limit).await {
            Ok(videos) => {
                histogram!(api::QUERY_DURATION, "endpoint" => "search")
                    .record(start.elapsed().as_secs_f64());
                return (
                    [(header::CACHE_CONTROL, "public, max-age=60")],
                    Json(serde_json::to_value(videos).unwrap()),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to search by hashtag");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response();
            }
        }
    }

    // Full-text search by query string
    if let Some(q) = params.q {
        match state.storage.search_by_text(&q, limit).await {
            Ok(videos) => {
                histogram!(api::QUERY_DURATION, "endpoint" => "search")
                    .record(start.elapsed().as_secs_f64());
                return (
                    [(header::CACHE_CONTROL, "public, max-age=60")],
                    Json(serde_json::to_value(videos).unwrap()),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!(error = %e, query = %q, "Failed to search by text");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response();
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "error": "Search requires 'tag' or 'q' parameter" })),
    )
        .into_response()
}

/// Stats response.
#[derive(Debug, Serialize)]
pub struct Stats {
    pub total_events: u64,
    pub total_videos: u64,
}

/// Get overall stats.
pub async fn get_stats<S>(State(state): State<AppState<S>>) -> impl IntoResponse
where
    S: VideoQueries + StatsQueries + Clone + Send + Sync + 'static,
{
    let start = Instant::now();
    counter!(api::REQUESTS, "endpoint" => "stats").increment(1);

    let events = state.storage.get_event_count().await.unwrap_or(0);
    let videos = state.storage.get_video_count().await.unwrap_or(0);

    histogram!(api::QUERY_DURATION, "endpoint" => "stats").record(start.elapsed().as_secs_f64());

    (
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(Stats {
            total_events: events,
            total_videos: videos,
        }),
    )
}
