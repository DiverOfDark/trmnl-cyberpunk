# ── Stage 1: Fonts ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS fonts

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates unzip fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fonts

# Liberation Sans ships in every Debian install; rename to match expected font filenames.
RUN cp /usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf    SpaceGrotesk-Bold.ttf \
 && cp /usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf SpaceGrotesk-Regular.ttf

# JetBrains Mono — fall back to Liberation Mono if download fails.
RUN curl -fsSL --retry 3 \
    "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip" \
    -o jb.zip \
    && unzip -j jb.zip "fonts/ttf/JetBrainsMono-Regular.ttf" -d . \
    && rm jb.zip \
    || cp /usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf JetBrainsMono-Regular.ttf

# ── Stage 2: Dependency cache (cargo-chef) ────────────────────────────────────
FROM lukemathwalker/cargo-chef:latest-rust-1-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build deps only — this layer is cached as long as Cargo.toml/Cargo.lock don't change.
RUN cargo chef cook --release --recipe-path recipe.json
# Build the actual binary.
COPY src ./src
RUN cargo build --release --bin trmnl-seedbox

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/trmnl-seedbox ./trmnl-seedbox
COPY --from=fonts   /fonts ./fonts

ENV FONT_DIR=/app/fonts \
    LISTEN=0.0.0.0:8080 \
    RUST_LOG=trmnl_seedbox=info

EXPOSE 8080
ENTRYPOINT ["/app/trmnl-seedbox"]
