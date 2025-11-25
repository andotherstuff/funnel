-- Funnel ClickHouse Schema
-- Version: 1.0
-- Description: Complete schema for storing Nostr events with video-specific analytics
--
-- Base schema adapted from proton-beam project, extended with video app features.
--
-- Usage:
--   clickhouse-client --multiquery < schema.sql

-- =============================================================================
-- DATABASE SETUP
-- =============================================================================

CREATE DATABASE IF NOT EXISTS nostr;
USE nostr;

-- =============================================================================
-- MAIN EVENTS TABLE
-- =============================================================================

-- Main events table (local storage)
-- Stores all Nostr events with optimized indexing for time-range queries
CREATE TABLE IF NOT EXISTS events_local (
    -- Event fields (NIP-01)
    id FixedString(64) COMMENT '32-byte hex event ID (SHA-256 hash)',
    pubkey FixedString(64) COMMENT '32-byte hex public key of event creator',
    created_at DateTime COMMENT 'Unix timestamp when event was created',
    kind UInt16 COMMENT 'Event kind (0-65535, see NIP-01)',
    content String CODEC(ZSTD(3)) COMMENT 'Event content (arbitrary string, format depends on kind)',
    sig FixedString(128) COMMENT '64-byte hex Schnorr signature',
    tags Array(Array(String)) COMMENT 'Nested array of tags',

    -- Metadata fields
    indexed_at DateTime DEFAULT now() COMMENT 'When this event was indexed into ClickHouse',
    relay_source String DEFAULT '' COMMENT 'Source relay URL (e.g., wss://relay.example.com)',

    -- Primary key for deduplication
    PRIMARY KEY (id),

    -- Secondary indexes for non-sorted columns
    INDEX idx_kind kind TYPE minmax GRANULARITY 4,
    INDEX idx_pubkey pubkey TYPE bloom_filter(0.01) GRANULARITY 4

) ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (created_at, kind, pubkey)
PARTITION BY toYYYYMM(created_at)
SETTINGS
    index_granularity = 8192,
    allow_nullable_key = 0
COMMENT 'Main Nostr events table with time-first sort order';

-- Projection for kind-first queries (e.g., "get all videos")
-- ClickHouse automatically uses this when filtering by kind
ALTER TABLE events_local ADD PROJECTION IF NOT EXISTS events_by_kind (
    SELECT *
    ORDER BY (kind, created_at, pubkey)
);

-- =============================================================================
-- TAG MATERIALIZED VIEW
-- =============================================================================

-- Flattened tag view for fast tag-based queries
-- Creates 1 row per tag with explicit position extraction
CREATE MATERIALIZED VIEW IF NOT EXISTS event_tags_flat
ENGINE = MergeTree()
ORDER BY (tag_name, tag_value_primary, created_at, event_id)
PARTITION BY toYYYYMM(created_at)
SETTINGS index_granularity = 8192
COMMENT 'Flattened tag view optimized for tag_name + primary_value queries'
AS SELECT
    id as event_id,
    pubkey,
    created_at,
    kind,
    arrayJoin(tags) as tag_array,
    -- Position 1: Tag type/name (e.g., "e", "p", "t", "d", "-")
    tag_array[1] as tag_name,
    -- Position 2: Primary value (event ID, pubkey, hashtag, d-tag value, etc.)
    if(length(tag_array) >= 2, tag_array[2], '') as tag_value_primary,
    -- Position 3: Usually relay hints, dimensions, or other metadata
    if(length(tag_array) >= 3, tag_array[3], '') as tag_value_position_3,
    -- Position 4: Usually markers like "root", "reply", "mention"
    if(length(tag_array) >= 4, tag_array[4], '') as tag_value_position_4,
    -- Position 5+: Additional metadata (rare)
    if(length(tag_array) >= 5, tag_array[5], '') as tag_value_position_5,
    if(length(tag_array) >= 6, tag_array[6], '') as tag_value_position_6,
    -- Full tag array for reference
    tag_array as tag_full,
    -- Value count (excluding tag name)
    length(tag_array) - 1 as tag_value_count
FROM events_local
WHERE length(tag_array) >= 1;

-- Index for kind-based tag filtering
ALTER TABLE event_tags_flat ADD INDEX IF NOT EXISTS idx_kind kind TYPE minmax GRANULARITY 4;

-- Projection for value-first queries (find all references to a specific pubkey/event)
ALTER TABLE event_tags_flat ADD PROJECTION IF NOT EXISTS tags_by_value (
    SELECT *
    ORDER BY (tag_value_primary, tag_name, created_at, event_id)
);

-- Projection for event-first queries (get all tags for a specific event)
ALTER TABLE event_tags_flat ADD PROJECTION IF NOT EXISTS tags_by_event (
    SELECT *
    ORDER BY (event_id, tag_name, created_at)
);

-- =============================================================================
-- VIDEO-SPECIFIC VIEWS (Kinds 34235, 34236)
-- =============================================================================

-- Video events view (addressable video events per NIP-71)
-- Kind 34235: Normal videos
-- Kind 34236: Short videos (Vine-style)
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
    -- Extract d-tag (unique identifier for addressable events)
    arrayElement(arrayFilter(t -> t[1] = 'd', tags), 1)[2] AS d_tag,
    -- Extract title from tags if present
    arrayElement(arrayFilter(t -> t[1] = 'title', tags), 1)[2] AS title,
    -- Extract thumbnail/thumb URL if present
    arrayElement(arrayFilter(t -> t[1] = 'thumb', tags), 1)[2] AS thumbnail,
    -- Extract video URL from imeta or url tag
    arrayElement(arrayFilter(t -> t[1] = 'url', tags), 1)[2] AS video_url
FROM events_local
WHERE kind IN (34235, 34236);

-- =============================================================================
-- ENGAGEMENT METRICS (Materialized Views)
-- =============================================================================

-- Reaction counts per event (kind 7 = reactions)
-- Uses SummingMergeTree for efficient incremental aggregation
CREATE MATERIALIZED VIEW IF NOT EXISTS reaction_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    tag_value_primary AS target_event_id,
    count() AS reaction_count
FROM event_tags_flat
WHERE kind = 7 AND tag_name = 'e'
GROUP BY target_event_id;

-- Comment/reply counts per event (kind 1 with 'e' tag = reply)
CREATE MATERIALIZED VIEW IF NOT EXISTS comment_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    tag_value_primary AS target_event_id,
    count() AS comment_count
FROM event_tags_flat
WHERE kind = 1 AND tag_name = 'e'
GROUP BY target_event_id;

-- Repost/quote counts per event (kind 6 = repost, kind 16 = generic repost)
CREATE MATERIALIZED VIEW IF NOT EXISTS repost_counts
ENGINE = SummingMergeTree()
ORDER BY (target_event_id)
AS SELECT
    tag_value_primary AS target_event_id,
    count() AS repost_count
FROM event_tags_flat
WHERE kind IN (6, 16) AND tag_name = 'e'
GROUP BY target_event_id;

-- =============================================================================
-- VIDEO ANALYTICS VIEWS
-- =============================================================================

-- Video stats aggregated view (combines all engagement metrics)
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
-- Weights recency and engagement for ranking
CREATE VIEW IF NOT EXISTS trending_videos AS
SELECT
    *,
    -- Decay factor: newer videos get boosted
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

-- Popular hashtags (by usage count in videos)
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

-- Daily active users
CREATE VIEW IF NOT EXISTS daily_active_users AS
SELECT
    toDate(created_at) AS date,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY date
ORDER BY date DESC;

-- Weekly active users
CREATE VIEW IF NOT EXISTS weekly_active_users AS
SELECT
    toMonday(created_at) AS week,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY week
ORDER BY week DESC;

-- Monthly active users
CREATE VIEW IF NOT EXISTS monthly_active_users AS
SELECT
    toStartOfMonth(created_at) AS month,
    uniq(pubkey) AS active_users,
    count() AS total_events
FROM events_local
GROUP BY month
ORDER BY month DESC;

-- User profiles (kind 0 metadata)
CREATE VIEW IF NOT EXISTS user_profiles AS
SELECT
    pubkey,
    argMax(content, created_at) AS metadata_json,
    max(created_at) AS last_updated,
    count() AS update_count
FROM events_local
WHERE kind = 0
GROUP BY pubkey;

-- Top video creators
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

-- Event statistics by kind
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

-- Tag statistics
CREATE VIEW IF NOT EXISTS tag_stats AS
SELECT
    tag_name,
    count() as occurrence_count,
    uniq(event_id) as unique_events,
    uniq(pubkey) as unique_users
FROM event_tags_flat
GROUP BY tag_name
ORDER BY occurrence_count DESC;

-- Activity by kind (daily breakdown)
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
-- VERIFICATION QUERIES
-- =============================================================================

-- Check table creation
SELECT
    database,
    name as table_name,
    engine,
    total_rows,
    formatReadableSize(total_bytes) as size
FROM system.tables
WHERE database = 'nostr'
ORDER BY name;

-- =============================================================================
-- MAINTENANCE NOTES
-- =============================================================================

/*
AFTER BULK IMPORT - Materialize projections for better query performance:

    ALTER TABLE events_local MATERIALIZE PROJECTION events_by_kind;
    ALTER TABLE event_tags_flat MATERIALIZE PROJECTION tags_by_value;
    ALTER TABLE event_tags_flat MATERIALIZE PROJECTION tags_by_event;

FORCE DEDUPLICATION (for ReplacingMergeTree):

    OPTIMIZE TABLE events_local FINAL;

DROP OLD PARTITIONS (e.g., data older than 2024):

    ALTER TABLE events_local DROP PARTITION 202312;

COMMON QUERIES:

-- Get video with stats
SELECT * FROM video_stats WHERE id = 'abc123...';

-- Search videos by hashtag
SELECT * FROM video_hashtags WHERE hashtag = 'nostr' ORDER BY created_at DESC LIMIT 100;

-- Get trending videos
SELECT * FROM trending_videos LIMIT 50;

-- Get user's videos with stats
SELECT * FROM video_stats WHERE pubkey = 'def456...' ORDER BY created_at DESC;

-- Get engagement for a specific video
SELECT
    v.*,
    r.reaction_count,
    c.comment_count
FROM videos v
LEFT JOIN reaction_counts r ON v.id = r.target_event_id
LEFT JOIN comment_counts c ON v.id = c.target_event_id
WHERE v.id = 'abc123...';
*/

