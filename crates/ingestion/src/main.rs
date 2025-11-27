//! Funnel Ingestion Service
//!
//! Connects to a Nostr relay and streams events to ClickHouse.
//!
//! ## Modes
//! - **Live mode** (default): Subscribes from last known timestamp, streams new events
//! - **Backfill mode** (`--backfill`): Paginates through all historical events
//!
//! ## Deduplication
//! ClickHouse's ReplacingMergeTree handles deduplication by event ID.

use std::env;
use std::time::{Duration, Instant};

use nostr_sdk::prelude::*;

use funnel_clickhouse::{ClickHouseClient, ClickHouseConfig};
use funnel_observability::{ingestion, init_tracing_dev};
use funnel_proto::ParsedEvent;
use metrics::{counter, gauge, histogram};

const DEFAULT_BATCH_SIZE: usize = 1000;
const PAGINATION_LIMIT: usize = 5000;
const PAGINATE_INTERVAL_MS: u64 = 500;

/// Buffer time to account for backdated events
const CATCHUP_BUFFER_SECS: u64 = 2 * 24 * 60 * 60; // 2 days

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install rustls crypto provider
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
    let backfill_mode = env::var("BACKFILL").is_ok();

    tracing::info!(
        relay_url = %relay_url,
        clickhouse_url = %ch_config.safe_url(),
        database = %ch_config.database,
        batch_size = batch_size,
        backfill_mode = backfill_mode,
        "Starting ingestion service"
    );

    let _metrics = funnel_observability::init_metrics();

    // Connect to ClickHouse
    let clickhouse = ClickHouseClient::from_config(&ch_config)?;
    clickhouse.ping().await?;
    let version = clickhouse.version().await?;
    tracing::info!(version = %version, "Connected to ClickHouse");

    if backfill_mode {
        tracing::info!("Running in BACKFILL mode - paginating through all historical events");
        backfill(&clickhouse, &relay_url, batch_size).await
    } else {
        tracing::info!("Running in LIVE mode - streaming new events");
        live_stream(&clickhouse, &relay_url, batch_size).await
    }
}

/// Backfill mode: Paginate through all historical events
async fn backfill(
    clickhouse: &ClickHouseClient,
    relay_url: &str,
    batch_size: usize,
) -> anyhow::Result<()> {
    let client = Client::builder().build();
    client.add_relay(relay_url).await?;

    tracing::info!("Connecting to relay...");
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    tracing::info!("Connected to relay");

    let mut total_events = 0u64;
    let mut until: Option<Timestamp> = None;
    let mut consecutive_empty = 0;
    let paginate_interval = Duration::from_millis(PAGINATE_INTERVAL_MS);

    loop {
        let filter = match until {
            Some(ts) => Filter::new().until(ts).limit(PAGINATION_LIMIT),
            None => Filter::new().limit(PAGINATION_LIMIT),
        };

        tracing::info!(
            until = ?until.map(|t| t.to_human_datetime()),
            limit = PAGINATION_LIMIT,
            total_so_far = total_events,
            "Fetching batch"
        );

        let events = match client.fetch_events(filter, Duration::from_secs(60)).await {
            Ok(events) => events,
            Err(e) => {
                tracing::warn!(error = %e, "Fetch failed, retrying...");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let count = events.len();

        if count == 0 {
            consecutive_empty += 1;
            if consecutive_empty >= 3 {
                tracing::info!("No more events after 3 empty batches");
                break;
            }
            tokio::time::sleep(paginate_interval).await;
            if let Some(ts) = until {
                until = Some(Timestamp::from(ts.as_secs().saturating_sub(86400 * 7)));
            }
            continue;
        }

        consecutive_empty = 0;

        let oldest_ts = events.iter().map(|e| e.created_at).min().unwrap();

        tracing::info!(
            count = count,
            oldest = ?oldest_ts.to_human_datetime(),
            "Received batch"
        );

        // Convert and insert - ClickHouse handles deduplication
        let batch: Vec<ParsedEvent> = events
            .into_iter()
            .filter_map(|e| convert_event(&e).ok())
            .collect();

        for chunk in batch.chunks(batch_size) {
            let rows: Vec<_> = chunk
                .iter()
                .map(|e| funnel_clickhouse::EventRow::from_parsed(e, ""))
                .collect();
            clickhouse.insert_events(&rows).await?;
            total_events += rows.len() as u64;
        }

        tracing::info!(
            batch_inserted = batch.len(),
            total_events = total_events,
            "Inserted"
        );

        until = Some(Timestamp::from(oldest_ts.as_secs().saturating_sub(1)));
        tokio::time::sleep(paginate_interval).await;
    }

    tracing::info!(total_events = total_events, "Backfill complete");
    client.disconnect().await;
    Ok(())
}

/// Live mode: Subscribe from last known timestamp and stream new events
async fn live_stream(
    clickhouse: &ClickHouseClient,
    relay_url: &str,
    batch_size: usize,
) -> anyhow::Result<()> {
    // Get latest event timestamp from ClickHouse
    let since_timestamp = clickhouse.get_latest_event_timestamp().await?;
    let since_with_buffer = since_timestamp.map(|ts| ts - CATCHUP_BUFFER_SECS as i64);

    let filter = match since_with_buffer {
        Some(ts) => {
            tracing::info!(
                latest_event = since_timestamp.unwrap_or(0),
                since_with_buffer = ts,
                "Subscribing from last known timestamp with buffer"
            );
            Filter::new().since(Timestamp::from(ts as u64))
        }
        None => {
            tracing::info!(
                "No existing events, subscribing to new events only (use BACKFILL=1 for historical)"
            );
            Filter::new().since(Timestamp::now())
        }
    };

    let client = Client::builder().build();
    client.add_relay(relay_url).await?;

    tracing::info!("Connecting to relay...");
    client.connect().await;
    tracing::info!("Connected");

    let output = client.subscribe(filter, None).await?;
    tracing::info!(subscription_id = %output.id(), "Subscribed");

    let mut notifications = client.notifications();
    let mut batch: Vec<ParsedEvent> = Vec::with_capacity(batch_size);
    let mut last_log = Instant::now();
    let mut events_since_log = 0u64;

    tracing::info!("Streaming events (drain strategy)...");

    loop {
        // Drain strategy: try to receive without blocking first
        loop {
            match notifications.try_recv() {
                Ok(notification) => {
                    if let Some(event) = handle_notification(notification) {
                        batch.push(event);
                        events_since_log += 1;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Channel lagged, some events may be lost");
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    tracing::warn!("Channel closed");
                    return Ok(());
                }
            }
        }

        // Update lag metric BEFORE flush (using oldest event by created_at)
        if let Some(oldest) = batch.iter().min_by_key(|e| e.created_at) {
            let lag = chrono::Utc::now()
                .signed_duration_since(oldest.created_at)
                .num_seconds() as f64;
            gauge!(ingestion::LAG).set(lag);
        }

        // Flush if we have events
        if !batch.is_empty() {
            flush_batch(clickhouse, &mut batch).await?;
        }

        // Log progress
        if last_log.elapsed() >= Duration::from_secs(30) {
            tracing::info!(events_received = events_since_log, "Progress");
            events_since_log = 0;
            last_log = Instant::now();
        }

        // Wait for more events (with timeout to allow periodic flush checks)
        match tokio::time::timeout(Duration::from_millis(100), notifications.recv()).await {
            Ok(Ok(notification)) => {
                if let Some(event) = handle_notification(notification) {
                    batch.push(event);
                    events_since_log += 1;
                }
            }
            Ok(Err(_)) => {
                tracing::warn!("Channel closed");
                break;
            }
            Err(_) => {
                // Timeout - continue loop to drain and flush
            }
        }
    }

    // Final flush
    if !batch.is_empty() {
        flush_batch(clickhouse, &mut batch).await?;
    }

    Ok(())
}

fn handle_notification(notification: RelayPoolNotification) -> Option<ParsedEvent> {
    match notification {
        RelayPoolNotification::Event { event, .. } => {
            counter!(ingestion::EVENTS_RECEIVED, "kind" => event.kind.as_u16().to_string())
                .increment(1);
            convert_event(&event).ok()
        }
        RelayPoolNotification::Message { message, .. } => {
            if let RelayMessage::EndOfStoredEvents(_) = message {
                tracing::info!("EOSE received - now streaming live events");
            }
            None
        }
        RelayPoolNotification::Shutdown => {
            tracing::warn!("Relay pool shutdown");
            None
        }
    }
}

fn convert_event(event: &Event) -> anyhow::Result<ParsedEvent> {
    let json = event.as_json();
    ParsedEvent::from_json(&json).map_err(|e| anyhow::anyhow!("Parse error: {}", e))
}

async fn flush_batch(
    clickhouse: &ClickHouseClient,
    batch: &mut Vec<ParsedEvent>,
) -> anyhow::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    histogram!(ingestion::BATCH_SIZE).record(batch.len() as f64);
    let start = Instant::now();

    let rows: Vec<_> = batch
        .iter()
        .map(|e| funnel_clickhouse::EventRow::from_parsed(e, ""))
        .collect();

    clickhouse.insert_events(&rows).await?;

    let duration = start.elapsed();
    histogram!(ingestion::WRITE_LATENCY).record(duration.as_secs_f64());
    counter!(ingestion::EVENTS_WRITTEN).increment(batch.len() as u64);

    tracing::debug!(
        count = batch.len(),
        duration_ms = duration.as_millis(),
        "Flushed"
    );

    batch.clear();
    Ok(())
}
