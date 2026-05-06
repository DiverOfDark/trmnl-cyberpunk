# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# Cache dependency build by copying only the manifests first, then a stub
# main.rs. The dep layer is reused as long as Cargo.toml/Cargo.lock are stable.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/trmnl-cyberpunk target/release/deps/trmnl_cyberpunk-*

# Now the real source.
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
