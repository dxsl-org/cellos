#!/usr/bin/env bash
# Build the detached Chrome App payload image (chrome-app.ext4) for CellOS.
# This volume contains exclusively the Chromium browser binary and runtime assets,
# designed to be mounted as /dev/vdb at /opt/app in the shared Linux base guest.
#
# Output:
#   build/chrome-app.ext4
#
# Usage:
#   bash scripts/build-chrome-app.sh

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="build"
OUTPUT="$OUT_DIR/chrome-app.ext4"
IMAGE_SIZE="350M"
WORK_DIR=$(mktemp -d -t cellos-chrome-app-XXXXXX)

trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$OUT_DIR"

if ! command -v docker >/dev/null 2>&1; then
    echo "ERROR: docker is required to extract chromium assets" >&2
    exit 1
fi
if ! command -v mke2fs >/dev/null 2>&1; then
    echo "ERROR: mke2fs (e2fsprogs) is required to build the ext4 image" >&2
    exit 1
fi

echo "[build-chrome-app] 1/3 Extracting Chromium binary and assets from Alpine..."
mkdir -p "$WORK_DIR/root/chromium"

docker run --rm -v "$WORK_DIR/root":/out alpine:3.21 sh -c "
  apk update && apk add --no-cache chromium
  cp -a /usr/lib/chromium/* /out/chromium/
  chown -R $(id -u):$(id -g) /out
" >/dev/null

echo "[build-chrome-app] 2/3 Creating app entrypoint..."
cat << 'EOF' > "$WORK_DIR/root/entrypoint.sh"
#!/bin/sh
# Chrome App Entrypoint for CellOS Tier 3 Guest
# Runs Chromium under Wayland / Cage or Headless with hardware sandbox disabled (contained by stage-2 EPT)

EXTRA_ARGS=""
if [ "$MODE" = "headless" ]; then
    EXTRA_ARGS="--headless --dump-dom"
fi

exec /opt/app/chromium/chrome \
    --no-sandbox \
    --disable-gpu \
    --disable-dev-shm-usage \
    --disable-software-rasterizer \
    --ozone-platform=wayland \
    $EXTRA_ARGS \
    "${@:-https://google.com}"
EOF
chmod +x "$WORK_DIR/root/entrypoint.sh"

echo "[build-chrome-app] 3/3 Formatting ext4 payload image: $OUTPUT ($IMAGE_SIZE)..."
rm -f "$OUTPUT"
truncate -s "$IMAGE_SIZE" "$OUTPUT"
mke2fs -t ext4 -F -L "CHROME_APP" -d "$WORK_DIR/root" "$OUTPUT" >/dev/null

echo ""
echo "=========================================================================="
echo "  CellOS Detached Chrome App Image Ready: $OUTPUT"
echo "=========================================================================="
ls -lh "$OUTPUT"
echo "  Mount Target: /dev/vdb -> /opt/app"
echo "=========================================================================="
