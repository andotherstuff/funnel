-- Funnel ClickHouse Schema
-- Version: 1.3
-- Description: Complete schema for storing Nostr events with video-specific analytics
--
-- Usage:
--   clickhouse-client --multiquery < schema.sql
--
-- For a FRESH install, first run the drop section below, then the full schema.
-- For updates, you may need to drop specific objects that changed.
--
-- NOTE: ClickHouse Cloud Limitations
-- ==================================
-- ClickHouse Cloud uses SharedMergeTree which doesn't support ALTER TABLE ADD PROJECTION.
-- The projection statements below may be skipped on ClickHouse Cloud.

-- =============================================================================
-- DATABASE SETUP
-- =============================================================================

CREATE DATABASE IF NOT EXISTS nostr;
USE nostr;

-- =============================================================================
-- DROP ALL (for fresh install - WARNING: deletes all data!)
-- =============================================================================

-- -- Views (must drop before tables they depend on)
-- DROP VIEW IF EXISTS trending_videos;
-- DROP VIEW IF EXISTS video_stats;
-- DROP VIEW IF EXISTS video_hashtags;
-- DROP VIEW IF EXISTS popular_video_hashtags;
-- DROP VIEW IF EXISTS videos;
-- DROP VIEW IF EXISTS daily_active_users;
-- DROP VIEW IF EXISTS weekly_active_users;
-- DROP VIEW IF EXISTS monthly_active_users;
-- DROP VIEW IF EXISTS user_profiles;
-- DROP VIEW IF EXISTS top_video_creators;
-- DROP VIEW IF EXISTS event_stats;
-- DROP VIEW IF EXISTS tag_stats;
-- DROP VIEW IF EXISTS activity_by_kind;

-- -- Materialized Views (DROP TABLE works for MVs)
-- DROP TABLE IF EXISTS event_tags_flat;
-- DROP TABLE IF EXISTS reaction_counts;
-- DROP TABLE IF EXISTS comment_counts;
-- DROP TABLE IF EXISTS repost_counts;

-- -- Base tables
-- DROP TABLE IF EXISTS event_tags_flat_data;
-- DROP TABLE IF EXISTS events_local;

-- =============================================================================
-- MAIN EVENTS TABLE
-- =============================================================================

-- Main events table using ReplacingMergeTree for deduplication by event id.
-- ORDER BY (id) ensures dedup works correctly.
-- Projections provide alternate sort orders for time and kind queries.
CREATE TABLE IF NOT EXISTS events_local (
    -- Event fields (NIP-01)
    id String,                    -- 32-byte hex event ID (SHA-256 hash, always 64 chars)
    pubkey String,                -- 32-byte hex public key (always 64 chars)
    created_at DateTime,          -- Unix timestamp when event was created
    kind UInt16,                  -- Event kind (0-65535, see NIP-01)
    content String CODEC(ZSTD(3)), -- Event content (arbitrary string)
    sig String,                   -- 64-byte hex Schnorr signature (always 128 chars)
    tags Array(Array(String)),    -- Nested array of tags

    -- Metadata fields
    indexed_at DateTime DEFAULT now(),
    relay_source String DEFAULT '',

    -- Secondary indexes
    INDEX idx_created_at created_at TYPE minmax GRANULARITY 4,
    INDEX idx_kind kind TYPE minmax GRANULARITY 4,
    INDEX idx_pubkey pubkey TYPE bloom_filter(0.01) GRANULARITY 4

) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (id)
PARTITION BY toYYYYMM(created_at)
SETTINGS index_granularity = 8192;

-- NOTE: Projections are not supported on ClickHouse Cloud (SharedMergeTree).
-- For self-hosted ClickHouse, you can add projections for better query performance:
--   ALTER TABLE events_local ADD PROJECTION events_by_time (SELECT * ORDER BY (created_at, kind, pubkey));
--   ALTER TABLE events_local ADD PROJECTION events_by_kind (SELECT * ORDER BY (kind, created_at, pubkey));
--   ALTER TABLE events_local ADD PROJECTION events_by_author (SELECT * ORDER BY (pubkey, created_at, kind));
--   ALTER TABLE events_local MATERIALIZE PROJECTION events_by_time;
--   ALTER TABLE events_local MATERIALIZE PROJECTION events_by_kind;
--   ALTER TABLE events_local MATERIALIZE PROJECTION events_by_author;

-- =============================================================================
-- TAG MATERIALIZED VIEW
-- =============================================================================

-- Flattened tag storage table (for advanced tag queries)
CREATE TABLE IF NOT EXISTS event_tags_flat_data (
    event_id String,
    pubkey String,
    created_at DateTime,
    kind UInt16,
    tag_array Array(String),
    tag_name String,
    tag_value_primary String,
    tag_value_position_3 String,
    tag_value_position_4 String,
    tag_value_position_5 String,
    tag_value_position_6 String,
    tag_full Array(String),
    tag_value_count UInt8,

    INDEX idx_kind kind TYPE minmax GRANULARITY 4
) ENGINE = MergeTree()
ORDER BY (tag_name, tag_value_primary, created_at, event_id)
PARTITION BY toYYYYMM(created_at)
SETTINGS index_granularity = 8192;

-- Materialized view to populate tag data
CREATE MATERIALIZED VIEW IF NOT EXISTS event_tags_flat TO event_tags_flat_data
AS SELECT
    id as event_id,
    pubkey,
    created_at,
    kind,
    arrayJoin(tags) as tag_array,
    tag_array[1] as tag_name,
    if(length(tag_array) >= 2, tag_array[2], '') as tag_value_primary,
    if(length(tag_array) >= 3, tag_array[3], '') as tag_value_position_3,
    if(length(tag_array) >= 4, tag_array[4], '') as tag_value_position_4,
    if(length(tag_array) >= 5, tag_array[5], '') as tag_value_position_5,
    if(length(tag_array) >= 6, tag_array[6], '') as tag_value_position_6,
    tag_array as tag_full,
    toUInt8(length(tag_array) - 1) as tag_value_count
FROM events_local
WHERE length(tag_array) >= 1;

-- =============================================================================
-- VIDEO-SPECIFIC VIEWS (Kinds 34235, 34236)
-- =============================================================================

-- Video events view (addressable video events per NIP-71)
CREATE VIEW IF NOT EXISTS videos AS
SELECT
    id,
    pubkey,
    created_at,
    kind,
    content,
    tags,
    sig,
    indexed_at,
    arrayElement(arrayFilter(t -> t[1] = 'd', tags), 1)[2] AS d_tag,
    arrayElement(arrayFilter(t -> t[1] = 'title', tags), 1)[2] AS title,
    arrayElement(arrayFilter(t -> t[1] = 'thumb', tags), 1)[2] AS thumbnail,
    arrayElement(arrayFilter(t -> t[1] = 'url', tags), 1)[2] AS video_url
FROM events_local
WHERE kind IN (34235, 34236);

-- =============================================================================
-- ENGAGEMENT METRICS (Materialized Views)
-- =============================================================================
-- NOTE: These MVs read directly from events_local (not from event_tags_flat)
-- because ClickHouse MVs don't chain - inserts from one MV don't trigger another.

-- Reaction counts per event (kind 7 = reactions)
-- Extracts the 'e' tag (target event reference) directly from the tags array
CREATE MATERIALIZED VIEW IF NOT EXISTS reaction_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    arrayJoin(arrayMap(t -> t[2], arrayFilter(t -> t[1] = 'e', tags))) AS target_event_id,
    count() AS reaction_count
FROM events_local
WHERE kind = 7
GROUP BY target_event_id;

-- Comment/reply counts per event (kind 1 with 'e' tag = reply)
CREATE MATERIALIZED VIEW IF NOT EXISTS comment_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    arrayJoin(arrayMap(t -> t[2], arrayFilter(t -> t[1] = 'e', tags))) AS target_event_id,
    count() AS comment_count
FROM events_local
WHERE kind = 1
GROUP BY target_event_id;

-- Repost/quote counts per event (kind 6 = repost, kind 16 = generic repost)
CREATE MATERIALIZED VIEW IF NOT EXISTS repost_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    arrayJoin(arrayMap(t -> t[2], arrayFilter(t -> t[1] = 'e', tags))) AS target_event_id,
    count() AS repost_count
FROM events_local
WHERE kind IN (6, 16)
GROUP BY target_event_id;

-- =============================================================================
-- VIDEO ANALYTICS VIEWS
-- =============================================================================

-- Video stats aggregated view
CREATE VIEW IF NOT EXISTS video_stats AS
SELECT
    v.id,
    v.pubkey,
    v.created_at,
    v.kind,
    v.d_tag,
    v.title,
    v.thumbnail,
    coalesce(r.reaction_count, 0) AS reactions,
    coalesce(c.comment_count, 0) AS comments,
    coalesce(rp.repost_count, 0) AS reposts,
    coalesce(r.reaction_count, 0) +
        coalesce(c.comment_count, 0) * 2 +
        coalesce(rp.repost_count, 0) * 3 AS engagement_score
FROM videos v
LEFT JOIN reaction_counts r ON v.id = r.target_event_id
LEFT JOIN comment_counts c ON v.id = c.target_event_id
LEFT JOIN repost_counts rp ON v.id = rp.target_event_id;

-- Trending videos (recent videos with high engagement)
CREATE VIEW IF NOT EXISTS trending_videos AS
SELECT
    *,
    engagement_score * exp(-dateDiff('hour', created_at, now()) / 24.0) AS trending_score
FROM video_stats
WHERE created_at > now() - INTERVAL 7 DAY
ORDER BY trending_score DESC;

-- Videos by hashtag
CREATE VIEW IF NOT EXISTS video_hashtags AS
SELECT
    t.event_id,
    t.tag_value_primary AS hashtag,
    t.created_at,
    t.pubkey,
    t.kind,
    v.title,
    v.thumbnail,
    v.d_tag
FROM event_tags_flat t
JOIN videos v ON t.event_id = v.id
WHERE t.tag_name = 't' AND t.kind IN (34235, 34236);

-- Popular hashtags
CREATE VIEW IF NOT EXISTS popular_video_hashtags AS
SELECT
    tag_value_primary AS hashtag,
    count() AS usage_count,
    uniq(pubkey) AS unique_creators,
    max(created_at) AS last_used
FROM event_tags_flat
WHERE tag_name = 't' AND kind IN (34235, 34236)
GROUP BY hashtag
ORDER BY usage_count DESC;

-- =============================================================================
-- USER ANALYTICS VIEWS
-- =============================================================================

CREATE VIEW IF NOT EXISTS daily_active_users AS
SELECT
    toDate(created_at) AS date,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY date
ORDER BY date DESC;

CREATE VIEW IF NOT EXISTS weekly_active_users AS
SELECT
    toMonday(created_at) AS week,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY week
ORDER BY week DESC;

CREATE VIEW IF NOT EXISTS monthly_active_users AS
SELECT
    toStartOfMonth(created_at) AS month,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY month
ORDER BY month DESC;

CREATE VIEW IF NOT EXISTS user_profiles AS
SELECT
    pubkey,
    argMax(content, created_at) AS metadata_json,
    max(created_at) AS last_updated,
    count() AS update_count
FROM events_local
WHERE kind = 0
GROUP BY pubkey;

CREATE VIEW IF NOT EXISTS top_video_creators AS
SELECT
    pubkey,
    count() AS video_count,
    countIf(kind = 34235) AS normal_videos,
    countIf(kind = 34236) AS short_videos,
    min(created_at) AS first_video,
    max(created_at) AS last_video,
    sum(engagement_score) AS total_engagement
FROM video_stats
GROUP BY pubkey
ORDER BY video_count DESC;

-- =============================================================================
-- GENERAL ANALYTICS VIEWS
-- =============================================================================

CREATE VIEW IF NOT EXISTS event_stats AS
SELECT
    toStartOfDay(created_at) as date,
    kind,
    count() as event_count,
    uniq(pubkey) as unique_authors,
    avg(length(content)) as avg_content_length
FROM events_local
GROUP BY date, kind
ORDER BY date DESC, event_count DESC;

CREATE VIEW IF NOT EXISTS tag_stats AS
SELECT
    tag_name,
    count() as occurrence_count,
    uniq(event_id) as unique_events,
    uniq(pubkey) as unique_users
FROM event_tags_flat
GROUP BY tag_name
ORDER BY occurrence_count DESC;

CREATE VIEW IF NOT EXISTS activity_by_kind AS
SELECT
    toDate(created_at) AS date,
    kind,
    count() AS events,
    uniq(pubkey) AS unique_publishers
FROM events_local
GROUP BY date, kind
ORDER BY date DESC, events DESC;

-- =============================================================================
-- VERIFICATION
-- =============================================================================

SELECT
    database,
    name as table_name,
    engine,
    total_rows,
    formatReadableSize(total_bytes) as size
FROM system.tables
WHERE database = 'nostr'
ORDER BY name;
