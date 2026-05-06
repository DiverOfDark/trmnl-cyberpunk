# ── Stage 1: Dependency cache (cargo-chef) ────────────────────────────────────
FROM lukemathwalker/cargo-chef:latest-rust-1-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY src ./src
COPY templates ./templates
RUN cargo build --release --bin trmnl-cyberpunk

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    chromium \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/trmnl-cyberpunk ./trmnl-cyberpunk
COPY --from=builder /app/templates ./templates

ENV LISTEN=0.0.0.0:8080 \
    RUST_LOG=trmnl_cyberpunk=info \
    CHROME_PATH=/usr/bin/chromium

EXPOSE 8080
ENTRYPOINT ["/app/trmnl-cyberpunk"]
