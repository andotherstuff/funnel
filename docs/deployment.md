# Funnel Deployment Guide

Complete guide to deploying Funnel on a bare metal server with ClickHouse Cloud.

## Architecture Overview

```
┌─────────────────────────────────────────────────┐
│              Bare Metal Server                   │
│           (OVH Vint Hill, VA)                    │
│                                                  │
│   ┌──────────────┐      ┌──────────┐            │
│   │ External     │─────▶│ ingestion│────────┐   │
│   │ Relay        │      │(internal)│        │   │       ┌──────────────┐
│   └──────────────┘      └──────────┘        │   │       │  ClickHouse  │
│                                             │   │       │    Cloud     │
│   ┌──────────┐          ┌──────────┐        ├───┼──────▶│ (GCP us-east)│
│   │  caddy   │◀─────────│   api    │────────┘   │       └──────────────┘
│   │ :80/:443 │          │  :8080   │            │
│   └──────────┘          └──────────┘            │
│                                                  │
│                         ┌──────────┐            │
│                         │prometheus│            │
│                         │  :9090   │            │
│                         └──────────┘            │
└─────────────────────────────────────────────────┘

Public (via Caddy):           Internal only:
• api.x.com    → API          • ingestion (relay → ClickHouse)
                              • prometheus (metrics collection)
```

## Server Requirements

| Component | Specification |
|-----------|---------------|
| CPU | AMD EPYC 6-8 cores |
| RAM | 64GB |
| Storage | 2TB+ NVMe SSD |
| Network | 1Gbps+ |
| Provider | OVH Bare Metal (Vint Hill, VA recommended for US East) |

## Prerequisites

### Local Machine Setup

Install Ansible on your local machine:

```bash
# macOS
brew install ansible

# Ubuntu/Debian
sudo apt install ansible

# pip
pip install ansible
```

Install required Ansible collections:

```bash
cd deploy
ansible-galaxy install -r requirements.yml
```

### Configure Ansible

1. **Set your server IP** in `deploy/inventory/production.yml`:

```yaml
all:
  hosts:
    funnel-prod:
      ansible_host: 51.xx.xx.xx  # Your server IP
      ansible_user: root
```

2. **Add your SSH keys** in `deploy/group_vars/all.yml`:

```yaml
deploy_ssh_keys:
  - "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5... you@laptop"
```

Get your public key with: `cat ~/.ssh/id_ed25519.pub`

3. **Set your domains** in `deploy/group_vars/all.yml`:

```yaml
domain_base: yourdomain.com
domain_api: "api.{{ domain_base }}"
```

## Step 1: Server Setup with Ansible

Run the setup playbook from your local machine:

```bash
cd deploy

# Test connection first
ansible all -m ping

# Run full server setup
ansible-playbook playbooks/setup.yml
```

This automatically:
- Updates packages and installs essentials (curl, git, htop, fail2ban, etc.)
- Creates `deploy` user with sudo access
- Configures SSH keys and hardens SSH (disables password auth)
- Sets up UFW firewall (ports 22, 80, 443)
- Installs Docker and Docker Compose
- Installs Caddy and deploys the Caddyfile

### After Setup

Update your inventory to use the deploy user:

```yaml
# deploy/inventory/production.yml
all:
  hosts:
    funnel-prod:
      ansible_host: 51.xx.xx.xx
      ansible_user: deploy  # Changed from root
```

## Step 2: Set Up ClickHouse Cloud

ClickHouse Cloud must be configured separately (not managed by Ansible).

### Create Database and Schema

```bash
# Option 1: Using clickhouse-client
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

## Step 3: Deploy Application

SSH into the server and clone the repo:

```bash
ssh deploy@YOUR_SERVER_IP

# Clone repository
git clone https://github.com/your-org/funnel.git ~/funnel
cd ~/funnel

# Create environment file
cp .env.example .env
```

### Configure Environment Variables

Edit `.env` with your ClickHouse Cloud credentials:

```bash
# .env
RELAY_URL=wss://your-relay.example.com
CLICKHOUSE_URL=https://your-instance.us-east1.gcp.clickhouse.cloud:8443?user=default&password=YOUR_PASSWORD
CLICKHOUSE_DATABASE=nostr
```

### Build and Start Services

```bash
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
# Check API health
curl http://localhost:8080/health

# Check Prometheus
curl http://localhost:9090/-/healthy
```

## Step 4: Configure DNS

Point your domain to the server IP:

| Record | Type | Value |
|--------|------|-------|
| `api.yourdomain.com` | A | YOUR_SERVER_IP |

Caddy will automatically obtain Let's Encrypt certificates.

## Step 5: Configure Monitoring

Prometheus is included for metrics collection. Connect it to your existing Grafana instance:

1. In your Grafana instance, add a new Prometheus data source
2. Set the URL to `http://YOUR_SERVER_IP:9090` (or use a private network)
3. Import dashboards or create custom ones

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `ingestion_events_received_total` | Events from relay | Rate drop |
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
ssh deploy@YOUR_SERVER_IP
cd ~/funnel
git pull
docker compose build
docker compose up -d
```

Or re-run Ansible if server config changed:

```bash
# From local machine
cd deploy
ansible-playbook playbooks/setup.yml
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

### Ansible Connection Issues

```bash
# Test connection
ansible all -m ping -vvv

# Make sure SSH key is loaded
ssh-add ~/.ssh/id_ed25519

# Specify key explicitly
ansible-playbook playbooks/setup.yml --private-key ~/.ssh/your_key
```

### Ingestion Not Receiving Events

```bash
# Check ingestion logs
docker compose logs ingestion

# Verify relay URL is correct
echo $RELAY_URL
```

### API Returns 500 Errors

```bash
# Check ClickHouse connectivity
docker compose exec api curl -s "$CLICKHOUSE_URL/?query=SELECT%201"

# Check API logs
docker compose logs api
```

### Firewall Locked You Out

Contact your hosting provider to access console/KVM and fix UFW rules.

## Ansible Reference

### Directory Structure

```
deploy/
├── ansible.cfg              # Ansible configuration
├── requirements.yml         # Galaxy collections
├── inventory/
│   ├── production.yml       # Production servers
│   └── staging.yml          # Staging servers
├── group_vars/
│   └── all.yml              # Shared variables (SSH keys, domains)
├── playbooks/
│   ├── setup.yml            # Initial server setup
│   └── deploy.yml           # Application deployment
└── roles/
    ├── base/                # Packages, timezone, sysctl
    ├── users/               # Deploy user, SSH hardening
    ├── firewall/            # UFW configuration
    ├── docker/              # Docker CE installation
    └── caddy/               # Caddy reverse proxy
```

### Common Commands

```bash
# Dry run (check what would change)
ansible-playbook playbooks/setup.yml --check --diff

# Run specific role only
ansible-playbook playbooks/setup.yml --tags docker

# Use staging inventory
ansible-playbook -i inventory/staging.yml playbooks/setup.yml
```

---

## Open Questions / TODOs

- [ ] Finalize ClickHouse Cloud URLs (staging vs production)
- [ ] Configure CORS restrictions for API
- [ ] Decide on API authentication strategy
- [ ] Set up alerting (PagerDuty/Slack integration)
- [ ] Configure log rotation
- [ ] Set up automated backups to S3/GCS
