use clickhouse::Client;
use url::Url;

use crate::error::ClickHouseError;
use crate::queries::{EventRow, TrendingVideo, VideoHashtag, VideoStats};

/// ClickHouse client wrapper with connection pooling and query methods.
#[derive(Clone)]
pub struct ClickHouseClient {
    client: Client,
    database: String,
}

/// Configuration for connecting to ClickHouse.
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: Option<String>,
    pub password: Option<String>,
}

impl ClickHouseConfig {
    /// Create config from environment variables.
    ///
    /// Reads:
    /// - `CLICKHOUSE_URL` (required): Base URL like `https://host:8443`
    /// - `CLICKHOUSE_DATABASE` (optional): Database name, defaults to "nostr"
    /// - `CLICKHOUSE_USER` (optional): Username, defaults to "default"
    /// - `CLICKHOUSE_PASSWORD` (optional): Password
    pub fn from_env() -> Result<Self, ClickHouseError> {
        let url = std::env::var("CLICKHOUSE_URL")
            .map_err(|_| ClickHouseError::Config("CLICKHOUSE_URL not set".to_string()))?;
        let database = std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "nostr".to_string());
        let user = std::env::var("CLICKHOUSE_USER").ok();
        let password = std::env::var("CLICKHOUSE_PASSWORD").ok();

        Ok(Self {
            url,
            database,
            user,
            password,
        })
    }

    /// Returns a redacted version of the URL for logging (no credentials).
    pub fn safe_url(&self) -> &str {
        &self.url
    }
}

impl ClickHouseClient {
    /// Create a new client from configuration.
    pub fn from_config(config: &ClickHouseConfig) -> Result<Self, ClickHouseError> {
        let parsed_url = Url::parse(&config.url)
            .map_err(|e| ClickHouseError::Config(format!("Invalid ClickHouse URL: {}", e)))?;

        // Build base URL without query params
        let base_url = format!(
            "{}://{}:{}",
            parsed_url.scheme(),
            parsed_url.host_str().unwrap_or("localhost"),
            parsed_url
                .port()
                .unwrap_or(if parsed_url.scheme() == "https" {
                    8443
                } else {
                    8123
                })
        );

        let mut client = Client::default()
            .with_url(&base_url)
            .with_database(&config.database)
            .with_option("async_insert", "1")
            .with_option("wait_for_async_insert", "0");

        // Add auth if present
        if let Some(ref u) = config.user {
            client = client.with_user(u);
        }
        if let Some(ref p) = config.password {
            client = client.with_password(p);
        }

        Ok(Self {
            client,
            database: config.database.clone(),
        })
    }

    /// Create a new client connected to the given URL (legacy, for tests).
    #[doc(hidden)]
    pub fn new(url: &str, database: &str) -> Result<Self, ClickHouseError> {
        let config = ClickHouseConfig {
            url: url.to_string(),
            database: database.to_string(),
            user: Some("default".to_string()),
            password: None,
        };
        Self::from_config(&config)
    }

    /// Test the connection by running a simple query.
    pub async fn ping(&self) -> Result<(), ClickHouseError> {
        self.client.query("SELECT 1").execute().await?;
        tracing::debug!("ClickHouse ping successful");
        Ok(())
    }

    /// Get the server version.
    pub async fn version(&self) -> Result<String, ClickHouseError> {
        let version: String = self.client.query("SELECT version()").fetch_one().await?;
        Ok(version)
    }

    /// Insert a batch of events into the events_local table.
    pub async fn insert_events(&self, events: &[EventRow]) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("events_local")?;

        for event in events {
            insert.write(event).await?;
        }

        insert.end().await?;

        tracing::debug!(count = events.len(), "Inserted events batch");
        Ok(())
    }

    /// Get video stats by event ID.
    pub async fn get_video_stats(
        &self,
        event_id: &str,
    ) -> Result<Option<VideoStats>, ClickHouseError> {
        let result = self
            .client
            .query("SELECT * FROM video_stats WHERE id = ?")
            .bind(event_id)
            .fetch_optional()
            .await?;

        Ok(result)
    }

    /// Get videos by author pubkey.
    pub async fn get_videos_by_author(
        &self,
        pubkey: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        let results = self
            .client
            .query("SELECT * FROM video_stats WHERE pubkey = ? ORDER BY created_at DESC LIMIT ?")
            .bind(pubkey)
            .bind(limit)
            .fetch_all()
            .await?;

        Ok(results)
    }

    /// Get trending videos.
    pub async fn get_trending_videos(
        &self,
        limit: u32,
    ) -> Result<Vec<TrendingVideo>, ClickHouseError> {
        let results = self
            .client
            .query("SELECT * FROM trending_videos LIMIT ?")
            .bind(limit)
            .fetch_all()
            .await?;

        Ok(results)
    }

    /// Get recent videos, optionally filtered by kind.
    pub async fn get_recent_videos(
        &self,
        kind: Option<u16>,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        let query = match kind {
            Some(k) => self
                .client
                .query("SELECT * FROM video_stats WHERE kind = ? ORDER BY created_at DESC LIMIT ?")
                .bind(k)
                .bind(limit),
            None => self
                .client
                .query("SELECT * FROM video_stats ORDER BY created_at DESC LIMIT ?")
                .bind(limit),
        };

        let results = query.fetch_all().await?;
        Ok(results)
    }

    /// Search videos by hashtag.
    pub async fn search_by_hashtag(
        &self,
        hashtag: &str,
        limit: u32,
    ) -> Result<Vec<VideoHashtag>, ClickHouseError> {
        let results = self
            .client
            .query(
                "SELECT * FROM video_hashtags WHERE hashtag = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(hashtag)
            .bind(limit)
            .fetch_all()
            .await?;

        Ok(results)
    }

    /// Full-text search videos by title.
    ///
    /// Uses `hasTokenCaseInsensitive` for word-boundary matching.
    /// Searches each word in the query independently.
    pub async fn search_by_text(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<VideoStats>, ClickHouseError> {
        // Split query into tokens and filter empty strings
        let tokens: Vec<&str> = query.split_whitespace().collect();

        if tokens.is_empty() {
            return Ok(vec![]);
        }

        // Build WHERE clause: all tokens must match (AND)
        let conditions: Vec<String> = tokens
            .iter()
            .map(|_| "hasTokenCaseInsensitive(title, ?)".to_string())
            .collect();
        let where_clause = conditions.join(" AND ");

        let sql = format!(
            "SELECT * FROM video_stats WHERE {} ORDER BY created_at DESC LIMIT ?",
            where_clause
        );

        let mut query_builder = self.client.query(&sql);

        // Bind each token
        for token in &tokens {
            query_builder = query_builder.bind(*token);
        }

        // Bind limit
        query_builder = query_builder.bind(limit);

        let results = query_builder.fetch_all().await?;
        Ok(results)
    }

    /// Get event count.
    pub async fn get_event_count(&self) -> Result<u64, ClickHouseError> {
        let count: u64 = self
            .client
            .query("SELECT count() FROM events_local")
            .fetch_one()
            .await?;

        Ok(count)
    }

    /// Get video count.
    pub async fn get_video_count(&self) -> Result<u64, ClickHouseError> {
        let count: u64 = self
            .client
            .query("SELECT count() FROM videos")
            .fetch_one()
            .await?;

        Ok(count)
    }

    /// Check if the schema is set up.
    pub async fn check_schema(&self) -> Result<bool, ClickHouseError> {
        let count: u64 = self
            .client
            .query("SELECT count() FROM system.tables WHERE database = ?")
            .bind(&self.database)
            .fetch_one()
            .await?;

        Ok(count > 0)
    }

    /// Execute a raw DDL statement (for schema setup).
    pub async fn execute_ddl(&self, ddl: &str) -> Result<(), ClickHouseError> {
        self.client.query(ddl).execute().await?;
        Ok(())
    }

    /// Get the latest event timestamp for catch-up sync.
    ///
    /// Returns the maximum `created_at` timestamp (as Unix timestamp) from the events table,
    /// or None if the table is empty.
    pub async fn get_latest_event_timestamp(&self) -> Result<Option<i64>, ClickHouseError> {
        // Use toUnixTimestamp to convert DateTime to Unix timestamp
        // and count() to check if table has any events
        let count: u64 = self
            .client
            .query("SELECT count() FROM events_local")
            .fetch_one()
            .await?;

        if count == 0 {
            return Ok(None);
        }

        let timestamp: i64 = self
            .client
            .query("SELECT toUnixTimestamp(max(created_at)) FROM events_local")
            .fetch_one()
            .await?;

        Ok(Some(timestamp))
    }
}
