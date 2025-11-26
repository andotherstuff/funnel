//! ClickHouse client wrapper for Funnel.
//!
//! Provides connection management, query builders, and batch insertion
//! for Nostr events.

mod client;
mod error;
pub mod queries;
pub mod traits;

pub use self::client::{ClickHouseClient, ClickHouseConfig};
pub use self::error::ClickHouseError;
pub use self::queries::{EventRow, TrendingVideo, VideoHashtag, VideoStats};
pub use self::traits::{EventWriter, StatsQueries, VideoQueries};
