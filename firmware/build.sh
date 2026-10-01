#!/usr/bin/env bash
# Build the patched TRMNL firmware served to the device as an OTA update.
#
#   firmware/build.sh <out-dir>
#
# Clones usetrmnl/trmnl-firmware at the ref in firmware/FIRMWARE_REF (or reuses
# FIRMWARE_SRC_DIR if it's already a checkout of that ref), applies
# firmware/patches/*.patch, and builds the PlatformIO env in firmware/FIRMWARE_ENV.
# Writes <out-dir>/firmware.bin (OTA app image) and <out-dir>/version.txt.
#
# The version gets a "-cp.<hash>" suffix derived from the ref + patches, so the
# server can tell this build apart from the official one with the same number
# and any patch change produces a new version (and therefore a new OTA offer).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="$(mkdir -p "${1:?usage: build.sh <out-dir>}" && cd "$1" && pwd)"
ref="$(tr -d '[:space:]' < "$here/FIRMWARE_REF")"
pio_env="$(tr -d '[:space:]' < "$here/FIRMWARE_ENV")"
src="${FIRMWARE_SRC_DIR:-$(mktemp -d)}"

suffix="-cp.$(cat "$here/FIRMWARE_REF" "$here/FIRMWARE_ENV" "$here"/patches/*.patch | sha256sum | cut -c1-7)"

# Reuse an existing checkout of the same ref (the Dockerfile pre-clones it to
# cache the toolchain download in its own layer); otherwise clone fresh.
if [ "$(git -C "$src" describe --tags --exact-match 2>/dev/null)" = "$ref" ]; then
  git -C "$src" reset --quiet --hard
  git -C "$src" clean --quiet -fd
else
  rm -rf "$src"
  git clone --quiet --depth 1 --branch "$ref" https://github.com/usetrmnl/trmnl-firmware "$src"
fi
git -C "$src" apply "$here"/patches/*.patch

PLATFORMIO_BUILD_FLAGS="-DFW_VERSION_SUFFIX=\\\"$suffix\\\"" \
  pio run --project-dir "$src" --environment "$pio_env"

base_version="$(sed -nE 's/^#define FW_(MAJOR|MINOR|PATCH)_VERSION ([0-9]+)/\2/p' "$src/include/config.h" | paste -sd.)"
cp "$src/.pio/build/$pio_env/firmware.bin" "$out/firmware.bin"
echo "$base_version$suffix" > "$out/version.txt"
echo "Built firmware $(cat "$out/version.txt") → $out/firmware.bin"
