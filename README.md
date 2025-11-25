# Funnel

A high-throughput Nostr relay backend for video sharing apps.

## What is this?

Funnel is the analytics and search layer for a Vine-style video sharing app built on Nostr. It sits alongside [strfry](https://github.com/hoytech/strfry) (which handles the core Nostr protocol) and provides:

- **Video stats** — reaction counts, comment counts, reposts
- **Search** — find videos by hashtag or content
- **Custom feeds** — trending videos, sorted by engagement, filtered by author
- **Analytics** — DAU/WAU/MAU, top creators, popular hashtags

## Architecture

```
Nostr Clients ──┬── Nostr protocol ──▶ strfry (LMDB)
                │                           │
                │                           │ stream
                │                           ▼
                │                      Ingestion Service
                │                           │
                │                           │ batch insert
                │                           ▼
                └── HTTP ──────────▶ REST API ◀── ClickHouse
```

- **strfry** handles EVENT/REQ/CLOSE, subscriptions, and primary storage
- **ClickHouse** stores events for complex queries and aggregations
- **REST API** exposes stats, search, and feeds to the app

## Why not just use strfry?

strfry is great for standard Nostr queries, but we need:

1. Aggregations (count reactions, comments) that Nostr REQ doesn't support
2. Custom sort orders (trending, popular) beyond `created_at`
3. Full-text search across content and hashtags
4. Data exports for recommendation systems

ClickHouse excels at these analytical queries while strfry handles the real-time Nostr protocol.

## Status

🚧 **Under development** — see [`docs/plan.md`](docs/plan.md) for the implementation roadmap.

## Documentation

- [`docs/plan.md`](docs/plan.md) — Implementation plan and architecture
- [`docs/schema.sql`](docs/schema.sql) — ClickHouse schema

## License

[MIT](LICENSE)

