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
