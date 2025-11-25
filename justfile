# Funnel - Local CI commands
# Run `just --list` to see available commands

# Default recipe: show available commands
default:
    @just --list

# Check that the code compiles without producing binaries
check:
    cargo check --workspace --all-targets

# Run cargo fmt to format all code
fmt:
    cargo fmt --all

# Check formatting without making changes
fmt-check:
    cargo fmt --all -- --check

# Run clippy linter with strict settings
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Run tests with output shown
test-verbose:
    cargo test --workspace -- --nocapture

# Run a specific test by name
test-one NAME:
    cargo test --workspace {{NAME}} -- --nocapture

# Build the project in release mode
build:
    cargo build --workspace --release

# Build the project in debug mode
build-debug:
    cargo build --workspace

# Clean build artifacts
clean:
    cargo clean

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

# Run all CI checks (format, lint, test)
ci: fmt-check lint test

# Pre-commit hook: format, lint, and test everything
precommit: fmt lint test
    @echo "✅ All pre-commit checks passed!"

# Watch for changes and run checks (requires cargo-watch)
watch:
    cargo watch -x check -x 'clippy --workspace --all-targets'

# Update dependencies
update:
    cargo update

# Audit dependencies for security vulnerabilities (requires cargo-audit)
audit:
    cargo audit

# Show outdated dependencies (requires cargo-outdated)
outdated:
    cargo outdated --workspace

# =============================================================================
# Docker commands
# =============================================================================

# Build Docker images
docker-build:
    docker compose build

# Start all services
up:
    docker compose up -d

# Start with rebuild
up-build:
    docker compose up -d --build

# Stop all services
down:
    docker compose down

# Stop and remove volumes (WARNING: deletes data)
down-volumes:
    docker compose down -v

# View logs (all services)
logs:
    docker compose logs -f

# View logs for specific service
logs-service SERVICE:
    docker compose logs -f {{SERVICE}}

# Show service status
ps:
    docker compose ps

# Restart all services
restart:
    docker compose restart

# Restart specific service
restart-service SERVICE:
    docker compose restart {{SERVICE}}

# Show container resource usage
stats:
    docker stats --no-stream

# Execute command in API container
exec-api *ARGS:
    docker compose exec api {{ARGS}}

# Execute command in strfry container
exec-strfry *ARGS:
    docker compose exec strfry {{ARGS}}

# Pull latest images
pull:
    docker compose pull
