# Deployment questions and notes

## General
1. How big a box do we need?
    - AMD 6-8 core, 64GB, nvme ssd at least 2TB
    - Probably use OVH in Vint Hill, VA
1. Bare Metal
1. Strfry + Ingestor + Observability + API on single machine.

## Clickhouse
1. What cloud provider and region is our hosted clickhouse in?
    - GCP - east 1 - south carolina
1. What is the URL for the staging/dev database vs production?
1. **Full-text search optimization**: Currently searching `video_stats.title` extracts title from tags on every query. If search becomes slow at scale, consider adding a materialized view with pre-extracted titles and a `tokenbf_v1` bloom filter index.

## Strfry
1. Does Strfry trigger the stream/sync plugin when you're importing events via jsonl?
   - **No.** `strfry import` does NOT trigger the stream plugin. Only WebSocket-received events trigger it.
   - Events imported via JSONL need a separate backfill step to get into ClickHouse (see below).
2. Are events validated when they're imported via jsonl?
   - **Yes.** Strfry validates event ID (hash), signature (schnorr), and JSON structure on import.
   - Invalid events are rejected. Write policy plugin also runs.

## Data Migration: Getting Events into ClickHouse

The ingestor (`funnel-ingestion`) reads JSONL from stdin and supports both:
- Strfry stream format (wrapped `{"type":"EVENT",...}` messages)
- Raw Nostr event JSON (`{"id":"...","pubkey":"...","kind":...}`)

### Normal Operation (live events)
```bash
# Strfry streams new events → ingestor → ClickHouse
strfry stream https://relay.example.com | funnel-ingestion
```

### Backfill from Strfry (existing events in LMDB)
```bash
# Export all events from strfry, pipe to ingestor
strfry export | funnel-ingestion
```

### Initial Migration from Another Relay
**Step 1:** Import events into strfry (validates & deduplicates)
```bash
cat events.jsonl | strfry import
```

**Step 2:** Export from strfry into ClickHouse
```bash
strfry export | funnel-ingestion
```

Or, if you trust the source and want to skip strfry validation:
```bash
# Direct to ingestor (raw JSONL works too)
cat events.jsonl | funnel-ingestion
```

### Backfill Performance Tips
- Default batch size is 1000 events, flush interval 100ms
- For large backfills, increase batch size:
  ```bash
  BATCH_SIZE=5000 strfry export | funnel-ingestion
  ```
- Monitor with logs: `RUST_LOG=debug` shows batch flush timing
- ClickHouse async inserts are enabled, so backfill is fast

### Filtering During Backfill
Strfry export supports filters:
```bash
# Only video events (kinds 34235, 34236)
strfry export --filter '{"kinds":[34235,34236]}' | funnel-ingestion

# Events from specific time range
strfry export --filter '{"since":1700000000,"until":1701000000}' | funnel-ingestion
```

## API
1. Need to tighten up CORS
2. Are all methods instrumented so that we can see database time vs total req time?
3. ~~TODO comments about full text search~~ ✓ Implemented with `hasTokenCaseInsensitive`
4. Do we need AUTH on the API?
5.
