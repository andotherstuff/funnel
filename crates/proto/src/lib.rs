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

    // Sample valid Nostr event JSON (kind 1 text note)
    const VALID_EVENT_JSON: &str = r#"{
        "id": "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
        "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
        "created_at": 1673347337,
        "kind": 1,
        "tags": [
            ["e", "3da979448d9ba263864c4d6f14984c423a3838364ec255f03c7904b1ae77f206"],
            ["p", "bf2376e17ba4ec269d10fcc996a4746b451152be9031fa48e74553dde5526bce"]
        ],
        "content": "Hello, Nostr!",
        "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
    }"#;

    // Sample video event JSON (kind 34235)
    // Uses the same valid pubkey/sig from VALID_EVENT_JSON for simplicity
    const VIDEO_EVENT_JSON: &str = r#"{
        "id": "a376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
        "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
        "created_at": 1700000000,
        "kind": 34235,
        "tags": [
            ["d", "my-video-id"],
            ["title", "My Cool Video"],
            ["thumb", "https://example.com/thumb.jpg"],
            ["url", "https://example.com/video.mp4"],
            ["t", "nostr"],
            ["t", "bitcoin"],
            ["p", "bf2376e17ba4ec269d10fcc996a4746b451152be9031fa48e74553dde5526bce"]
        ],
        "content": "Check out my video!",
        "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
    }"#;

    // Sample short video event JSON (kind 34236)
    const SHORT_VIDEO_EVENT_JSON: &str = r#"{
        "id": "b376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
        "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
        "created_at": 1700000001,
        "kind": 34236,
        "tags": [
            ["d", "my-short-id"],
            ["title", "Short clip"]
        ],
        "content": "",
        "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
    }"#;

    // Sample strfry stream message
    const STRFRY_MESSAGE_JSON: &str = r#"{
        "type": "EVENT",
        "event": {
            "id": "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
            "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
            "created_at": 1673347337,
            "kind": 1,
            "tags": [],
            "content": "Test",
            "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
        },
        "receivedAt": 1673347338.123,
        "sourceType": "IP4",
        "sourceInfo": "192.168.1.1"
    }"#;

    mod parsed_event_tests {
        use super::*;

        #[test]
        fn from_json_valid_event() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();

            assert_eq!(
                event.id,
                "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65"
            );
            assert_eq!(
                event.pubkey,
                "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93"
            );
            assert_eq!(event.kind, 1);
            assert_eq!(event.content, "Hello, Nostr!");
            assert_eq!(event.tags.len(), 2);
        }

        #[test]
        fn from_json_video_event() {
            let event = ParsedEvent::from_json(VIDEO_EVENT_JSON).unwrap();

            assert_eq!(event.kind, KIND_VIDEO);
            assert!(event.is_video());
            assert_eq!(event.tags.len(), 7);
        }

        #[test]
        fn from_json_short_video_event() {
            let event = ParsedEvent::from_json(SHORT_VIDEO_EVENT_JSON).unwrap();

            assert_eq!(event.kind, KIND_VIDEO_SHORT);
            assert!(event.is_video());
        }

        #[test]
        fn from_json_invalid_json() {
            let result = ParsedEvent::from_json("not json");
            assert!(result.is_err());
        }

        #[test]
        fn from_json_missing_fields() {
            let result = ParsedEvent::from_json(r#"{"id": "abc"}"#);
            assert!(result.is_err());
        }

        #[test]
        fn is_video_returns_true_for_video_kinds() {
            let video = ParsedEvent::from_json(VIDEO_EVENT_JSON).unwrap();
            let short = ParsedEvent::from_json(SHORT_VIDEO_EVENT_JSON).unwrap();

            assert!(video.is_video());
            assert!(short.is_video());
        }

        #[test]
        fn is_video_returns_false_for_other_kinds() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();
            assert!(!event.is_video());
        }

        #[test]
        fn get_tag_returns_first_matching_tag() {
            let event = ParsedEvent::from_json(VIDEO_EVENT_JSON).unwrap();

            assert_eq!(event.get_tag("d"), Some("my-video-id"));
            assert_eq!(event.get_tag("title"), Some("My Cool Video"));
            assert_eq!(event.get_tag("thumb"), Some("https://example.com/thumb.jpg"));
            assert_eq!(event.get_tag("url"), Some("https://example.com/video.mp4"));
        }

        #[test]
        fn get_tag_returns_none_for_missing_tag() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();
            assert_eq!(event.get_tag("nonexistent"), None);
        }

        #[test]
        fn get_tags_returns_all_matching_tags() {
            let event = ParsedEvent::from_json(VIDEO_EVENT_JSON).unwrap();

            let t_tags = event.get_tags("t");
            assert_eq!(t_tags.len(), 2);
            assert_eq!(t_tags[0], &["t", "nostr"]);
            assert_eq!(t_tags[1], &["t", "bitcoin"]);
        }

        #[test]
        fn get_tags_returns_empty_for_no_matches() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();
            let tags = event.get_tags("nonexistent");
            assert!(tags.is_empty());
        }

        #[test]
        fn created_at_is_parsed_correctly() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();
            assert_eq!(event.created_at.timestamp(), 1673347337);
        }
    }

    mod video_meta_tests {
        use super::*;

        #[test]
        fn from_event_extracts_all_fields() {
            let event = ParsedEvent::from_json(VIDEO_EVENT_JSON).unwrap();
            let meta = VideoMeta::from_event(&event).unwrap();

            assert_eq!(meta.d_tag, "my-video-id");
            assert_eq!(meta.title, Some("My Cool Video".to_string()));
            assert_eq!(
                meta.thumbnail,
                Some("https://example.com/thumb.jpg".to_string())
            );
            assert_eq!(meta.video_url, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(meta.hashtags, vec!["nostr", "bitcoin"]);
        }

        #[test]
        fn from_event_handles_minimal_video() {
            let event = ParsedEvent::from_json(SHORT_VIDEO_EVENT_JSON).unwrap();
            let meta = VideoMeta::from_event(&event).unwrap();

            assert_eq!(meta.d_tag, "my-short-id");
            assert_eq!(meta.title, Some("Short clip".to_string()));
            assert_eq!(meta.thumbnail, None);
            assert_eq!(meta.video_url, None);
            assert!(meta.hashtags.is_empty());
        }

        #[test]
        fn from_event_returns_none_for_non_video() {
            let event = ParsedEvent::from_json(VALID_EVENT_JSON).unwrap();
            let meta = VideoMeta::from_event(&event);
            assert!(meta.is_none());
        }

        #[test]
        fn from_event_returns_none_without_d_tag() {
            // Video event without d tag
            let json = r#"{
                "id": "c376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
                "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
                "created_at": 1700000000,
                "kind": 34235,
                "tags": [["title", "No D Tag"]],
                "content": "",
                "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
            }"#;

            let event = ParsedEvent::from_json(json).unwrap();
            let meta = VideoMeta::from_event(&event);
            assert!(meta.is_none());
        }

        #[test]
        fn from_event_uses_thumbnail_fallback() {
            // Video with "thumbnail" instead of "thumb"
            let json = r#"{
                "id": "d376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
                "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
                "created_at": 1700000000,
                "kind": 34235,
                "tags": [
                    ["d", "test"],
                    ["thumbnail", "https://example.com/alt-thumb.jpg"]
                ],
                "content": "",
                "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
            }"#;

            let event = ParsedEvent::from_json(json).unwrap();
            let meta = VideoMeta::from_event(&event).unwrap();
            assert_eq!(
                meta.thumbnail,
                Some("https://example.com/alt-thumb.jpg".to_string())
            );
        }
    }

    mod strfry_message_tests {
        use super::*;

        #[test]
        fn from_json_valid_message() {
            let msg = StrfryMessage::from_json(STRFRY_MESSAGE_JSON).unwrap();

            assert_eq!(msg.msg_type, "EVENT");
            assert_eq!(
                msg.event.id.to_hex(),
                "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65"
            );
            assert!(msg.received_at.is_some());
            assert_eq!(msg.source_type, Some("IP4".to_string()));
            assert_eq!(msg.source_info, Some("192.168.1.1".to_string()));
        }

        #[test]
        fn from_json_invalid_json() {
            let result = StrfryMessage::from_json("not json");
            assert!(result.is_err());
        }

        #[test]
        fn to_parsed_event_converts_correctly() {
            let msg = StrfryMessage::from_json(STRFRY_MESSAGE_JSON).unwrap();
            let event = msg.to_parsed_event();

            assert_eq!(
                event.id,
                "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65"
            );
            assert_eq!(event.kind, 1);
            assert_eq!(event.content, "Test");
        }

        #[test]
        fn from_json_without_optional_fields() {
            let json = r#"{
                "type": "EVENT",
                "event": {
                    "id": "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65",
                    "pubkey": "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93",
                    "created_at": 1673347337,
                    "kind": 1,
                    "tags": [],
                    "content": "Test",
                    "sig": "908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"
                }
            }"#;

            let msg = StrfryMessage::from_json(json).unwrap();
            assert!(msg.received_at.is_none());
            assert!(msg.source_type.is_none());
            assert!(msg.source_info.is_none());
        }
    }
}
