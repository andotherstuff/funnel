# Funnel Deployment with Ansible

Automated server setup and deployment for Funnel.

## Prerequisites

1. **Install Ansible** (on your local machine):
   ```bash
   # macOS
   brew install ansible

   # Ubuntu/Debian
   sudo apt install ansible

   # pip
   pip install ansible
   ```

2. **Install required collections**:
   ```bash
   cd deploy
   ansible-galaxy install -r requirements.yml
   ```

3. **Create config files from examples** (these are gitignored):
   ```bash
   cp inventory/production.yml.example inventory/production.yml
   cp inventory/staging.yml.example inventory/staging.yml
   cp group_vars/all.yml.example group_vars/all.yml
   ```

4. **Configure your values**:
   - Edit `inventory/production.yml` with your server IP
   - Edit `group_vars/all.yml` with your SSH keys and domains

## Usage

### Initial Server Setup

Run once on a fresh server (as root):

```bash
cd deploy

# Test connection first
ansible all -m ping

# Run full setup
ansible-playbook playbooks/setup.yml
```

This will:
- Install base packages (curl, git, htop, etc.)
- Create `deploy` user with sudo access
- Configure SSH keys and harden SSH
- Set up UFW firewall
- Install Docker and Docker Compose
- Install and configure Caddy

### Setup ClickHouse Schema

Run once to create tables and views (idempotent - safe to re-run):

```bash
ansible-playbook playbooks/schema.yml
```

This reads `CLICKHOUSE_URL` from the `.env` file on the server.

### Deploy Application

After initial setup, deploy the app:

```bash
# Update inventory to use deploy user (not root)
# Then run:
ansible-playbook playbooks/deploy.yml
```

### Common Commands

```bash
# Check what would change (dry run)
ansible-playbook playbooks/setup.yml --check --diff

# Run specific role only
ansible-playbook playbooks/setup.yml --tags docker

# Run on specific host
ansible-playbook playbooks/setup.yml -l funnel-prod

# Use staging inventory
ansible-playbook -i inventory/staging.yml playbooks/setup.yml
```

## Directory Structure

```
deploy/
├── ansible.cfg                      # Ansible configuration
├── requirements.yml                 # Galaxy collections
├── inventory/
│   ├── production.yml.example       # Template (committed)
│   ├── production.yml               # Your config (gitignored)
│   ├── staging.yml.example          # Template (committed)
│   └── staging.yml                  # Your config (gitignored)
├── group_vars/
│   ├── all.yml.example              # Template (committed)
│   └── all.yml                      # Your config (gitignored)
├── playbooks/
│   ├── setup.yml                    # Initial server setup
│   └── deploy.yml                   # Application deployment
└── roles/
    ├── base/                        # Base packages, sysctl
    ├── users/                       # Deploy user, SSH
    ├── firewall/                    # UFW configuration
    ├── docker/                      # Docker CE installation
    └── caddy/                       # Caddy reverse proxy
```

> **Note:** Files with your actual IPs, domains, and SSH keys are gitignored.
> Only `.example` templates are committed to the repo.

## Configuration

### SSH Keys

Add your SSH public keys to `group_vars/all.yml`:

```yaml
deploy_ssh_keys:
  - "ssh-ed25519 AAAAC3... jeff@laptop"
  - "ssh-ed25519 AAAAC3... ci-deploy-key"
```

### Domains

Configure your domains in `group_vars/all.yml`:

```yaml
domain_base: funnel.example.com
domain_relay: "relay.{{ domain_base }}"
domain_api: "api.{{ domain_base }}"
domain_grafana: "grafana.{{ domain_base }}"
```

### Firewall Ports

Default open ports:
- 22 (SSH)
- 80 (HTTP)
- 443 (HTTPS)
- 7777 (strfry WebSocket)

Modify in `group_vars/all.yml` if needed.

## After Setup

1. SSH to server: `ssh deploy@YOUR_SERVER_IP`
2. Clone repo: `git clone <repo> ~/funnel`
3. Configure: `cp .env.example .env && vim .env`
4. Start: `docker compose up -d`
5. Verify: `docker compose ps`

## Troubleshooting

### Connection refused
```bash
# Check if server is reachable
ansible all -m ping -vvv
```

### Permission denied
```bash
# Make sure your SSH key is added
ssh-add ~/.ssh/id_ed25519

# Or specify key explicitly
ansible-playbook playbooks/setup.yml --private-key ~/.ssh/your_key
```

### Firewall locked you out
Contact your hosting provider to access console/KVM and fix UFW rules.
