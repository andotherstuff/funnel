# Funnel Relay Implementation Plan

Revised plan for a high-throughput Nostr relay backend supporting a Vine-style video sharing app.

## Context

- **Current state**: Nostrify DB (TypeScript/SQLite) struggling with throughput at scale
- **Write volume**: Several thousand events/sec
- **Read volume**: 10-100k reads/sec (most complex queries via custom API, not Nostr REQ)
- **Video events**: Kinds 34235 (normal) and 34236 (short) - addressable/replaceable per [NIP-71 PR](https://github.com/nostr-protocol/nips/pull/2072)
- **Video storage**: Metadata in Nostr events, video files on Blossom servers
- **Migration**: ~millions of events from existing Nostrify instance

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Nostr Clients                            │
└───────────────┬─────────────────────────────────┬───────────────┘
                │                                 │
                │ Nostr protocol                  │ HTTP
                │ (EVENT/REQ/CLOSE)               │ (stats, search, feed)
                │                                 │
                ▼                                 │
       ┌─────────────────┐                        │
       │     strfry      │                        │
       │    (single)     │                        │
       │     + LMDB      │                        │
       └────────┬────────┘                        │
                │                                 │
                │ strfry stream (JSONL)           │
                ▼                                 │
       ┌─────────────────┐                        │
       │   Ingestion     │                        │
       │    Service      │                        │
       │     (Rust)      │                        │
       └────────┬────────┘                        │
                │                                 │
                │ batched inserts                 │
                ▼                                 │
       ┌─────────────────┐                        │
       │   ClickHouse    │                        │
       │                 │                        │
       │ • Raw events    │                        │
       │ • Materialized  │                        │
       │   views for     │                        │
       │   aggregations  │                        │
       └────────┬────────┘                        │
                │                                 │
                │                                 │
                ▼                                 ▼
       ┌─────────────────────────────────────────────────┐
       │                   REST API                      │
       │                    (Rust)                       │
       │                                                 │
       │    • /stats    • /search    • /feed            │
       └─────────────────────────────────────────────────┘
```

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **strfry for Nostr protocol** | Battle-tested, LMDB is fast, handles subscriptions efficiently. Don't reinvent the relay. |
| **Single strfry to start** | Simpler ops. Scale out with negentropy sync later if needed. |
| **Direct strfry → ingestion (no NATS)** | Fewer moving parts for single-node setup. Add NATS when we go multi-node. |
| **ClickHouse for analytics** | Excellent for aggregations, search, and custom sort orders. Not in the hot path for Nostr protocol. |
| **Docker deployment** | Simpler ops, easy to scale later. |
| **Gorse deferred to Phase 2** | Get core analytics working first. |

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `crates/proto` | Shared Nostr types, video event parsing (kinds 34235/34236), wrap `nostr` crate |
| `crates/ingestion` | Reads strfry stream, batches writes to ClickHouse |
| `crates/clickhouse` | ClickHouse client wrapper, query builders, connection pooling |
| `crates/api` | Axum HTTP server for custom endpoints |
| `crates/observability` | Prometheus metrics, tracing setup |

## ClickHouse Schema

Full schema is in [`docs/schema.sql`](./schema.sql). Key components:

| Table/View | Purpose |
|------------|---------|
| `events_local` | Main events table with `ReplacingMergeTree` for dedup, `FixedString` for id/pubkey, ZSTD compression on content |
| `event_tags_flat` | Materialized view denormalizing tags for fast filtering by tag name/value |
| `videos` | View filtering for kinds 34235/34236 with extracted d-tag, title, thumbnail |
| `reaction_counts` | Materialized view aggregating kind 7 reactions per target event |
| `comment_counts` | Materialized view aggregating kind 1 replies per target event |
| `video_stats` | Combined view joining videos with engagement metrics |
| `trending_videos` | Videos ranked by time-decayed engagement score |
| `video_hashtags` | Videos indexed by hashtag for search |

Schema features:
- **Monthly partitioning** by `created_at` for efficient time-range queries and data retention
- **Projections** for alternate sort orders (kind-first, value-first) without duplicating data
- **Bloom filter index** on pubkey for fast author lookups
- **Native `Array(Array(String))`** for tags (no JSON parsing at query time)

## REST API Endpoints (Phase 1)

| Endpoint | Description | ClickHouse Query |
|----------|-------------|------------------|
| `GET /api/videos/{id}/stats` | Reaction + comment counts | Join materialized views |
| `GET /api/search?q=...&tag=...` | Full-text / hashtag search | Query nostr_event_tags + nostr_events |
| `GET /api/videos?sort=recent\|popular` | Video feed with custom sort | Order by created_at or reaction_count |
| `GET /api/users/{pubkey}/videos` | Videos by author | Filter by pubkey, kind in (34235, 34236) |

All endpoints return JSON with `Cache-Control` headers for HTTP caching.

## Observability

Prometheus metrics exposed on `/metrics` for each service:

**Ingestion service:**
- `ingestion_events_received_total` (counter, by kind)
- `ingestion_events_written_total` (counter)
- `ingestion_batch_size` (histogram)
- `ingestion_clickhouse_write_latency_seconds` (histogram)
- `ingestion_lag_seconds` (gauge - time since oldest unbatched event)

**API service:**
- `api_requests_total` (counter, by endpoint, status)
- `api_request_duration_seconds` (histogram, by endpoint)
- `api_clickhouse_query_duration_seconds` (histogram)

**strfry (external):**
- Monitor via strfry's built-in stats or parse logs

**Infrastructure:**
- Standard node_exporter for host metrics
- ClickHouse's built-in Prometheus endpoint

Grafana dashboards for: ingestion pipeline health, API latency/throughput, ClickHouse query performance.

## Docker Compose Setup

```yaml
services:
  strfry:
    image: ghcr.io/hoytech/strfry:latest
    volumes:
      - strfry_data:/app/strfry-db
      - ./strfry.conf:/app/strfry.conf
    ports:
      - "7777:7777"

  ingestion:
    build: ./crates/ingestion
    depends_on:
      - strfry
      - clickhouse
    environment:
      - CLICKHOUSE_URL=http://clickhouse:8123
    command: >
      sh -c "strfry --config /app/strfry.conf stream --dir both |
             /app/ingestion"

  clickhouse:
    image: clickhouse/clickhouse-server:latest
    volumes:
      - clickhouse_data:/var/lib/clickhouse
    ports:
      - "8123:8123"
      - "9000:9000"

  api:
    build: ./crates/api
    depends_on:
      - clickhouse
    environment:
      - CLICKHOUSE_URL=http://clickhouse:8123
    ports:
      - "8080:8080"

  prometheus:
    image: prom/prometheus:latest
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    ports:
      - "9090:9090"

  grafana:
    image: grafana/grafana:latest
    volumes:
      - grafana_data:/var/lib/grafana
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards
    ports:
      - "3000:3000"

volumes:
  strfry_data:
  clickhouse_data:
  grafana_data:
```

## Phase 0 – Migration & Validation

**Goal:** Replace Nostrify with strfry, validate throughput improvement.

1. Deploy strfry (single instance) with Docker
2. Export events from Nostrify as JSONL
3. Import into strfry: `cat events.jsonl | strfry import`
4. Point clients at strfry, verify functionality
5. Measure: connection count, write latency, REQ query latency
6. Set up basic Prometheus + Grafana for strfry host metrics

**Exit criteria:** strfry handling production traffic, measurably faster than Nostrify.

## Phase 1 – Analytics Pipeline

**Goal:** ClickHouse ingestion + REST API for custom queries.

1. Deploy ClickHouse (hosted or Docker)
2. Apply schema from `docs/schema.sql`
3. Implement `crates/ingestion`:
   - Read JSONL from strfry stream
   - Batch events (target: 1000+ per insert, or 100ms max delay)
   - Insert to ClickHouse
   - Expose Prometheus metrics
5. Implement `crates/api`:
   - `/api/videos/{id}/stats` (reaction/comment counts)
   - `/api/search` (hashtag and content search)
   - `/api/videos` (feed with custom sort)
   - Cache-Control headers
   - Prometheus metrics
6. Build Grafana dashboards for full pipeline
7. Load test: verify throughput targets

**Exit criteria:** Custom API serving stats/search queries from ClickHouse, full observability.

## Phase 2 – Recommendations & Scale

**Goal:** Gorse integration, prepare for horizontal scaling.

1. Deploy Gorse, feed interaction data from ClickHouse
2. Add `/api/feed/recommended` endpoint
3. If strfry hits limits:
   - Add second strfry instance
   - Configure negentropy sync
   - Add Redis for ingestion dedup
   - Add NATS between strfry and ingestion (optional)
4. CDN/caching layer in front of API if needed
5. ClickHouse cluster if single node becomes bottleneck

## Open Questions (Resolved)

| Question | Resolution |
|----------|------------|
| ClickHouse as primary storage? | No - strfry/LMDB for Nostr protocol, ClickHouse for analytics only |
| NATS from start? | No - direct strfry stream to ingestion for simplicity |
| Multi-node strfry? | Start single, scale out with negentropy when needed |
| Gorse timing? | Phase 2 |

## Future Considerations

- **Replaceable event handling**: Kinds 34235/34236 are addressable. ClickHouse's `ReplacingMergeTree` handles updates, but need to ensure materialized views update correctly.
- **Delete semantics**: NIP-09 deletion events - need to handle in both strfry (native support) and ClickHouse (mark deleted or actually remove).
- **Rate limiting**: Currently not addressed. Could add at strfry (write policy plugin) or API layer.
- **Geographic distribution**: Multiple strfry instances in different regions with negentropy sync.
