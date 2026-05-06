# ── Stage 1: Assemble fonts ───────────────────────────────────────────────────
FROM debian:bookworm-slim AS fonts

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates unzip fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fonts

# Liberation Sans ships in every Debian install — use as the display font.
# Replace SpaceGrotesk-*.ttf with the real Space Grotesk TTFs if desired.
RUN cp /usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf    SpaceGrotesk-Bold.ttf \
 && cp /usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf SpaceGrotesk-Regular.ttf

# JetBrains Mono from GitHub releases
RUN curl -fsSL "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip" \
    -o jb.zip \
    && unzip -j jb.zip "fonts/ttf/JetBrainsMono-Regular.ttf" -d . \
    && rm jb.zip

# ── Stage 2: Build binary ─────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder

WORKDIR /app

# Cache dependency layer
COPY Cargo.toml ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs && cargo build --release 2>/dev/null || true
RUN rm -rf src

COPY src ./src
RUN cargo build --release

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/trmnl-seedbox /app/trmnl-seedbox
COPY --from=fonts   /fonts /app/fonts

ENV FONT_DIR=/app/fonts
ENV LISTEN=0.0.0.0:8080
ENV RUST_LOG=trmnl_seedbox=info

EXPOSE 8080

ENTRYPOINT ["/app/trmnl-seedbox"]
