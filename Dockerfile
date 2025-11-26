# =============================================================================
# Multi-stage Dockerfile for Funnel services
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build Rust binaries (Alpine/musl for compatibility with strfry)
# -----------------------------------------------------------------------------
FROM rust:1.83-alpine AS builder

WORKDIR /app

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconf \
    openssl-dev \
    openssl-libs-static

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/proto/Cargo.toml crates/proto/
COPY crates/clickhouse/Cargo.toml crates/clickhouse/
COPY crates/ingestion/Cargo.toml crates/ingestion/
COPY crates/api/Cargo.toml crates/api/
COPY crates/observability/Cargo.toml crates/observability/

# Create dummy source files for dependency caching
RUN mkdir -p crates/proto/src crates/clickhouse/src crates/ingestion/src crates/api/src crates/observability/src \
    && echo "pub fn dummy() {}" > crates/proto/src/lib.rs \
    && echo "pub fn dummy() {}" > crates/clickhouse/src/lib.rs \
    && echo "fn main() {}" > crates/ingestion/src/main.rs \
    && echo "fn main() {}" > crates/api/src/main.rs \
    && echo "pub fn dummy() {}" > crates/observability/src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/

# Touch source files to invalidate cache and rebuild with real code
RUN touch crates/*/src/*.rs

# Build release binaries
RUN cargo build --release --bin funnel-ingestion --bin funnel-api

# -----------------------------------------------------------------------------
# Stage 2: Ingestion runtime (Alpine 3.18 to match strfry)
# -----------------------------------------------------------------------------
FROM alpine:3.18 AS ingestion

RUN apk add --no-cache \
    ca-certificates \
    curl

# Copy strfry binary (now works natively - same Alpine base)
COPY --from=ghcr.io/hoytech/strfry:latest /app/strfry /usr/local/bin/strfry

WORKDIR /app

COPY --from=builder /app/target/release/funnel-ingestion /app/funnel-ingestion

# Default command (overridden in docker-compose)
CMD ["/app/funnel-ingestion"]

# -----------------------------------------------------------------------------
# Stage 3: API runtime
# -----------------------------------------------------------------------------
FROM alpine:3.18 AS api

RUN apk add --no-cache \
    ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/funnel-api /app/funnel-api

EXPOSE 8080

CMD ["/app/funnel-api"]
