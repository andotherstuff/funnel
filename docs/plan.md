# Funnel Relay Implementation Plan

High-throughput Nostr relay backend supporting a Vine-style video sharing app.

## Context

- **Current state**: Nostrify DB (TypeScript/Postgres) struggling with throughput at scale, doesn't have search and complex queries
- **Write volume**: Several thousand events/sec
- **Read volume**: 10-100k reads/sec (most complex queries via custom API, not Nostr REQ)
- **Video events**: Kinds 34235 (normal) and 34236 (short) - addressable/replaceable per [NIP-71](https://github.com/nostr-protocol/nips/blob/master/71.md)
- **Video storage**: Metadata in Nostr events, video files on Blossom servers
- **Migration**: ~millions of events from existing Nostrify instance

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Nostr Clients                            │
└───────────────┬─────────────────────────────────┬───────────────┘
                │                                 │
                │ Nostr protocol                  │ HTTP
                │ (EVENT/REQ/CLOSE)               │ (stats, search, feeds)
                │                                 │
                ▼                                 │
       ┌─────────────────┐                        │
       │     strfry      │                        │
       │    (single)     │                        │
       │     + LMDB      │                        │
       └────────┬────────┘                        │
                │                                 │
                │ WebSocket subscription          │
                │ (Nostr REQ/EVENT)               │
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
       │  • /api/videos/{id}/stats  • /api/search        │
       │  • /api/videos             • /api/stats         │
       │  • /api/users/{pubkey}/videos                   │
       └─────────────────────────────────────────────────┘
```

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **strfry for Nostr protocol** | Battle-tested, LMDB is fast, handles subscriptions efficiently. Don't reinvent the relay. |
| **Single strfry to start** | Simpler ops. Scale out with negentropy sync later if needed. |
| **WebSocket subscription for ingestion** | Standard Nostr protocol, supports catch-up sync with `since` filter, reconnection handling. |
| **ClickHouse for analytics** | Excellent for aggregations, search, and custom sort orders. Not in the hot path for Nostr protocol. |
| **Docker deployment** | Simpler ops, easy to scale later. |
| **Gorse deferred to Phase 2** | Get core analytics working first. |

## Crate Layout

| Crate | Purpose | Status |
|-------|---------|--------|
| `crates/proto` | Shared Nostr types, video event parsing (kinds 34235/34236), wrap `nostr` crate | ✅ Complete |
| `crates/ingestion` | WebSocket subscription to strfry, batches writes to ClickHouse | ✅ Complete |
| `crates/clickhouse` | ClickHouse client wrapper, query builders, connection pooling | ✅ Complete |
| `crates/api` | Axum HTTP server for custom endpoints | ✅ Complete |
| `crates/observability` | Prometheus metrics, tracing setup | ✅ Complete |

## ClickHouse Schema

Full schema is in [`docs/schema.sql`](./schema.sql). Key components:

| Table/View | Purpose |
|------------|---------|
| `events_local` | Main events table with `ReplacingMergeTree` for dedup, `FixedString` for id/pubkey, ZSTD compression on content |
| `event_tags_flat` | Materialized view denormalizing tags for fast filtering by tag name/value |
| `videos` | View filtering for kinds 34235/34236 with extracted d-tag, title, thumbnail |
| `reaction_counts` | Materialized view aggregating kind 7 reactions per target event |
| `comment_counts` | Materialized view aggregating kind 1 replies per target event |
| `repost_counts` | Materialized view aggregating kind 6/16 reposts per target event |
| `video_stats` | Combined view joining videos with engagement metrics |
| `trending_videos` | Videos ranked by time-decayed engagement score |
| `video_hashtags` | Videos indexed by hashtag for search |
| `daily/weekly/monthly_active_users` | User activity analytics |
| `top_video_creators` | Creator leaderboard by video count and engagement |

Schema features:
- **Monthly partitioning** by `created_at` for efficient time-range queries and data retention
- **Projections** for alternate sort orders (kind-first, value-first) without duplicating data
- **Bloom filter index** on pubkey for fast author lookups
- **Native `Array(Array(String))`** for tags (no JSON parsing at query time)
- **ClickHouse Cloud compatible** with notes on SharedMergeTree limitations

## REST API Endpoints

| Endpoint | Description | Status |
|----------|-------------|--------|
| `GET /health` | Health check | ✅ |
| `GET /metrics` | Prometheus metrics | ✅ |
| `GET /api/videos/{id}/stats` | Reaction + comment + repost counts | ✅ |
| `GET /api/videos?sort=recent\|trending&kind=&limit=` | Video feed with custom sort | ✅ |
| `GET /api/users/{pubkey}/videos?limit=` | Videos by author | ✅ |
| `GET /api/search?tag=...&q=...&limit=` | Hashtag or full-text search | ✅ |
| `GET /api/stats` | Total events and video counts | ✅ |

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
- `api_requests_total` (counter, by endpoint)
- `api_clickhouse_query_duration_seconds` (histogram, by endpoint)

**Infrastructure:**
- Standard node_exporter for host metrics
- ClickHouse's built-in Prometheus endpoint

Grafana dashboards for: ingestion pipeline health, API latency/throughput, ClickHouse query performance.

## Docker Compose Services

| Service | Description |
|---------|-------------|
| `strfry` | Nostr relay with LMDB storage |
| `ingestion` | Subscribes to strfry via WebSocket, writes to ClickHouse |
| `api` | REST API for video stats, search, and feeds |
| `prometheus` | Metrics collection |
| `grafana` | Dashboards and alerting |

ClickHouse is expected to be external (self-hosted or ClickHouse Cloud). Configure via `CLICKHOUSE_URL` environment variable.

---

## Phase 0 – Migration & Validation ✅ COMPLETE

**Goal:** Replace Nostrify with strfry, validate throughput improvement.

1. ✅ Deploy strfry (single instance) with Docker
2. ✅ Export events from Nostrify as JSONL
3. ✅ Import into strfry: `cat events.jsonl | strfry import`
4. ✅ Point clients at strfry, verify functionality
5. ✅ Measure: connection count, write latency, REQ query latency
6. ✅ Set up basic Prometheus + Grafana for strfry host metrics

---

## Phase 1 – Analytics Pipeline ✅ COMPLETE

**Goal:** ClickHouse ingestion + REST API for custom queries.

1. ✅ Deploy ClickHouse
2. ✅ Apply schema from `docs/schema.sql`
3. ✅ Implement `crates/proto`:
   - ParsedEvent type for ClickHouse insertion
   - VideoMeta extraction from video events
   - StrfryMessage parsing (for optional strfry stream input)
4. ✅ Implement `crates/clickhouse`:
   - Client with connection pooling and auth
   - Event insertion with async inserts
   - Video queries (stats, by author, trending, recent)
   - Search queries (hashtag, full-text)
   - Latest timestamp for catch-up sync
5. ✅ Implement `crates/ingestion`:
   - WebSocket subscription to strfry relay
   - Catch-up sync with `since` filter (2-day buffer for backdated events)
   - Batched writes (1000 events or 100ms, configurable)
   - Automatic reconnection on disconnect
   - Prometheus metrics
6. ✅ Implement `crates/api`:
   - `/api/videos/{id}/stats` (reaction/comment/repost counts)
   - `/api/search` (hashtag and content search)
   - `/api/videos` (feed with recent/trending sort)
   - `/api/users/{pubkey}/videos`
   - `/api/stats` (total counts)
   - Cache-Control headers
   - Prometheus metrics
7. ✅ Implement `crates/observability`:
   - Tracing setup (JSON for production, pretty for dev)
   - Prometheus metrics exporter
8. ✅ Docker Compose setup with health checks
9. ⏳ Build Grafana dashboards for full pipeline
10. ⏳ Load test: verify throughput targets

---

## Phase 2 – Recommendations & Scale (NEXT)

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

---

## Resolved Design Questions

| Question | Resolution |
|----------|------------|
| ClickHouse as primary storage? | No - strfry/LMDB for Nostr protocol, ClickHouse for analytics only |
| NATS from start? | No - direct WebSocket to strfry for simplicity |
| strfry stream vs WebSocket? | WebSocket subscription - supports catch-up sync and standard Nostr protocol |
| Multi-node strfry? | Start single, scale out with negentropy when needed |
| Gorse timing? | Phase 2 |

## Future Considerations

- **Replaceable event handling**: Kinds 34235/34236 are addressable. ClickHouse's `ReplacingMergeTree` handles updates, but need to ensure materialized views update correctly.
- **Delete semantics**: NIP-09 deletion events - need to handle in both strfry (native support) and ClickHouse (mark deleted or actually remove).
- **Rate limiting**: Currently not addressed. Could add at strfry (write policy plugin) or API layer.
- **Geographic distribution**: Multiple strfry instances in different regions with negentropy sync.
