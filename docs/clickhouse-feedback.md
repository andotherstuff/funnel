# ClickHouse Schema Feedback

> **Status**: ✅ Applied in schema v2.0 (except FixedString - not compatible with Rust client)
>
> Schema files:
> - `schema_cloud.sql` — ClickHouse Cloud (no projections)
> - `schema_self_hosted.sql` — Self-hosted with projections

## 1. Events Local Table Schema

- Primary/Order by key uses ID (SHA-256 value) which leads to full table scans
- Unnecessary partitioning increases query latency
- String columns not optimized for known fixed lengths

### Recommended Changes

#### a) Remove partitions from this table

- Partitions in ClickHouse are primarily for data lifecycle management
- Your current queries scan multiple partitions before pruning granules
- This increases latency without providing benefits

#### b) Update String data types to FixedString for known-length fields

- `id`: String → FixedString(64)
- `pubkey`: String → FixedString(64)
- `sig`: String → FixedString(128)

#### c) Add materialized columns to improve Views performance

```sql
d_tag String MATERIALIZED arrayElement(arrayFilter(t -> t[1] = 'd', tags), 1)[2],
title String MATERIALIZED arrayElement(arrayFilter(t -> t[1] = 'title', tags), 1)[2],
thumbnail String MATERIALIZED arrayElement(arrayFilter(t -> t[1] = 'thumb', tags), 1)[2],
video_url String MATERIALIZED arrayElement(arrayFilter(t -> t[1] = 'url', tags), 1)[2]
```

## 2. Event Tags Flat Data Table Schema

- Remove partitions (same reasons as above)
- Move `created_at` to the end of the ORDER BY key
- Remove column `tag_array Array(String)` - we're updating the MV to improve performance

## 3. Event Tags Flat Materialized View

Replace current MV with this cleaner approach that avoids out-of-bounds errors:

```sql
CREATE MATERIALIZED VIEW IF NOT EXISTS event_tags_flat TO event_tags_flat_data
AS SELECT
    id AS event_id,
    pubkey,
    created_at,
    kind,
    tag[1] AS tag_name,
    tag[2] AS tag_value_primary,
    tag[3] AS tag_value_position_3,
    tag[4] AS tag_value_position_4,
    tag[5] AS tag_value_position_5,
    tag[6] AS tag_value_position_6,
    tag AS tag_full,
    toUInt8(length(tag) - 1) AS tag_value_count
FROM events
ARRAY JOIN tags AS tag
WHERE length(tag) >= 1;
```

## 4. Video View

Simplified view leveraging upstream changes:

```sql
CREATE VIEW videos AS
SELECT
    id, pubkey, created_at, kind, content, tags, sig, indexed_at,
    d_tag, title, thumbnail, video_url
FROM events
WHERE kind IN (34235, 34236);
```

## 5. Reaction Counts Materialized View

```sql
CREATE TABLE IF NOT EXISTS reaction_counts (
    target_event_id FixedString(64),
    reaction_count UInt64
) ENGINE = SummingMergeTree()
ORDER BY (target_event_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS reaction_counts_mv TO reaction_counts
AS SELECT
    tag[2] AS target_event_id,
    toUInt64(1) AS reaction_count
FROM events
ARRAY JOIN tags AS tag
WHERE kind = 7
    AND tag[1] = 'e'
    AND length(tag) >= 2;
```

## 6. Comment Counts Materialized View

```sql
CREATE TABLE IF NOT EXISTS comment_counts (
    target_event_id FixedString(64),
    comment_count UInt64
) ENGINE = SummingMergeTree()
ORDER BY (target_event_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS comment_counts_mv TO comment_counts
AS SELECT
    tag[2] AS target_event_id,
    toUInt64(1) AS comment_count
FROM events
ARRAY JOIN tags AS tag
WHERE kind = 1
    AND tag[1] = 'e'
    AND length(tag) >= 2;
```

## 7. Repost Counts Materialized View

```sql
CREATE TABLE IF NOT EXISTS repost_counts (
    target_event_id FixedString(64),
    repost_count UInt64
) ENGINE = SummingMergeTree()
ORDER BY (target_event_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS repost_counts_mv TO repost_counts
AS SELECT
    tag[2] AS target_event_id,
    toUInt64(1) AS repost_count
FROM events
ARRAY JOIN tags AS tag
WHERE kind IN (6, 16)
    AND tag[1] = 'e'
    AND length(tag) >= 2;
```

## 8. Video Stats View

Updated view using `ifNull` and proper aggregation for SummingMergeTree:

```sql
CREATE VIEW IF NOT EXISTS video_stats AS
SELECT
    v.id,
    v.pubkey,
    v.created_at,
    v.kind,
    v.d_tag,
    v.title,
    v.thumbnail,
    ifNull(r.reaction_count, 0) AS reactions,
    ifNull(c.comment_count, 0) AS comments,
    ifNull(rp.repost_count, 0) AS reposts,
    ifNull(r.reaction_count, 0) +
        ifNull(c.comment_count, 0) * 2 +
        ifNull(rp.repost_count, 0) * 3 AS engagement_score
FROM videos v
LEFT JOIN (
    SELECT target_event_id, sum(reaction_count) AS reaction_count
    FROM reaction_counts
    GROUP BY target_event_id
) r ON v.id = r.target_event_id
LEFT JOIN (
    SELECT target_event_id, sum(comment_count) AS comment_count
    FROM comment_counts
    GROUP BY target_event_id
) c ON v.id = c.target_event_id
LEFT JOIN (
    SELECT target_event_id, sum(repost_count) AS repost_count
    FROM repost_counts
    GROUP BY target_event_id
) rp ON v.id = rp.target_event_id;
```

> **Note:** SummingMergeTree requires explicit aggregation (`sum()`) in queries for proper merge behavior.

### Example: Why aggregation is needed

What's stored (before merge):

| target_event_id | reaction_count |
|-----------------|----------------|
| abc123          | 1              |
| abc123          | 1              |
| abc123          | 1              |

What you get with `sum()`:

| target_event_id | reaction_count |
|-----------------|----------------|
| abc123          | 3              |

## Best Practices & Key Points

### ORDER BY / Primary Key Selection

- Start with low cardinality columns, then move to higher cardinality (max 5 columns recommended)
- Select keys based on your query patterns, especially filter conditions
- Place "Selective Filters" at the beginning (e.g., `id = 1`)
- Place "Range Filters" at the end (e.g., `date >= '2025-01-01'`)

### Other Tips

- `uniq()` returns approximate unique count, use `uniqExact()` if you need exact unique count
