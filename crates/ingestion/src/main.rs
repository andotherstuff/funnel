//! Funnel Ingestion Service
//!
//! Reads Nostr events from strfry stream (JSONL) and batches them into ClickHouse.

use std::env;
use std::io::{self, BufRead};
use std::time::{Duration, Instant};

use funnel_clickhouse::ClickHouseClient;
use funnel_observability::{ingestion, init_tracing_dev};
use funnel_proto::{ParsedEvent, StrfryMessage};
use metrics::{counter, gauge, histogram};
use tokio::sync::mpsc;

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 100;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_dev();

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

    // Channel for parsed events
    let (tx, mut rx) = mpsc::channel::<ParsedEvent>(10_000);

    // Spawn stdin reader task
    let reader_handle = tokio::task::spawn_blocking(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => {
                    // Try to parse as strfry message first, then as raw event
                    let event = if let Ok(msg) = StrfryMessage::from_json(&line) {
                        Some(msg.to_parsed_event())
                    } else if let Ok(event) = ParsedEvent::from_json(&line) {
                        Some(event)
                    } else {
                        tracing::warn!(line = %line.chars().take(100).collect::<String>(), "Failed to parse line");
                        None
                    };

                    if let Some(event) = event {
                        counter!(ingestion::EVENTS_RECEIVED, "kind" => event.kind.to_string())
                            .increment(1);

                        if tx.blocking_send(event).is_err() {
                            tracing::error!("Receiver dropped, shutting down reader");
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Error reading stdin");
                    break;
                }
            }
        }
    });

    // Batch writer
    let mut batch = Vec::with_capacity(batch_size);
    let mut last_flush = Instant::now();

    loop {
        let timeout = tokio::time::timeout(flush_interval, rx.recv()).await;

        match timeout {
            Ok(Some(event)) => {
                batch.push(event);

                if batch.len() >= batch_size {
                    flush_batch(&client, &mut batch).await?;
                    last_flush = Instant::now();
                }
            }
            Ok(None) => {
                // Channel closed
                if !batch.is_empty() {
                    flush_batch(&client, &mut batch).await?;
                }
                break;
            }
            Err(_) => {
                // Timeout - flush if we have events and enough time has passed
                if !batch.is_empty() && last_flush.elapsed() >= flush_interval {
                    flush_batch(&client, &mut batch).await?;
                    last_flush = Instant::now();
                }
            }
        }

        // Update lag metric (time since oldest event in batch)
        if let Some(oldest) = batch.first() {
            let lag = chrono::Utc::now()
                .signed_duration_since(oldest.created_at)
                .num_seconds() as f64;
            gauge!(ingestion::LAG).set(lag);
        } else {
            gauge!(ingestion::LAG).set(0.0);
        }
    }

    reader_handle.await?;
    tracing::info!("Ingestion service stopped");

    Ok(())
}

async fn flush_batch(
    client: &ClickHouseClient,
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

    client.insert_events(&rows).await?;

    let duration = start.elapsed();
    histogram!(ingestion::WRITE_LATENCY).record(duration.as_secs_f64());
    counter!(ingestion::EVENTS_WRITTEN).increment(batch.len() as u64);

    tracing::debug!(
        count = batch.len(),
        duration_ms = duration.as_millis(),
        "Flushed batch"
    );

    batch.clear();
    Ok(())
}
