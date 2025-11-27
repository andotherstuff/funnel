# Funnel API Documentation

The Funnel API provides REST endpoints for querying video statistics, listings, and search functionality for Nostr video content.

## Base URL

The API is typically served at `http://localhost:8080` in development or your configured domain in production.

## Authentication

All `/api/*` endpoints require bearer token authentication when `API_TOKEN` is configured.

### Request Header

```
Authorization: Bearer <your-api-token>
```

### Example

```bash
curl -H "Authorization: Bearer your-secret-token" \
  https://api.example.com/api/videos
```

### Error Response (401 Unauthorized)

When authentication fails, the API returns:

```json
{
  "error": "Missing authorization header"
}
```

or

```json
{
  "error": "Invalid token"
}
```

### Public Endpoints

The following endpoints do **not** require authentication:
- `GET /health` - Health check
- `GET /metrics` - Prometheus metrics

---

## Endpoints

### Health Check

Check if the API is running.

```
GET /health
```

#### Response

```json
{
  "status": "ok"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Always `"ok"` if the service is running |

#### Headers

- `Cache-Control: no-store`

---

### Prometheus Metrics

Returns Prometheus-formatted metrics for monitoring.

```
GET /metrics
```

#### Response

Returns plain text in Prometheus exposition format.

#### Headers

- `Cache-Control: no-store`

---

### List Videos

Returns a list of videos with optional sorting and filtering.

```
GET /api/videos
```

#### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `sort` | string | No | `recent` | Sort order: `recent`, `popular`, or `trending` |
| `kind` | integer | No | - | Filter by Nostr event kind (e.g., `34235` for video, `34236` for short video) |
| `limit` | integer | No | `50` | Maximum number of results (max: 100) |

#### Response (sort=recent)

```json
[
  {
    "id": "abc123...",
    "pubkey": "npub1...",
    "created_at": "2024-01-15T10:30:00Z",
    "kind": 34235,
    "d_tag": "my-video-slug",
    "title": "My Video Title",
    "thumbnail": "https://example.com/thumb.jpg",
    "reactions": 42,
    "comments": 15,
    "reposts": 5,
    "engagement_score": 92,
    "trending_score": 0.0
  }
]
```

#### Response (sort=trending or sort=popular)

```json
[
  {
    "id": "abc123...",
    "pubkey": "npub1...",
    "created_at": "2024-01-15T10:30:00Z",
    "kind": 34235,
    "d_tag": "my-video-slug",
    "title": "My Video Title",
    "thumbnail": "https://example.com/thumb.jpg",
    "reactions": 42,
    "comments": 15,
    "reposts": 5,
    "engagement_score": 92,
    "trending_score": 156.7
  }
]
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Nostr event ID (hex) |
| `pubkey` | string | Author's public key (hex) |
| `created_at` | string | ISO 8601 timestamp |
| `kind` | integer | Nostr event kind |
| `d_tag` | string | Unique identifier for addressable events |
| `title` | string | Video title |
| `thumbnail` | string | Thumbnail URL |
| `reactions` | integer | Total reaction count |
| `comments` | integer | Total comment count |
| `reposts` | integer | Total repost count |
| `engagement_score` | integer | Calculated engagement score |
| `trending_score` | float | Trending algorithm score (only non-zero for trending sort) |

#### Headers

- `Cache-Control: public, max-age=60`

#### Examples

```bash
# Get recent videos
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos"

# Get trending videos
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos?sort=trending&limit=20"

# Get recent short videos (kind 34236)
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos?kind=34236"
```

---

### Get Video Stats

Get detailed statistics for a specific video.

```
GET /api/videos/{id}/stats
```

#### Path Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | Nostr event ID (hex) |

#### Response (200 OK)

```json
{
  "id": "abc123...",
  "pubkey": "def456...",
  "created_at": "2024-01-15T10:30:00Z",
  "kind": 34235,
  "d_tag": "my-video-slug",
  "title": "My Video Title",
  "thumbnail": "https://example.com/thumb.jpg",
  "reactions": 42,
  "comments": 15,
  "reposts": 5,
  "engagement_score": 92
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Nostr event ID (hex) |
| `pubkey` | string | Author's public key (hex) |
| `created_at` | string | ISO 8601 timestamp |
| `kind` | integer | Nostr event kind |
| `d_tag` | string | Unique identifier for addressable events |
| `title` | string | Video title |
| `thumbnail` | string | Thumbnail URL |
| `reactions` | integer | Total reaction count |
| `comments` | integer | Total comment count |
| `reposts` | integer | Total repost count |
| `engagement_score` | integer | Calculated engagement score |

#### Headers

- Success: `Cache-Control: public, max-age=30`
- Error: `Cache-Control: no-store`

#### Error Response (404 Not Found)

```json
{
  "error": "Video not found"
}
```

#### Example

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos/abc123def456.../stats"
```

---

### Get User Videos

Get all videos published by a specific user.

```
GET /api/users/{pubkey}/videos
```

#### Path Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `pubkey` | string | User's public key (hex) |

#### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `limit` | integer | No | `50` | Maximum number of results (max: 100) |

#### Response

```json
[
  {
    "id": "abc123...",
    "pubkey": "def456...",
    "created_at": "2024-01-15T10:30:00Z",
    "kind": 34235,
    "d_tag": "my-video-slug",
    "title": "My Video Title",
    "thumbnail": "https://example.com/thumb.jpg",
    "reactions": 42,
    "comments": 15,
    "reposts": 5,
    "engagement_score": 92
  }
]
```

Returns an empty array `[]` if the user has no videos.

#### Headers

- `Cache-Control: public, max-age=60`

#### Example

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/users/def456.../videos?limit=10"
```

---

### Search Videos

Search for videos by hashtag or text query.

```
GET /api/search
```

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `tag` | string | One of `tag` or `q` required | Search by hashtag (without #) |
| `q` | string | One of `tag` or `q` required | Full-text search query |
| `limit` | integer | No | Maximum number of results (default: 50, max: 100) |

**Note:** Either `tag` or `q` must be provided. If both are provided, `tag` takes precedence.

#### Response (hashtag search)

```json
[
  {
    "event_id": "abc123...",
    "hashtag": "nostr",
    "created_at": "2024-01-15T10:30:00Z",
    "pubkey": "def456...",
    "kind": 34235,
    "title": "Video About Nostr",
    "thumbnail": "https://example.com/thumb.jpg",
    "d_tag": "nostr-video"
  }
]
```

#### Hashtag Search Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `event_id` | string | Nostr event ID (hex) |
| `hashtag` | string | Matched hashtag |
| `created_at` | string | ISO 8601 timestamp |
| `pubkey` | string | Author's public key (hex) |
| `kind` | integer | Nostr event kind |
| `title` | string | Video title |
| `thumbnail` | string | Thumbnail URL |
| `d_tag` | string | Unique identifier for addressable events |

#### Response (text search)

```json
[
  {
    "id": "abc123...",
    "pubkey": "def456...",
    "created_at": "2024-01-15T10:30:00Z",
    "kind": 34235,
    "d_tag": "my-video-slug",
    "title": "Bitcoin Tutorial",
    "thumbnail": "https://example.com/thumb.jpg",
    "reactions": 42,
    "comments": 15,
    "reposts": 5,
    "engagement_score": 92
  }
]
```

#### Headers

- Success: `Cache-Control: public, max-age=60`
- Error: `Cache-Control: no-store`

#### Error Response (400 Bad Request)

```json
{
  "error": "Search requires 'tag' or 'q' parameter"
}
```

#### Examples

```bash
# Search by hashtag
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/search?tag=bitcoin&limit=20"

# Full-text search
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/search?q=tutorial&limit=20"
```

---

### Get Global Stats

Get aggregate statistics about the system.

```
GET /api/stats
```

#### Response

```json
{
  "total_events": 150000,
  "total_videos": 5000
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `total_events` | integer | Total number of Nostr events ingested |
| `total_videos` | integer | Total number of video events indexed |

#### Headers

- `Cache-Control: public, max-age=60`

#### Example

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/stats"
```

---

## Error Handling

All endpoints return consistent error responses.

### Error Response Format

```json
{
  "error": "Error message description"
}
```

### HTTP Status Codes

| Code | Description |
|------|-------------|
| `200` | Success |
| `400` | Bad Request - Invalid parameters |
| `401` | Unauthorized - Missing or invalid authentication |
| `404` | Not Found - Resource does not exist |
| `500` | Internal Server Error - Server-side error |

### Internal Server Error (500)

```json
{
  "error": "Internal server error"
}
```

---

## Caching

The API sets appropriate `Cache-Control` headers:

| Endpoint Type | Cache-Control |
|---------------|---------------|
| Success responses (API) | `public, max-age=60` (or `max-age=30` for video stats) |
| Error responses | `no-store` |
| Health/Metrics | `no-store` |

Clients should respect these headers for optimal performance.

---

## Rate Limiting

The API does not currently implement rate limiting. For high-traffic deployments, consider using a reverse proxy (e.g., Caddy, nginx) to add rate limiting.

---

## Nostr Event Kinds

The API works with the following Nostr event kinds:

| Kind | Description |
|------|-------------|
| `34235` | Video event (long-form video) |
| `34236` | Short video event (vertical/short format) |

---

## Example Integration

### JavaScript/TypeScript

```typescript
const API_URL = 'https://api.example.com';
const API_TOKEN = 'your-secret-token';

async function fetchVideos(sort = 'recent', limit = 20) {
  const response = await fetch(
    `${API_URL}/api/videos?sort=${sort}&limit=${limit}`,
    {
      headers: {
        'Authorization': `Bearer ${API_TOKEN}`,
      },
    }
  );

  if (!response.ok) {
    throw new Error(`API error: ${response.status}`);
  }

  return response.json();
}

async function searchByHashtag(tag: string) {
  const response = await fetch(
    `${API_URL}/api/search?tag=${encodeURIComponent(tag)}`,
    {
      headers: {
        'Authorization': `Bearer ${API_TOKEN}`,
      },
    }
  );

  return response.json();
}
```

### Python

```python
import requests

API_URL = 'https://api.example.com'
API_TOKEN = 'your-secret-token'

headers = {'Authorization': f'Bearer {API_TOKEN}'}

def get_trending_videos(limit=20):
    response = requests.get(
        f'{API_URL}/api/videos',
        params={'sort': 'trending', 'limit': limit},
        headers=headers
    )
    response.raise_for_status()
    return response.json()

def get_video_stats(video_id):
    response = requests.get(
        f'{API_URL}/api/videos/{video_id}/stats',
        headers=headers
    )
    response.raise_for_status()
    return response.json()
```

### cURL

```bash
# Set your token
export TOKEN="your-secret-token"

# Get trending videos
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos?sort=trending"

# Get a specific video's stats
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/videos/abc123.../stats"

# Search by hashtag
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/search?tag=bitcoin"

# Get global stats
curl -H "Authorization: Bearer $TOKEN" \
  "https://api.example.com/api/stats"
```

---

## Token Rotation

To rotate the API token:

1. Generate a new token:
   ```bash
   openssl rand -hex 32
   ```

2. Update the `API_TOKEN` environment variable with the new value

3. Restart the API server

4. Update all clients with the new token

**Note:** There is no grace period for old tokens. Ensure all clients are updated before rotating.

