//! Nostr protocol types and video event parsing for Funnel.
//!
//! This crate wraps the `nostr` crate and provides video-specific event types
//! for kinds 34235 (normal videos) and 34236 (short videos) per NIP-71.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use nostr::{Event, EventId, Kind, PublicKey, Tag, Timestamp};

/// Video event kinds per NIP-71.
pub const KIND_VIDEO: u16 = 34235;
pub const KIND_VIDEO_SHORT: u16 = 34236;

/// Errors that can occur when parsing events.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid event JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid nostr event: {0}")]
    InvalidEvent(String),

    #[error("missing required tag: {0}")]
    MissingTag(String),
}

/// A parsed Nostr event with extracted fields for ClickHouse insertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: DateTime<Utc>,
    pub kind: u16,
    pub content: String,
    pub sig: String,
    pub tags: Vec<Vec<String>>,
}

impl ParsedEvent {
    /// Parse from a nostr Event.
    pub fn from_event(event: &Event) -> Self {
        Self {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: DateTime::from_timestamp(event.created_at.as_u64() as i64, 0)
                .unwrap_or_default(),
            kind: event.kind.as_u16(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
            tags: event
                .tags
                .iter()
                .map(|t| t.as_slice().iter().map(|s| s.to_string()).collect())
                .collect(),
        }
    }

    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, ParseError> {
        let event: Event = serde_json::from_str(json)?;
        Ok(Self::from_event(&event))
    }

    /// Check if this is a video event.
    pub fn is_video(&self) -> bool {
        self.kind == KIND_VIDEO || self.kind == KIND_VIDEO_SHORT
    }

    /// Extract a tag value by name (first occurrence).
    pub fn get_tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.first().map(|s| s.as_str()) == Some(name))
            .and_then(|t| t.get(1).map(|s| s.as_str()))
    }

    /// Extract all tag values for a given name.
    pub fn get_tags(&self, name: &str) -> Vec<&[String]> {
        self.tags
            .iter()
            .filter(|t| t.first().map(|s| s.as_str()) == Some(name))
            .map(|t| t.as_slice())
            .collect()
    }
}

/// Video metadata extracted from a video event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMeta {
    pub d_tag: String,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub video_url: Option<String>,
    pub hashtags: Vec<String>,
}

impl VideoMeta {
    /// Extract video metadata from a parsed event.
    pub fn from_event(event: &ParsedEvent) -> Option<Self> {
        if !event.is_video() {
            return None;
        }

        let d_tag = event.get_tag("d")?.to_string();

        Some(Self {
            d_tag,
            title: event.get_tag("title").map(|s| s.to_string()),
            thumbnail: event
                .get_tag("thumb")
                .or_else(|| event.get_tag("thumbnail"))
                .map(|s| s.to_string()),
            video_url: event.get_tag("url").map(|s| s.to_string()),
            hashtags: event
                .get_tags("t")
                .iter()
                .filter_map(|t| t.get(1).map(|s| s.to_string()))
                .collect(),
        })
    }
}

/// strfry stream message format (JSONL from `strfry stream`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrfryMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub event: Event,
    pub received_at: Option<f64>,
    pub source_type: Option<String>,
    pub source_info: Option<String>,
}

impl StrfryMessage {
    /// Parse from JSON line.
    pub fn from_json(json: &str) -> Result<Self, ParseError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Convert to ParsedEvent.
    pub fn to_parsed_event(&self) -> ParsedEvent {
        ParsedEvent::from_event(&self.event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_kind_constants() {
        assert_eq!(KIND_VIDEO, 34235);
        assert_eq!(KIND_VIDEO_SHORT, 34236);
    }
}
