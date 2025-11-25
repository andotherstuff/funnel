use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

/// Row structure for inserting events into ClickHouse.
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct EventRow {
    pub id: String,
    pub pubkey: String,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub created_at: DateTime<Utc>,
    pub kind: u16,
    pub content: String,
    pub sig: String,
    pub tags: Vec<Vec<String>>,
    pub relay_source: String,
}

impl EventRow {
    pub fn from_parsed(event: &funnel_proto::ParsedEvent, relay_source: &str) -> Self {
        Self {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            created_at: event.created_at,
            kind: event.kind,
            content: event.content.clone(),
            sig: event.sig.clone(),
            tags: event.tags.clone(),
            relay_source: relay_source.to_string(),
        }
    }
}

/// Video stats returned from the video_stats view.
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct VideoStats {
    pub id: String,
    pub pubkey: String,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub created_at: DateTime<Utc>,
    pub kind: u16,
    pub d_tag: String,
    pub title: String,
    pub thumbnail: String,
    pub reactions: u64,
    pub comments: u64,
    pub reposts: u64,
    pub engagement_score: u64,
}

/// Trending video with score.
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TrendingVideo {
    pub id: String,
    pub pubkey: String,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub created_at: DateTime<Utc>,
    pub kind: u16,
    pub d_tag: String,
    pub title: String,
    pub thumbnail: String,
    pub reactions: u64,
    pub comments: u64,
    pub reposts: u64,
    pub engagement_score: u64,
    pub trending_score: f64,
}

/// Video hashtag mapping.
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct VideoHashtag {
    pub event_id: String,
    pub hashtag: String,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub created_at: DateTime<Utc>,
    pub pubkey: String,
    pub kind: u16,
    pub title: String,
    pub thumbnail: String,
    pub d_tag: String,
}
