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

# ── Stage 2: Patched TRMNL firmware (OTA update for the device) ──────────────
# See firmware/build.sh. Layered for the CI layer cache (type=gha): the
# upstream checkout + ESP32 toolchain/libs (~2.4 GB unpacked) depend only on
# FIRMWARE_REF/FIRMWARE_ENV, so editing patches reuses them and only recompiles.
FROM python:3.12-slim-bookworm AS firmware
RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir platformio
COPY firmware/FIRMWARE_REF firmware/FIRMWARE_ENV ./firmware/
RUN git clone --quiet --depth 1 --branch "$(cat firmware/FIRMWARE_REF)" \
        https://github.com/usetrmnl/trmnl-firmware /src \
    && pio pkg install --project-dir /src --environment "$(cat firmware/FIRMWARE_ENV)"
COPY firmware ./firmware
RUN FIRMWARE_SRC_DIR=/src ./firmware/build.sh /out

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
# Pixel-direct rendering: no browser, no fonts, no graphics libs needed.
# Just the static binary + ca-certs for HTTPS to upstream APIs.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/trmnl-cyberpunk ./trmnl-cyberpunk
COPY --from=firmware /out ./firmware

# The memo lives in DATA_DIR; mount a volume there to keep it across restarts.
ENV LISTEN=0.0.0.0:8080 \
    RUST_LOG=trmnl_cyberpunk=info \
    DATA_DIR=/data
VOLUME /data

EXPOSE 8080
ENTRYPOINT ["/app/trmnl-cyberpunk"]
