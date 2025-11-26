//! Funnel Ingestion Service
//!
//! Connects to a Nostr relay and streams events to ClickHouse.
//! Uses nostr-sdk for reliable relay connections.

use std::env;
use std::time::{Duration, Instant};

use nostr_sdk::prelude::*;

use funnel_clickhouse::{ClickHouseClient, ClickHouseConfig};
use funnel_ingestion::{BatchConfig, BatchProcessor, FlushReason};
use funnel_observability::{ingestion, init_tracing_dev};
use funnel_proto::ParsedEvent;
use metrics::{counter, gauge, histogram};

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 100;

/// Buffer time to account for backdated events (Nostr allows events with past timestamps)
const CATCHUP_BUFFER_SECS: u64 = 2 * 24 * 60 * 60; // 2 days

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install rustls crypto provider (required before any TLS operations)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    init_tracing_dev();

    let relay_url = env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:7777".to_string());
    let ch_config = ClickHouseConfig::from_env()?;
    let batch_size: usize = env::var("BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BATCH_SIZE);
    let flush_interval = Duration::from_millis(
        env::var("FLUSH_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_FLUSH_INTERVAL_MS),
    );

    tracing::info!(
        relay_url = %relay_url,
        clickhouse_url = %ch_config.safe_url(),
        database = %ch_config.database,
        batch_size = batch_size,
        flush_interval_ms = ?flush_interval.as_millis(),
        "Starting ingestion service"
    );

    // Initialize metrics
    let _metrics = funnel_observability::init_metrics();

    // Connect to ClickHouse
    let client = ClickHouseClient::from_config(&ch_config)?;
    client.ping().await?;
    let version = client.version().await?;
    tracing::info!(version = %version, "Connected to ClickHouse");

    // Run ingestion
    run_ingestion(&client, &relay_url, batch_size, flush_interval).await
}

async fn run_ingestion(
    clickhouse: &ClickHouseClient,
    relay_url: &str,
    batch_size: usize,
    flush_interval: Duration,
) -> anyhow::Result<()> {
    // Get the latest event timestamp for catch-up
    let since_timestamp = clickhouse.get_latest_event_timestamp().await?;

    // Apply 2-day buffer to catch backdated events
    let since_with_buffer = since_timestamp.map(|ts| ts - CATCHUP_BUFFER_SECS as i64);

    let filter = if let Some(ts) = since_with_buffer {
        tracing::info!(
            latest_event = since_timestamp.unwrap_or(0),
            since_with_buffer = ts,
            buffer_days = 2,
            "Catching up with buffer for backdated events"
        );
        Filter::new().since(Timestamp::from(ts as u64))
    } else {
        tracing::info!("No existing events, subscribing to all events");
        Filter::new()
    };

    // Create nostr client (no keys needed for read-only)
    let client = Client::builder().build();

    // Add relay
    tracing::info!(url = %relay_url, "Adding relay");
    client.add_relay(relay_url).await?;

    // Connect
    tracing::info!("Connecting to relay...");
    client.connect().await;
    tracing::info!("Connected to relay");

    // Subscribe
    let sub_id = client.subscribe(filter, None).await?;
    tracing::info!(subscription_id = %sub_id.to_string(), "Subscribed to events");

    // Batch processor
    let config = BatchConfig::new(batch_size, flush_interval);
    let mut processor = BatchProcessor::new(config);
    let mut last_flush = Instant::now();
    let mut events_since_log = 0u64;
    let mut last_log = Instant::now();

    // Process notifications
    tracing::info!("Waiting for events...");
    let mut notifications = client.notifications();
    loop {
        // Use tokio::select! to handle both notifications and flush timeout
        tokio::select! {
            // Wait for next notification with timeout
            result = tokio::time::timeout(flush_interval, notifications.recv()) => {
                match result {
                    Ok(Ok(notification)) => {
                        match notification {
                            RelayPoolNotification::Event { event, .. } => {
                                // Convert to our ParsedEvent
                                if let Ok(parsed) = convert_event(&event) {
                                    counter!(ingestion::EVENTS_RECEIVED, "kind" => parsed.kind.to_string())
                                        .increment(1);

                                    processor.push(parsed);
                                    events_since_log += 1;

                                    // Log progress every 10 seconds
                                    if last_log.elapsed() >= Duration::from_secs(10) {
                                        tracing::info!(
                                            events_received = events_since_log,
                                            batch_size = processor.len(),
                                            "Progress"
                                        );
                                        events_since_log = 0;
                                        last_log = Instant::now();
                                    }

                                    // Check if batch is full
                                    if processor.should_flush() == FlushReason::BatchFull
                                        && let Some(batch) = processor.take_batch()
                                    {
                                        flush_batch(clickhouse, batch).await?;
                                    }
                                }
                            }
                            RelayPoolNotification::Message { message, .. } => {
                                if let RelayMessage::EndOfStoredEvents(_) = message {
                                    tracing::info!("End of stored events (EOSE) received");
                                }
                            }
                            RelayPoolNotification::Shutdown => {
                                tracing::warn!("Relay pool shutdown");
                                break;
                            }
                        }
                    }
                    Ok(Err(_)) => {
                        // Channel closed
                        tracing::warn!("Notification channel closed");
                        break;
                    }
                    Err(_) => {
                        // Timeout - check for flush
                    }
                }
            }
        }

        // Check for time-based flush
        if last_flush.elapsed() >= flush_interval {
            if processor.should_flush() == FlushReason::TimeoutReached
                && let Some(batch) = processor.take_batch()
            {
                flush_batch(clickhouse, batch).await?;
            }
            last_flush = Instant::now();
        }

        // Update lag metric
        if let Some(oldest) = processor.oldest_event() {
            let lag = chrono::Utc::now()
                .signed_duration_since(oldest.created_at)
                .num_seconds() as f64;
            gauge!(ingestion::LAG).set(lag);
        } else {
            gauge!(ingestion::LAG).set(0.0);
        }
    }

    // Flush any remaining events
    let batch = processor.take_batch_force();
    if !batch.is_empty() {
        flush_batch(clickhouse, batch).await?;
    }

    Ok(())
}

/// Convert nostr_sdk Event to our ParsedEvent
fn convert_event(event: &Event) -> anyhow::Result<ParsedEvent> {
    // Serialize to JSON and parse with our parser
    let json = event.as_json();
    ParsedEvent::from_json(&json).map_err(|e| anyhow::anyhow!("Failed to parse event: {}", e))
}

async fn flush_batch(client: &ClickHouseClient, batch: Vec<ParsedEvent>) -> anyhow::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    histogram!(ingestion::BATCH_SIZE).record(batch.len() as f64);

    let start = Instant::now();

    let rows: Vec<_> = batch
        .iter()
        .map(|e| funnel_clickhouse::EventRow::from_parsed(e, ""))
        .collect();

    client.insert_events(&rows).await?;

    let duration = start.elapsed();
    histogram!(ingestion::WRITE_LATENCY).record(duration.as_secs_f64());
    counter!(ingestion::EVENTS_WRITTEN).increment(batch.len() as u64);

    tracing::info!(
        count = batch.len(),
        duration_ms = duration.as_millis(),
        "Flushed batch to ClickHouse"
    );

    Ok(())
}
