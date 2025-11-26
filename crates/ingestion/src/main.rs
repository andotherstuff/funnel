//! Funnel Ingestion Service
//!
//! Connects to a Nostr relay via websocket and streams events to ClickHouse.
//! On startup, queries ClickHouse for the latest event timestamp and subscribes
//! with a `since` filter for catch-up after restarts.

use std::env;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use funnel_clickhouse::ClickHouseClient;
use funnel_ingestion::{BatchConfig, BatchProcessor, FlushReason};
use funnel_observability::{ingestion, init_tracing_dev};
use funnel_proto::ParsedEvent;
use metrics::{counter, gauge, histogram};

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 100;
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const SUBSCRIPTION_ID: &str = "funnel-ingestion";

/// Buffer time to account for backdated events (Nostr allows events with past timestamps)
const CATCHUP_BUFFER_SECS: i64 = 2 * 24 * 60 * 60; // 2 days

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_dev();

    let relay_url =
        env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:7777".to_string());
    let clickhouse_url =
        env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let database = env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "nostr".to_string());
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
        clickhouse_url = %clickhouse_url,
        database = %database,
        batch_size = batch_size,
        flush_interval_ms = ?flush_interval.as_millis(),
        "Starting ingestion service"
    );

    // Initialize metrics
    let _metrics = funnel_observability::init_metrics();

    // Connect to ClickHouse
    let client = ClickHouseClient::new(&clickhouse_url, &database)?;
    client.ping().await?;
    let version = client.version().await?;
    tracing::info!(version = %version, "Connected to ClickHouse");

    // Run the main loop with reconnection
    loop {
        match run_ingestion(&client, &relay_url, batch_size, flush_interval).await {
            Ok(()) => {
                tracing::info!("Ingestion stopped gracefully");
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "Ingestion error, reconnecting in {:?}", RECONNECT_DELAY);
                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        }
    }

    Ok(())
}

async fn run_ingestion(
    client: &ClickHouseClient,
    relay_url: &str,
    batch_size: usize,
    flush_interval: Duration,
) -> anyhow::Result<()> {
    // Get the latest event timestamp for catch-up
    let since_timestamp = client.get_latest_event_timestamp().await?;
    
    // Apply 2-day buffer to catch backdated events (Nostr allows events with past timestamps)
    // ClickHouse's ReplacingMergeTree will deduplicate by event ID
    let since_with_buffer = since_timestamp.map(|ts| ts - CATCHUP_BUFFER_SECS);
    
    if let Some(ts) = since_with_buffer {
        tracing::info!(
            latest_event = since_timestamp.unwrap_or(0),
            since_with_buffer = ts,
            buffer_days = 2,
            "Catching up with buffer for backdated events"
        );
    } else {
        tracing::info!("No existing events, subscribing to all events");
    }

    // Connect to relay
    tracing::info!(url = %relay_url, "Connecting to relay");
    let (ws_stream, _) = connect_async(relay_url).await?;
    let (mut write, mut read) = ws_stream.split();
    tracing::info!("Connected to relay");

    // Build subscription filter
    let filter = if let Some(ts) = since_with_buffer {
        // Subscribe to events since (last known - 2 days) to catch backdated events
        serde_json::json!({ "since": ts })
    } else {
        // Subscribe to all events (no filter)
        serde_json::json!({})
    };

    // Send REQ message: ["REQ", "<sub_id>", <filter>]
    let req_msg = serde_json::json!(["REQ", SUBSCRIPTION_ID, filter]);
    write.send(Message::Text(req_msg.to_string())).await?;
    tracing::info!(filter = %filter, "Sent subscription request");

    // Batch processor
    let config = BatchConfig::new(batch_size, flush_interval);
    let mut processor = BatchProcessor::new(config);
    let mut last_flush_check = Instant::now();

    loop {
        // Check for flush timeout
        if last_flush_check.elapsed() >= flush_interval {
            if processor.should_flush() == FlushReason::TimeoutReached
                && let Some(batch) = processor.take_batch()
            {
                flush_batch(client, batch).await?;
            }
            last_flush_check = Instant::now();
        }

        // Read next message with timeout
        let msg = tokio::time::timeout(flush_interval, read.next()).await;

        match msg {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Some(event) = parse_relay_message(&text) {
                    counter!(ingestion::EVENTS_RECEIVED, "kind" => event.kind.to_string())
                        .increment(1);

                    processor.push(event);

                    // Check if batch is full
                    if processor.should_flush() == FlushReason::BatchFull
                        && let Some(batch) = processor.take_batch()
                    {
                        flush_batch(client, batch).await?;
                    }
                }
            }
            Ok(Some(Ok(Message::Ping(data)))) => {
                write.send(Message::Pong(data)).await?;
            }
            Ok(Some(Ok(Message::Pong(_)))) => {
                // Ignore pong messages
            }
            Ok(Some(Ok(Message::Binary(_)))) => {
                // Nostr doesn't use binary messages, ignore
            }
            Ok(Some(Ok(Message::Frame(_)))) => {
                // Raw frames, ignore
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                tracing::warn!("Relay closed connection");
                // Flush remaining events before returning
                let batch = processor.take_batch_force();
                if !batch.is_empty() {
                    flush_batch(client, batch).await?;
                }
                return Err(anyhow::anyhow!("Connection closed by relay"));
            }
            Ok(Some(Err(e))) => {
                tracing::error!(error = %e, "WebSocket error");
                // Flush remaining events before returning
                let batch = processor.take_batch_force();
                if !batch.is_empty() {
                    flush_batch(client, batch).await?;
                }
                return Err(e.into());
            }
            Ok(None) => {
                tracing::warn!("WebSocket stream ended");
                // Flush remaining events before returning
                let batch = processor.take_batch_force();
                if !batch.is_empty() {
                    flush_batch(client, batch).await?;
                }
                return Err(anyhow::anyhow!("WebSocket stream ended"));
            }
            Err(_) => {
                // Timeout - check for flush and continue
                continue;
            }
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
}

/// Parse a Nostr relay message and extract the event if it's an EVENT message.
///
/// Relay messages are JSON arrays:
/// - ["EVENT", "<sub_id>", <event>] - An event matching the subscription
/// - ["EOSE", "<sub_id>"] - End of stored events
/// - ["NOTICE", "<message>"] - A notice from the relay
fn parse_relay_message(text: &str) -> Option<ParsedEvent> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let arr = value.as_array()?;

    // Check if it's an EVENT message
    let msg_type = arr.first()?.as_str()?;

    if msg_type == "EVENT" && arr.len() >= 3 {
        // arr[1] is subscription ID, arr[2] is the event
        let event_json = arr.get(2)?;
        let event_str = serde_json::to_string(event_json).ok()?;
        ParsedEvent::from_json(&event_str).ok()
    } else if msg_type == "EOSE" {
        tracing::debug!("End of stored events");
        None
    } else if msg_type == "NOTICE" {
        let notice = arr.get(1).and_then(|v| v.as_str()).unwrap_or("unknown");
        tracing::info!(notice = notice, "Relay notice");
        None
    } else {
        None
    }
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

    tracing::debug!(
        count = batch.len(),
        duration_ms = duration.as_millis(),
        "Flushed batch"
    );

    Ok(())
}
