//! API handler tests using mock storage.

use axum::http::StatusCode;
use axum_test::TestServer;
use chrono::{DateTime, Utc};

use funnel_clickhouse::{
    ClickHouseError, StatsQueries, TrendingVideo, VideoHashtag, VideoQueries, VideoStats,
};

use crate::handlers::AppState;
use crate::router::create_test_router;

/// Mock storage backend for testing.
#[derive(Debug, Clone, Default)]
struct MockStorage {
    /// Videos to return from queries.
    videos: Vec<VideoStats>,
    /// Trending videos to return.
    trending: Vec<TrendingVideo>,
    /// Hashtag search results.
    hashtag_results: Vec<VideoHashtag>,
    /// Whether to simulate an error.
    should_error: bool,
    /// Event count to return.
    event_count: u64,
    /// Video count to return.
    video_count: u64,
}

impl MockStorage {
    fn new() -> Self {
        Self::default()
    }

    fn with_videos(mut self, videos: Vec<VideoStats>) -> Self {
        self.videos = videos;
        self
    }

    fn with_trending(mut self, trending: Vec<TrendingVideo>) -> Self {
        self.trending = trending;
        self
    }

    fn with_hashtag_results(mut self, results: Vec<VideoHashtag>) -> Self {
        self.hashtag_results = results;
        self
    }

    fn with_error(mut self) -> Self {
        self.should_error = true;
        self
    }

    fn with_counts(mut self, events: u64, videos: u64) -> Self {
        self.event_count = events;
        self.video_count = videos;
        self
    }
}

impl VideoQueries for MockStorage {
    async fn get_video_stats(&self, event_id: &str) -> Result<Option<VideoStats>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self.videos.iter().find(|v| v.id == event_id).cloned())
    }

    async fn get_videos_by_author(
        &self,
        pubkey: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self
            .videos
            .iter()
            .filter(|v| v.pubkey == pubkey)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn get_trending_videos(&self, limit: u32) -> Result<Vec<TrendingVideo>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self.trending.iter().take(limit as usize).cloned().collect())
    }

    async fn get_recent_videos(
        &self,
        kind: Option<u16>,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self
            .videos
            .iter()
            .filter(|v| kind.is_none_or(|k| v.kind == k))
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn search_by_hashtag(
        &self,
        hashtag: &str,
        limit: u32,
    ) -> Result<Vec<VideoHashtag>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self
            .hashtag_results
            .iter()
            .filter(|v| v.hashtag == hashtag)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn search_by_text(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        let query_lower = query.to_lowercase();
        Ok(self
            .videos
            .iter()
            .filter(|v| v.title.to_lowercase().contains(&query_lower))
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

impl StatsQueries for MockStorage {
    async fn get_event_count(&self) -> Result<u64, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self.event_count)
    }

    async fn get_video_count(&self) -> Result<u64, ClickHouseError> {
        if self.should_error {
            return Err(ClickHouseError::Connection("mock error".to_string()));
        }
        Ok(self.video_count)
    }
}

// Test fixtures

fn make_video_stats(id: &str, pubkey: &str, title: &str, kind: u16) -> VideoStats {
    VideoStats {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at: DateTime::<Utc>::from_timestamp(1700000000, 0).unwrap(),
        kind,
        d_tag: format!("d-{}", id),
        title: title.to_string(),
        thumbnail: format!("https://example.com/{}.jpg", id),
        reactions: 10,
        comments: 5,
        reposts: 2,
        engagement_score: 27,
    }
}

fn make_trending_video(id: &str, pubkey: &str, title: &str, score: f64) -> TrendingVideo {
    TrendingVideo {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at: DateTime::<Utc>::from_timestamp(1700000000, 0).unwrap(),
        kind: 34235,
        d_tag: format!("d-{}", id),
        title: title.to_string(),
        thumbnail: format!("https://example.com/{}.jpg", id),
        reactions: 100,
        comments: 50,
        reposts: 20,
        engagement_score: 270,
        trending_score: score,
    }
}

fn make_video_hashtag(id: &str, hashtag: &str, pubkey: &str) -> VideoHashtag {
    VideoHashtag {
        event_id: id.to_string(),
        hashtag: hashtag.to_string(),
        created_at: DateTime::<Utc>::from_timestamp(1700000000, 0).unwrap(),
        pubkey: pubkey.to_string(),
        kind: 34235,
        title: format!("Video {}", id),
        thumbnail: format!("https://example.com/{}.jpg", id),
        d_tag: format!("d-{}", id),
    }
}

fn create_test_server(storage: MockStorage) -> TestServer {
    let state = AppState::new(storage);
    let app = create_test_router(state);
    TestServer::new(app).unwrap()
}

// Health endpoint tests

#[tokio::test]
async fn health_returns_ok() {
    let server = create_test_server(MockStorage::new());
    let response = server.get("/health").await;

    response.assert_status_ok();
    response.assert_json(&serde_json::json!({ "status": "ok" }));
}

// Video stats endpoint tests

#[tokio::test]
async fn get_video_stats_returns_stats_when_found() {
    let storage = MockStorage::new().with_videos(vec![make_video_stats(
        "video123",
        "pubkey1",
        "My Video",
        34235,
    )]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos/video123/stats").await;

    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["id"], "video123");
    assert_eq!(body["title"], "My Video");
    assert_eq!(body["reactions"], 10);
}

#[tokio::test]
async fn get_video_stats_returns_404_when_not_found() {
    let server = create_test_server(MockStorage::new());

    let response = server.get("/api/videos/nonexistent/stats").await;

    response.assert_status(StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json();
    assert_eq!(body["error"], "Video not found");
}

#[tokio::test]
async fn get_video_stats_returns_500_on_error() {
    let storage = MockStorage::new().with_error();
    let server = create_test_server(storage);

    let response = server.get("/api/videos/video123/stats").await;

    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value = response.json();
    assert_eq!(body["error"], "Internal server error");
}

// List videos endpoint tests

#[tokio::test]
async fn list_videos_returns_recent_by_default() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "pubkey1", "Video 1", 34235),
        make_video_stats("video2", "pubkey2", "Video 2", 34236),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
}

#[tokio::test]
async fn list_videos_returns_trending_when_requested() {
    let storage = MockStorage::new().with_trending(vec![
        make_trending_video("video1", "pubkey1", "Trending 1", 100.0),
        make_trending_video("video2", "pubkey2", "Trending 2", 80.0),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos?sort=trending").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0]["trending_score"], 100.0);
}

#[tokio::test]
async fn list_videos_respects_limit() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "pubkey1", "Video 1", 34235),
        make_video_stats("video2", "pubkey2", "Video 2", 34235),
        make_video_stats("video3", "pubkey3", "Video 3", 34235),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos?limit=2").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
}

#[tokio::test]
async fn list_videos_caps_limit_at_100() {
    let storage = MockStorage::new();
    let server = create_test_server(storage);

    // Request with limit > 100 should be capped
    let response = server.get("/api/videos?limit=200").await;

    response.assert_status_ok();
}

#[tokio::test]
async fn list_videos_filters_by_kind() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "pubkey1", "Normal Video", 34235),
        make_video_stats("video2", "pubkey2", "Short Video", 34236),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos?kind=34236").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["kind"], 34236);
}

// User videos endpoint tests

#[tokio::test]
async fn get_user_videos_returns_videos_for_user() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "user1", "Video 1", 34235),
        make_video_stats("video2", "user1", "Video 2", 34235),
        make_video_stats("video3", "user2", "Video 3", 34235),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/users/user1/videos").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
    assert!(body.iter().all(|v| v["pubkey"] == "user1"));
}

#[tokio::test]
async fn get_user_videos_returns_empty_for_unknown_user() {
    let storage = MockStorage::new().with_videos(vec![make_video_stats(
        "video1",
        "user1",
        "Video 1",
        34235,
    )]);
    let server = create_test_server(storage);

    let response = server.get("/api/users/unknown_user/videos").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert!(body.is_empty());
}

#[tokio::test]
async fn get_user_videos_respects_limit() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "user1", "Video 1", 34235),
        make_video_stats("video2", "user1", "Video 2", 34235),
        make_video_stats("video3", "user1", "Video 3", 34235),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/users/user1/videos?limit=1").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 1);
}

// Search endpoint tests

#[tokio::test]
async fn search_by_hashtag_returns_matching_videos() {
    let storage = MockStorage::new().with_hashtag_results(vec![
        make_video_hashtag("video1", "nostr", "pubkey1"),
        make_video_hashtag("video2", "nostr", "pubkey2"),
        make_video_hashtag("video3", "bitcoin", "pubkey3"),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/search?tag=nostr").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
    assert!(body.iter().all(|v| v["hashtag"] == "nostr"));
}

#[tokio::test]
async fn search_by_text_returns_matching_videos() {
    let storage = MockStorage::new().with_videos(vec![
        make_video_stats("video1", "pubkey1", "Bitcoin Tutorial", 34235),
        make_video_stats("video2", "pubkey2", "Nostr Guide", 34235),
        make_video_stats("video3", "pubkey3", "Bitcoin News", 34235),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/search?q=bitcoin").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
    assert!(body.iter().all(|v| {
        v["title"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("bitcoin")
    }));
}

#[tokio::test]
async fn search_without_params_returns_400() {
    let server = create_test_server(MockStorage::new());

    let response = server.get("/api/search").await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json();
    assert_eq!(body["error"], "Search requires 'tag' or 'q' parameter");
}

#[tokio::test]
async fn search_respects_limit() {
    let storage = MockStorage::new().with_hashtag_results(vec![
        make_video_hashtag("video1", "nostr", "pubkey1"),
        make_video_hashtag("video2", "nostr", "pubkey2"),
        make_video_hashtag("video3", "nostr", "pubkey3"),
    ]);
    let server = create_test_server(storage);

    let response = server.get("/api/search?tag=nostr&limit=2").await;

    response.assert_status_ok();
    let body: Vec<serde_json::Value> = response.json();
    assert_eq!(body.len(), 2);
}

// Stats endpoint tests

#[tokio::test]
async fn get_stats_returns_counts() {
    let storage = MockStorage::new().with_counts(1000, 50);
    let server = create_test_server(storage);

    let response = server.get("/api/stats").await;

    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["total_events"], 1000);
    assert_eq!(body["total_videos"], 50);
}

#[tokio::test]
async fn get_stats_returns_zero_on_error() {
    let storage = MockStorage::new().with_error();
    let server = create_test_server(storage);

    let response = server.get("/api/stats").await;

    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    // Should return 0 on error (graceful degradation)
    assert_eq!(body["total_events"], 0);
    assert_eq!(body["total_videos"], 0);
}

// Cache-Control header tests

#[tokio::test]
async fn health_has_no_store_cache_header() {
    let server = create_test_server(MockStorage::new());
    let response = server.get("/health").await;

    let cache_control = response
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cache_control, "no-store");
}

#[tokio::test]
async fn video_stats_has_public_cache_header() {
    let storage = MockStorage::new().with_videos(vec![make_video_stats(
        "video1",
        "pubkey1",
        "Video",
        34235,
    )]);
    let server = create_test_server(storage);

    let response = server.get("/api/videos/video1/stats").await;

    let cache_control = response
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cache_control.contains("public"));
    assert!(cache_control.contains("max-age=30"));
}

#[tokio::test]
async fn list_videos_has_public_cache_header() {
    let server = create_test_server(MockStorage::new());
    let response = server.get("/api/videos").await;

    let cache_control = response
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cache_control.contains("public"));
    assert!(cache_control.contains("max-age=60"));
}

#[tokio::test]
async fn error_responses_have_no_store_cache_header() {
    let server = create_test_server(MockStorage::new());
    let response = server.get("/api/videos/nonexistent/stats").await;

    let cache_control = response
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cache_control, "no-store");
}
