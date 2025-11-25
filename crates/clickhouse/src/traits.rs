//! Traits for abstracting ClickHouse operations.
//!
//! These traits allow for mocking in tests without requiring a real ClickHouse instance.

use std::future::Future;

use crate::error::ClickHouseError;
use crate::queries::{EventRow, TrendingVideo, VideoHashtag, VideoStats};

/// Trait for read-only video queries.
///
/// This trait can be mocked for testing API handlers.
#[allow(dead_code)]
pub trait VideoQueries: Send + Sync {
    /// Get video stats by event ID.
    fn get_video_stats(
        &self,
        event_id: &str,
    ) -> impl Future<Output = Result<Option<VideoStats>, ClickHouseError>> + Send;

    /// Get videos by author pubkey.
    fn get_videos_by_author(
        &self,
        pubkey: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<VideoStats>, ClickHouseError>> + Send;

    /// Get trending videos.
    fn get_trending_videos(
        &self,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<TrendingVideo>, ClickHouseError>> + Send;

    /// Get recent videos, optionally filtered by kind.
    fn get_recent_videos(
        &self,
        kind: Option<u16>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<VideoStats>, ClickHouseError>> + Send;

    /// Search videos by hashtag.
    fn search_by_hashtag(
        &self,
        hashtag: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<VideoHashtag>, ClickHouseError>> + Send;

    /// Full-text search videos by title.
    fn search_by_text(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<VideoStats>, ClickHouseError>> + Send;
}

/// Trait for event insertion operations.
#[allow(dead_code)]
pub trait EventWriter: Send + Sync {
    /// Insert a batch of events.
    fn insert_events(
        &self,
        events: &[EventRow],
    ) -> impl Future<Output = Result<(), ClickHouseError>> + Send;
}

/// Trait for stats queries.
#[allow(dead_code)]
pub trait StatsQueries: Send + Sync {
    /// Get total event count.
    fn get_event_count(&self) -> impl Future<Output = Result<u64, ClickHouseError>> + Send;

    /// Get total video count.
    fn get_video_count(&self) -> impl Future<Output = Result<u64, ClickHouseError>> + Send;
}

// Implement traits for ClickHouseClient
impl VideoQueries for crate::ClickHouseClient {
    async fn get_video_stats(&self, event_id: &str) -> Result<Option<VideoStats>, ClickHouseError> {
        self.get_video_stats(event_id).await
    }

    async fn get_videos_by_author(
        &self,
        pubkey: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        self.get_videos_by_author(pubkey, limit).await
    }

    async fn get_trending_videos(&self, limit: u32) -> Result<Vec<TrendingVideo>, ClickHouseError> {
        self.get_trending_videos(limit).await
    }

    async fn get_recent_videos(
        &self,
        kind: Option<u16>,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        self.get_recent_videos(kind, limit).await
    }

    async fn search_by_hashtag(
        &self,
        hashtag: &str,
        limit: u32,
    ) -> Result<Vec<VideoHashtag>, ClickHouseError> {
        self.search_by_hashtag(hashtag, limit).await
    }

    async fn search_by_text(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        self.search_by_text(query, limit).await
    }
}

impl EventWriter for crate::ClickHouseClient {
    async fn insert_events(&self, events: &[EventRow]) -> Result<(), ClickHouseError> {
        self.insert_events(events).await
    }
}

impl StatsQueries for crate::ClickHouseClient {
    async fn get_event_count(&self) -> Result<u64, ClickHouseError> {
        self.get_event_count().await
    }

    async fn get_video_count(&self) -> Result<u64, ClickHouseError> {
        self.get_video_count().await
    }
}
