//! Funnel Ingestion Library
//!
//! Core components for reading Nostr events and batching them for ClickHouse insertion.

use std::time::{Duration, Instant};

use funnel_proto::ParsedEvent;

/// Configuration for the batch processor.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of events in a batch before forcing a flush.
    pub max_batch_size: usize,
    /// Maximum time to wait before flushing a non-empty batch.
    pub flush_interval: Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            flush_interval: Duration::from_millis(100),
        }
    }
}

impl BatchConfig {
    pub fn new(max_batch_size: usize, flush_interval: Duration) -> Self {
        Self {
            max_batch_size,
            flush_interval,
        }
    }
}

/// Result of checking whether a batch should be flushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushReason {
    /// Batch reached maximum size.
    BatchFull,
    /// Flush interval elapsed.
    TimeoutReached,
    /// No flush needed.
    None,
}

/// Batch processor that accumulates events and determines when to flush.
///
/// This is a pure data structure that doesn't perform I/O. The caller is responsible
/// for actually flushing the batch to storage.
#[derive(Debug)]
pub struct BatchProcessor {
    config: BatchConfig,
    batch: Vec<ParsedEvent>,
    last_flush: Instant,
}

impl BatchProcessor {
    /// Create a new batch processor with the given configuration.
    pub fn new(config: BatchConfig) -> Self {
        Self {
            batch: Vec::with_capacity(config.max_batch_size),
            config,
            last_flush: Instant::now(),
        }
    }

    /// Add an event to the batch.
    pub fn push(&mut self, event: ParsedEvent) {
        self.batch.push(event);
    }

    /// Check if the batch should be flushed.
    pub fn should_flush(&self) -> FlushReason {
        if self.batch.len() >= self.config.max_batch_size {
            FlushReason::BatchFull
        } else if !self.batch.is_empty() && self.last_flush.elapsed() >= self.config.flush_interval
        {
            FlushReason::TimeoutReached
        } else {
            FlushReason::None
        }
    }

    /// Take the current batch for flushing and reset internal state.
    ///
    /// Returns `None` if the batch is empty.
    pub fn take_batch(&mut self) -> Option<Vec<ParsedEvent>> {
        if self.batch.is_empty() {
            return None;
        }

        self.last_flush = Instant::now();
        Some(std::mem::take(&mut self.batch))
    }

    /// Force take the batch even if empty (useful for shutdown).
    pub fn take_batch_force(&mut self) -> Vec<ParsedEvent> {
        self.last_flush = Instant::now();
        std::mem::take(&mut self.batch)
    }

    /// Get the number of events currently in the batch.
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Check if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Get the oldest event's timestamp for lag calculation.
    pub fn oldest_event(&self) -> Option<&ParsedEvent> {
        self.batch.first()
    }

    /// Get the configured flush interval.
    pub fn flush_interval(&self) -> Duration {
        self.config.flush_interval
    }

    /// Time since the last flush.
    pub fn time_since_flush(&self) -> Duration {
        self.last_flush.elapsed()
    }
}

/// Parse a line from strfry stream or raw event JSON.
///
/// Returns `None` if the line cannot be parsed.
pub fn parse_line(line: &str) -> Option<ParsedEvent> {
    use funnel_proto::StrfryMessage;

    if line.is_empty() {
        return None;
    }

    // Try to parse as strfry message first, then as raw event
    if let Ok(msg) = StrfryMessage::from_json(line) {
        Some(msg.to_parsed_event())
    } else {
        ParsedEvent::from_json(line).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    fn make_test_event(id: &str, kind: u16) -> ParsedEvent {
        ParsedEvent {
            id: id.to_string(),
            pubkey: "test_pubkey".to_string(),
            created_at: chrono::Utc::now(),
            kind,
            content: "test".to_string(),
            sig: "test_sig".to_string(),
            tags: vec![],
        }
    }

    mod batch_config_tests {
        use super::*;

        #[test]
        fn default_config() {
            let config = BatchConfig::default();
            assert_eq!(config.max_batch_size, 1000);
            assert_eq!(config.flush_interval, Duration::from_millis(100));
        }

        #[test]
        fn custom_config() {
            let config = BatchConfig::new(500, Duration::from_secs(1));
            assert_eq!(config.max_batch_size, 500);
            assert_eq!(config.flush_interval, Duration::from_secs(1));
        }
    }

    mod batch_processor_tests {
        use super::*;

        #[test]
        fn new_processor_is_empty() {
            let processor = BatchProcessor::new(BatchConfig::default());
            assert!(processor.is_empty());
            assert_eq!(processor.len(), 0);
            assert!(processor.oldest_event().is_none());
        }

        #[test]
        fn push_adds_events() {
            let mut processor = BatchProcessor::new(BatchConfig::default());

            processor.push(make_test_event("1", 1));
            assert_eq!(processor.len(), 1);

            processor.push(make_test_event("2", 1));
            assert_eq!(processor.len(), 2);

            assert!(!processor.is_empty());
        }

        #[test]
        fn oldest_event_returns_first_pushed() {
            let mut processor = BatchProcessor::new(BatchConfig::default());

            processor.push(make_test_event("first", 1));
            processor.push(make_test_event("second", 1));

            let oldest = processor.oldest_event().unwrap();
            assert_eq!(oldest.id, "first");
        }

        #[test]
        fn should_flush_when_batch_full() {
            let config = BatchConfig::new(3, Duration::from_secs(60));
            let mut processor = BatchProcessor::new(config);

            processor.push(make_test_event("1", 1));
            assert_eq!(processor.should_flush(), FlushReason::None);

            processor.push(make_test_event("2", 1));
            assert_eq!(processor.should_flush(), FlushReason::None);

            processor.push(make_test_event("3", 1));
            assert_eq!(processor.should_flush(), FlushReason::BatchFull);
        }

        #[test]
        fn should_flush_when_timeout_reached() {
            let config = BatchConfig::new(1000, Duration::from_millis(10));
            let mut processor = BatchProcessor::new(config);

            processor.push(make_test_event("1", 1));
            assert_eq!(processor.should_flush(), FlushReason::None);

            // Wait for timeout
            sleep(Duration::from_millis(15));

            assert_eq!(processor.should_flush(), FlushReason::TimeoutReached);
        }

        #[test]
        fn should_not_flush_empty_batch_on_timeout() {
            let config = BatchConfig::new(1000, Duration::from_millis(10));
            let processor = BatchProcessor::new(config);

            // Wait for timeout
            sleep(Duration::from_millis(15));

            // Empty batch should not flush
            assert_eq!(processor.should_flush(), FlushReason::None);
        }

        #[test]
        fn take_batch_returns_events_and_clears() {
            let mut processor = BatchProcessor::new(BatchConfig::default());

            processor.push(make_test_event("1", 1));
            processor.push(make_test_event("2", 1));

            let batch = processor.take_batch().unwrap();
            assert_eq!(batch.len(), 2);
            assert_eq!(batch[0].id, "1");
            assert_eq!(batch[1].id, "2");

            // Processor should be empty now
            assert!(processor.is_empty());
            assert_eq!(processor.len(), 0);
        }

        #[test]
        fn take_batch_returns_none_when_empty() {
            let mut processor = BatchProcessor::new(BatchConfig::default());
            assert!(processor.take_batch().is_none());
        }

        #[test]
        fn take_batch_resets_flush_timer() {
            let config = BatchConfig::new(1000, Duration::from_millis(50));
            let mut processor = BatchProcessor::new(config);

            // Wait a bit
            sleep(Duration::from_millis(30));

            processor.push(make_test_event("1", 1));
            let _ = processor.take_batch();

            // Timer should have reset
            assert!(processor.time_since_flush() < Duration::from_millis(20));
        }

        #[test]
        fn take_batch_force_returns_empty_vec() {
            let mut processor = BatchProcessor::new(BatchConfig::default());
            let batch = processor.take_batch_force();
            assert!(batch.is_empty());
        }
    }

    mod parse_line_tests {
        use super::*;

        const VALID_EVENT_JSON: &str = r#"{"id":"4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65","pubkey":"6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93","created_at":1673347337,"kind":1,"tags":[],"content":"Test","sig":"908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"}"#;

        const STRFRY_MESSAGE_JSON: &str = r#"{"type":"EVENT","event":{"id":"4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65","pubkey":"6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93","created_at":1673347337,"kind":1,"tags":[],"content":"Test","sig":"908a15e46fb4d8675bab026fc230a0e3542bfade63da02d542fb78b2a8513fcd0092619a2c8c1221e581946e0191f2af505dfdf8657a414dbca329186f009262"}}"#;

        #[test]
        fn parses_raw_event_json() {
            let event = parse_line(VALID_EVENT_JSON).unwrap();
            assert_eq!(
                event.id,
                "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65"
            );
            assert_eq!(event.kind, 1);
        }

        #[test]
        fn parses_strfry_message() {
            let event = parse_line(STRFRY_MESSAGE_JSON).unwrap();
            assert_eq!(
                event.id,
                "4376c65d2f232afbe9b882a35baa4f6fe8667c4e684749af565f981833ed6a65"
            );
            assert_eq!(event.kind, 1);
        }

        #[test]
        fn returns_none_for_empty_line() {
            assert!(parse_line("").is_none());
        }

        #[test]
        fn returns_none_for_invalid_json() {
            assert!(parse_line("not json").is_none());
        }

        #[test]
        fn returns_none_for_incomplete_event() {
            assert!(parse_line(r#"{"id": "abc"}"#).is_none());
        }
    }
}
