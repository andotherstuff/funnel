# Funnel

A high-throughput Nostr analytics backend for video sharing apps, built with Rust and ClickHouse.

## Overview

Funnel is the analytics and search layer for a Vine-style video sharing app built on Nostr. It ingests events from an external relay and provides:

- **Video stats** — reaction counts, comment counts, reposts
- **Search** — find videos by hashtag or content
- **Custom feeds** — trending videos, sorted by engagement, filtered by author
- **Analytics** — DAU/WAU/MAU, top creators, popular hashtags

## Architecture

```
                        ┌────────────────────┐
                        │    Nostr Clients   │
                        └─────────┬──────────┘
                                  │
                ┌─────────────────┴─────────────────┐
                │                                   │
                │ Nostr protocol                    │ HTTP
                │ (EVENT/REQ/CLOSE)                 │ (stats, search, feeds)
                ▼                                   ▼
        ┌───────────────┐                   ┌───────────────┐
        │ External Relay│                   │   REST API    │
        │               │                   │    (Rust)     │
        └───────┬───────┘                   └───────┬───────┘
                │                                   │
                │ WebSocket                         │ queries
                ▼                                   ▼
        ┌───────────────┐                   ┌───────────────┐
        │   Ingestion   │──── writes ──────▶│  ClickHouse   │
        │    Service    │                   │               │
        └───────────────┘                   └───────────────┘
```

**External Relay** handles EVENT/REQ/CLOSE, subscriptions, and primary storage.

**Ingestion** subscribes to the relay via WebSocket and streams all events to ClickHouse with batched inserts.

**ClickHouse** stores events for complex queries, aggregations, and analytics that Nostr REQ doesn't support.

**REST API** exposes video stats, search, and feeds to the app.

## Why ClickHouse?

Standard Nostr queries are great for real-time protocol operations, but we need:

1. **Aggregations** — count reactions, comments, reposts (Nostr REQ doesn't support)
2. **Custom sort orders** — trending, popular (beyond `created_at`)
3. **Full-text search** — across titles and content
4. **Analytics** — DAU/WAU/MAU, creator stats, hashtag trends
5. **Data exports** — for recommendation systems

ClickHouse excels at these analytical queries.

## API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /metrics` | Prometheus metrics |
| `GET /api/videos/{id}/stats` | Get reaction, comment, and repost counts for a video |
| `GET /api/videos?sort=recent\|trending&limit=` | List videos with custom sort |
| `GET /api/users/{pubkey}/videos?limit=` | Get videos by a specific creator |
| `GET /api/search?tag=...&q=...&limit=` | Search by hashtag or text |
| `GET /api/stats` | Total event and video counts |

All endpoints return JSON with `Cache-Control` headers.

## Quick Start

### Prerequisites

- Docker and Docker Compose
- ClickHouse instance (self-hosted or [ClickHouse Cloud](https://clickhouse.cloud))
- An external Nostr relay to ingest events from

### 1. Set up ClickHouse

Apply the schema to your ClickHouse instance:

```bash
clickhouse-client --multiquery < docs/schema.sql
```

Or for ClickHouse Cloud:

```bash
clickhouse-client \
  --host your-host.clickhouse.cloud \
  --secure \
  --user default \
  --password your-password \
  --multiquery < docs/schema.sql
```

### 2. Configure environment

```bash
cp .env.example .env
# Edit .env with your settings:
# RELAY_URL=wss://your-relay.example.com
# CLICKHOUSE_URL=https://host:8443?user=default&password=xxx
```

### 3. Start services

```bash
docker compose up -d
```

This starts:
- **API** on port 8080 (REST endpoints)
- **Prometheus** on port 9090 (metrics)
- **Ingestion** (internal, streams events from relay to ClickHouse)

### 4. Ingest events

The ingestion service operates in two modes:

#### Live Mode (default)

Automatically starts with `docker compose up` and streams new events in real-time:

```bash
docker compose up -d
docker compose logs -f ingestion
```

Live mode subscribes from the last known event timestamp (with a 2-day buffer) so it catches up on any events missed while stopped.

#### Backfill Mode (historical sync)

To import all historical events from a relay, run the backfill container:

```bash
# Run backfill (one-time, will exit when complete)
docker compose run --rm backfill
```

Backfill paginates through the entire relay history in batches of 5,000 events, walking backwards in time. Progress is logged:

```
INFO Fetching batch until=2024-01-15T10:30:00Z limit=5000 total_so_far=150000
INFO Received batch count=5000 oldest=2024-01-14T22:15:33Z
INFO Inserted batch_inserted=5000 total_events=155000
```

**Notes:**
- Backfill is safe to re-run — ClickHouse deduplicates by event ID
- Run backfill in `tmux` or `screen` for long-running syncs
- Stop early with `Ctrl+C` if needed; progress is saved to ClickHouse
- Live ingestion and backfill can run simultaneously

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `RELAY_URL` | Yes | — | WebSocket URL of the Nostr relay to ingest from |
| `CLICKHOUSE_URL` | Yes | — | ClickHouse server URL (e.g., `https://host:8443`) |
| `CLICKHOUSE_USER` | No | `default` | ClickHouse username |
| `CLICKHOUSE_PASSWORD` | Yes | — | ClickHouse password |
| `CLICKHOUSE_DATABASE` | No | `nostr` | ClickHouse database name |
| `BATCH_SIZE` | No | `1000` | Events per insert batch |
| `BACKFILL` | No | — | Set to `1` to run in backfill mode |
| `RUST_LOG` | No | `info` | Log level (`debug`, `info`, `warn`, `error`) |

### Example `.env`

```bash
RELAY_URL=wss://relay.example.com
CLICKHOUSE_URL=https://your-instance.clickhouse.cloud:8443
CLICKHOUSE_USER=default
CLICKHOUSE_PASSWORD=your-password
CLICKHOUSE_DATABASE=nostr
```

## Development

### Build

```bash
cargo build --release
```

### Run tests

```bash
cargo test
```

### Run locally

```bash
# Start prometheus for metrics
docker compose up -d prometheus

# Run ingestion service
RELAY_URL=wss://relay.example.com \
CLICKHOUSE_URL=http://localhost:8123 \
cargo run --bin funnel-ingestion

# Run API server
CLICKHOUSE_URL=http://localhost:8123 \
cargo run --bin funnel-api
```

### Useful commands (via justfile)

```bash
just build       # Build all crates
just test        # Run tests
just fmt         # Format code
just lint        # Run clippy
just up          # Start all services
just down        # Stop all services
```

## Project Structure

```
crates/
├── proto/        # Nostr types, video event parsing
├── clickhouse/   # ClickHouse client and queries
├── ingestion/    # WebSocket subscriber, batch processor
├── api/          # Axum REST API
└── observability/# Tracing and metrics

docs/
├── plan.md       # Implementation roadmap
├── schema.sql    # ClickHouse schema
└── deployment.md # Production deployment guide

config/
└── prometheus.yml# Prometheus scrape config
```

## Video Events

Funnel indexes video events per [NIP-71](https://github.com/nostr-protocol/nips/blob/master/71.md):

- **Kind 34235** — Normal videos
- **Kind 34236** — Short videos (vertical format)

Both are addressable/replaceable events identified by the `d` tag.

## Documentation

- [`docs/plan.md`](docs/plan.md) — Implementation plan and architecture
- [`docs/schema.sql`](docs/schema.sql) — ClickHouse schema with all tables and views
- [`docs/deployment.md`](docs/deployment.md) — Production deployment with Ansible

## License

[MIT](LICENSE)
