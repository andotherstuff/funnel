# Funnel Implementation Plan

High-throughput Nostr analytics backend supporting a Vine-style video sharing app.

## Context

- **Current state**: Analytics and search layer for a Nostr video sharing app
- **Write volume**: Several thousand events/sec
- **Read volume**: 10-100k reads/sec (most complex queries via custom API, not Nostr REQ)
- **Video events**: Kinds 34235 (normal) and 34236 (short) - addressable/replaceable per [NIP-71](https://github.com/nostr-protocol/nips/blob/master/71.md)
- **Video storage**: Metadata in Nostr events, video files on Blossom servers
- **External relay**: Events are sourced from an external Nostr relay

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
       │  External Relay │                        │
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
| **External relay** | Use an existing Nostr relay for protocol handling. Don't reinvent the relay. |
| **WebSocket subscription for ingestion** | Standard Nostr protocol, supports catch-up sync with `since` filter, reconnection handling. |
| **ClickHouse for analytics** | Excellent for aggregations, search, and custom sort orders. Not in the hot path for Nostr protocol. |
| **Docker deployment** | Simpler ops, easy to scale later. |
| **Gorse deferred to Phase 2** | Get core analytics working first. |

## Crate Layout

| Crate | Purpose | Status |
|-------|---------|--------|
| `crates/proto` | Shared Nostr types, video event parsing (kinds 34235/34236), wrap `nostr` crate | ✅ Complete |
| `crates/ingestion` | WebSocket subscription to relay, batches writes to ClickHouse | ✅ Complete |
| `crates/clickhouse` | ClickHouse client wrapper, query builders, connection pooling | ✅ Complete |
| `crates/api` | Axum HTTP server for custom endpoints | ✅ Complete |
| `crates/observability` | Prometheus metrics, tracing setup | ✅ Complete |

## ClickHouse Schema

Full schema is available in two variants:
- [`schema_cloud.sql`](./schema_cloud.sql) — For ClickHouse Cloud
- [`schema_self_hosted.sql`](./schema_self_hosted.sql) — For self-hosted (with projections)

Key components:

| Table/View | Purpose |
|------------|---------|
| `events_local` | Main events table with `ReplacingMergeTree` for dedup, ZSTD compression on content, materialized columns for video tags |
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

Prometheus is included for metrics collection. Connect to your existing Grafana instance for dashboards.

## Docker Compose Services

| Service | Description |
|---------|-------------|
| `ingestion` | Subscribes to external relay via WebSocket, writes to ClickHouse |
| `api` | REST API for video stats, search, and feeds |
| `prometheus` | Metrics collection |

ClickHouse is expected to be external (self-hosted or ClickHouse Cloud). Configure via `CLICKHOUSE_URL` environment variable.

---

## Phase 1 – Analytics Pipeline ✅ COMPLETE

**Goal:** ClickHouse ingestion + REST API for custom queries.

1. ✅ Deploy ClickHouse
2. ✅ Apply schema from `docs/schema_cloud.sql` or `docs/schema_self_hosted.sql`
3. ✅ Implement `crates/proto`:
   - ParsedEvent type for ClickHouse insertion
   - VideoMeta extraction from video events
   - Message parsing for relay stream input
4. ✅ Implement `crates/clickhouse`:
   - Client with connection pooling and auth
   - Event insertion with async inserts
   - Video queries (stats, by author, trending, recent)
   - Search queries (hashtag, full-text)
   - Latest timestamp for catch-up sync
5. ✅ Implement `crates/ingestion`:
   - WebSocket subscription to relay
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
9. ⏳ Load test: verify throughput targets

---

## Phase 2 – Recommendations & Scale (NEXT)

**Goal:** Gorse integration, prepare for horizontal scaling.

1. Deploy Gorse, feed interaction data from ClickHouse
2. Add `/api/feed/recommended` endpoint
3. If relay hits limits:
   - Consider running own relay instance
   - Add Redis for ingestion dedup
   - Add NATS between relay and ingestion (optional)
4. CDN/caching layer in front of API if needed
5. ClickHouse cluster if single node becomes bottleneck

---

## Resolved Design Questions

| Question | Resolution |
|----------|------------|
| ClickHouse as primary storage? | No - external relay for Nostr protocol, ClickHouse for analytics only |
| NATS from start? | No - direct WebSocket to relay for simplicity |
| Gorse timing? | Phase 2 |

## Future Considerations

- **Replaceable event handling**: Kinds 34235/34236 are addressable. ClickHouse's `ReplacingMergeTree` handles updates, but need to ensure materialized views update correctly.
- **Delete semantics**: NIP-09 deletion events - need to handle in relay (native support) and ClickHouse (mark deleted or actually remove).
- **Rate limiting**: Currently not addressed. Could add at relay (write policy plugin) or API layer.
- **Geographic distribution**: Multiple relay instances in different regions with negentropy sync.
