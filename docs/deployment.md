# Funnel Deployment Guide

Complete guide to deploying Funnel on a bare metal server with ClickHouse Cloud.

## Architecture Overview

```
┌─────────────────────────────────────────────────┐
│              Bare Metal Server                   │
│           (OVH Vint Hill, VA)                    │
│                                                  │
│   ┌──────────┐      ┌──────────┐                │
│   │  strfry  │─────▶│ ingestion│────────────┐   │
│   │  :7777   │      │(internal)│            │   │
│   └────┬─────┘      └──────────┘            │   │       ┌──────────────┐
│        │                                    │   │       │  ClickHouse  │
│   ┌────┴─────┐      ┌──────────┐            ├───┼──────▶│    Cloud     │
│   │  caddy   │◀─────│   api    │────────────┘   │       │ (GCP us-east)│
│   │ :80/:443 │      │  :8080   │                │       └──────────────┘
│   └────┬─────┘      └──────────┘                │
│        │                                        │
│        │            ┌──────────┐  ┌──────────┐  │
│        └───────────▶│ grafana  │─▶│prometheus│  │
│                     │  :3000   │  │  :9090   │  │
│                     └──────────┘  └──────────┘  │
└─────────────────────────────────────────────────┘

Public (via Caddy):           Internal only:
• relay.x.com  → strfry       • ingestion (strfry → ClickHouse)
• api.x.com    → API          • prometheus (metrics collection)
• grafana.x.com → Grafana     • grafana → prometheus
```

## Server Requirements

| Component | Specification |
|-----------|---------------|
| CPU | AMD EPYC 6-8 cores |
| RAM | 64GB |
| Storage | 2TB+ NVMe SSD |
| Network | 1Gbps+ |
| Provider | OVH Bare Metal (Vint Hill, VA recommended for US East) |

## Step 1: Initial Server Setup

SSH into your new server and run initial setup:

```bash
# Update system
apt update && apt upgrade -y

# Install essential tools
apt install -y curl git htop iotop ncdu ufw fail2ban

# Set hostname
hostnamectl set-hostname funnel-prod

# Configure timezone
timedatectl set-timezone UTC
```

### Configure Firewall

```bash
# Allow SSH
ufw allow 22/tcp

# Allow HTTP/HTTPS (for Caddy)
ufw allow 80/tcp
ufw allow 443/tcp

# Allow Nostr WebSocket (strfry)
ufw allow 7777/tcp

# Enable firewall
ufw enable
```

### Create Deploy User

```bash
# Create user
useradd -m -s /bin/bash deploy
usermod -aG sudo deploy

# Set up SSH key auth (copy your public key)
mkdir -p /home/deploy/.ssh
# Add your SSH public key to /home/deploy/.ssh/authorized_keys
chown -R deploy:deploy /home/deploy/.ssh
chmod 700 /home/deploy/.ssh
chmod 600 /home/deploy/.ssh/authorized_keys

# Switch to deploy user for remaining steps
su - deploy
```

## Step 2: Install Docker

```bash
# Install Docker
curl -fsSL https://get.docker.com | sh

# Add deploy user to docker group
sudo usermod -aG docker deploy

# Log out and back in, then verify
docker --version
docker compose version
```

## Step 3: Clone and Configure Funnel

```bash
# Clone repository
cd ~
git clone https://github.com/your-org/funnel.git
cd funnel

# Create environment file
cp .env.example .env
```

### Configure Environment Variables

Edit `.env` with your ClickHouse Cloud credentials:

```bash
# .env
CLICKHOUSE_URL=https://your-instance.us-east1.gcp.clickhouse.cloud:8443?user=default&password=YOUR_PASSWORD
CLICKHOUSE_DATABASE=nostr
```

## Step 4: Set Up ClickHouse Cloud

### Create Database and Schema

Connect to ClickHouse Cloud and run the schema:

```bash
# Option 1: Using clickhouse-client (if installed locally)
clickhouse-client \
  --host your-instance.us-east1.gcp.clickhouse.cloud \
  --port 8443 \
  --secure \
  --user default \
  --password 'YOUR_PASSWORD' \
  --multiquery < docs/schema.sql

# Option 2: Using curl
curl -X POST \
  'https://your-instance.us-east1.gcp.clickhouse.cloud:8443/?user=default&password=YOUR_PASSWORD' \
  --data-binary @docs/schema.sql
```

### Verify Schema

```bash
clickhouse-client \
  --host your-instance.us-east1.gcp.clickhouse.cloud \
  --port 8443 --secure \
  --user default --password 'YOUR_PASSWORD' \
  --query "SELECT name FROM system.tables WHERE database = 'nostr'"
```

Expected tables: `events_local`, `event_tags_flat_data`, plus views.

## Step 5: Build and Start Services

```bash
cd ~/funnel

# Build Docker images
docker compose build

# Start all services
docker compose up -d

# Check status
docker compose ps

# View logs
docker compose logs -f
```

### Verify Services

```bash
# Check strfry is accepting connections
curl -i http://localhost:7777

# Check API health
curl http://localhost:8080/health

# Check Prometheus
curl http://localhost:9090/-/healthy

# Check Grafana
curl http://localhost:3000/api/health
```

## Step 6: Set Up Reverse Proxy (Caddy)

Install Caddy for automatic HTTPS:

```bash
# Install Caddy
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install caddy
```

### Configure Caddy

```bash
sudo tee /etc/caddy/Caddyfile << 'EOF'
# Nostr relay (WebSocket)
relay.yourdomain.com {
    reverse_proxy localhost:7777
}

# REST API
api.yourdomain.com {
    reverse_proxy localhost:8080
}

# Grafana (optional, restrict access)
grafana.yourdomain.com {
    reverse_proxy localhost:3000
    # Add basic auth or IP restriction for production
}
EOF

# Reload Caddy
sudo systemctl reload caddy
```

## Step 7: Data Migration

### Import Existing Events from Another Relay

```bash
# Step 1: Import into strfry (validates signatures)
cat events.jsonl | docker compose exec -T strfry strfry import

# Step 2: Backfill to ClickHouse
docker compose exec strfry strfry export | \
  docker compose exec -T ingestion /app/funnel-ingestion
```

### Backfill Performance Tips

For large imports (millions of events):

```bash
# Increase batch size for faster backfill
docker compose exec -e BATCH_SIZE=5000 strfry \
  strfry export | docker compose exec -T ingestion /app/funnel-ingestion

# Filter by kind (videos only)
docker compose exec strfry \
  strfry export --filter '{"kinds":[34235,34236]}' | \
  docker compose exec -T ingestion /app/funnel-ingestion
```

## Step 8: Configure Monitoring

### Grafana Setup

1. Open `https://grafana.yourdomain.com`
2. Login with `admin` / `admin` (change immediately!)
3. Add Prometheus data source: `http://prometheus:9090`
4. Import dashboards or create custom ones

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `ingestion_events_received_total` | Events from strfry | Rate drop |
| `ingestion_lag_seconds` | Processing delay | > 60s |
| `api_request_duration_seconds` | API latency | p99 > 500ms |
| `api_clickhouse_query_duration_seconds` | DB query time | p99 > 200ms |

## Maintenance

### View Logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f api
docker compose logs -f ingestion
```

### Restart Services

```bash
# Restart everything
docker compose restart

# Restart specific service
docker compose restart api
```

### Update Deployment

```bash
cd ~/funnel
git pull
docker compose build
docker compose up -d
```

### Backup strfry Database

```bash
# Stop strfry briefly for consistent backup
docker compose stop strfry

# Backup LMDB (fast, just copy files)
sudo tar -czf /backup/strfry-$(date +%Y%m%d).tar.gz \
  /var/lib/docker/volumes/funnel_strfry_data/_data/

# Restart
docker compose start strfry
```

### ClickHouse Maintenance

ClickHouse Cloud handles backups automatically. For manual exports:

```bash
clickhouse-client \
  --host your-instance.clickhouse.cloud \
  --port 8443 --secure \
  --user default --password 'XXX' \
  --query "SELECT * FROM nostr.events_local FORMAT JSONEachRow" \
  > events-backup.jsonl
```

## Troubleshooting

### Ingestion Not Receiving Events

```bash
# Check strfry stream is working
docker compose exec strfry strfry stream --dir both | head -5

# Check ingestion logs
docker compose logs ingestion
```

### API Returns 500 Errors

```bash
# Check ClickHouse connectivity
docker compose exec api curl -s "$CLICKHOUSE_URL/?query=SELECT%201"

# Check API logs
docker compose logs api
```

### High Memory Usage

```bash
# Check container stats
docker stats

# strfry LMDB can use lots of RAM for caching - this is normal
# Reduce if needed by adjusting mapsize in config/strfry.conf
```

---

## Open Questions / TODOs

- [ ] Finalize ClickHouse Cloud URLs (staging vs production)
- [ ] Configure CORS restrictions for API
- [ ] Decide on API authentication strategy
- [ ] Set up alerting (PagerDuty/Slack integration)
- [ ] Configure log rotation
- [ ] Set up automated backups to S3/GCS
