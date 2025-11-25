//! ClickHouse client wrapper for Funnel.
//!
//! Provides connection management, query builders, and batch insertion
//! for Nostr events.

mod client;
mod error;
pub mod queries;

pub use self::client::ClickHouseClient;
pub use self::error::ClickHouseError;
pub use self::queries::{EventRow, TrendingVideo, VideoHashtag, VideoStats};
