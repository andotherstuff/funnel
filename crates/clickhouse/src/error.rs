use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClickHouseError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(#[from] clickhouse::error::Error),

    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("invalid configuration: {0}")]
    Config(String),
}
