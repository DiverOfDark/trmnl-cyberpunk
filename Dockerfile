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
RUN cargo build --release --bin trmnl-cyberpunk

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
# Pixel-direct rendering: no browser, no fonts, no graphics libs needed.
# Just the static binary + ca-certs for HTTPS to upstream APIs.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/trmnl-cyberpunk ./trmnl-cyberpunk

ENV LISTEN=0.0.0.0:8080 \
    RUST_LOG=trmnl_cyberpunk=info

EXPOSE 8080
ENTRYPOINT ["/app/trmnl-cyberpunk"]
