#!/usr/bin/env bash
# Build the unified Linux Base VM image (linux-base.ext4) for CellOS Tier 3 guests.
# Supports both headless and GUI (Wayland/Cage) execution via kernel cmdline:
#   - cellos.mode=headless (default)
#   - cellos.mode=gui
#
# Automatically attaches and executes application payload from /dev/vdb if present.
#
# Output:
#   build/linux-base.ext4
#
# Usage:
#   bash scripts/build-linux-base-image.sh

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="build"
OUTPUT="$OUT_DIR/linux-base.ext4"
IMAGE_SIZE="512M"
WORK_DIR=$(mktemp -d -t cellos-linux-base-XXXXXX)

trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$OUT_DIR"

if ! command -v docker >/dev/null 2>&1; then
    echo "ERROR: docker is required to assemble the base Linux image" >&2
    exit 1
fi
if ! command -v mke2fs >/dev/null 2>&1; then
    echo "ERROR: mke2fs (e2fsprogs) is required to build the ext4 image" >&2
    exit 1
fi

echo "[build-linux-base] 1/4 Assembling base Alpine Linux rootfs via Docker..."
cat << 'EOF' > "$WORK_DIR/Dockerfile"
FROM alpine:3.21

RUN apk update && apk add --no-cache \
    busybox \
    ca-certificates \
    cage \
    font-dejavu \
    fontconfig \
    freetype \
    mesa-dri-gallium \
    nss \
    nspr \
    alsa-lib \
    dbus \
    e2fsprogs \
    && rm -rf /var/cache/apk/*

# Create guest init script
RUN cat << 'INITSCRIPT' > /init
#!/bin/sh
set -e

# Mount pseudo filesystems
mount -t proc none /proc
mount -t sysfs none /sys
mount -t devtmpfs none /dev
mkdir -p /dev/pts /dev/shm /run /tmp
mount -t devpts none /dev/pts
mount -t tmpfs none /dev/shm
mount -t tmpfs none /run
chmod 1777 /tmp /dev/shm

hostname cellos-guest
ifconfig lo 127.0.0.1 up

# Network bring-up
ifconfig eth0 up 2>/dev/null || true
udhcpc -b -i eth0 2>/dev/null || true

# Parse mode from kernel command line
MODE="headless"
for cmd in $(cat /proc/cmdline); do
    case "$cmd" in
        cellos.mode=gui) MODE="gui" ;;
        cellos.mode=headless) MODE="headless" ;;
    esac
done

# Check and mount secondary app disk (/dev/vdb)
if [ -b /dev/vdb ]; then
    mkdir -p /opt/app
    mount -o ro /dev/vdb /opt/app 2>/dev/null || mount /dev/vdb /opt/app 2>/dev/null || true
    echo "[guest-init] secondary disk attached at /opt/app"
fi

# Start DBus system daemon
mkdir -p /var/run/dbus
dbus-uuidgen --ensure 2>/dev/null || true
dbus-daemon --system --fork 2>/dev/null || true

echo "[guest-init] Linux guest ready (mode=$MODE)"

ENTRYPOINT=""
if [ -x /opt/app/entrypoint.sh ]; then
    ENTRYPOINT="/opt/app/entrypoint.sh"
elif [ -x /opt/app/chrome ]; then
    ENTRYPOINT="/opt/app/chrome --no-sandbox"
elif [ -x /opt/app/chromium ]; then
    ENTRYPOINT="/opt/app/chromium --no-sandbox"
fi

if [ "$MODE" = "gui" ]; then
    export XDG_RUNTIME_DIR=/run/user/0
    mkdir -p -m 0700 "$XDG_RUNTIME_DIR"
    export WLR_RENDERER=pixman
    export WLR_NO_HARDWARE_CURSORS=1
    if [ -n "$ENTRYPOINT" ]; then
        exec cage -s -- $ENTRYPOINT
    else
        exec cage -s -- /bin/sh
    fi
else
    if [ -n "$ENTRYPOINT" ]; then
        exec $ENTRYPOINT
    else
        exec /bin/sh </dev/ttyS0 >/dev/ttyS0 2>&1
    fi
fi
INITSCRIPT

RUN chmod +x /init
EOF

echo "[build-linux-base] 2/4 Building Docker image..."
docker build -t cellos-linux-base:temp "$WORK_DIR" >/dev/null

echo "[build-linux-base] 3/4 Extracting rootfs..."
CONTAINER_ID=$(docker create cellos-linux-base:temp)
mkdir -p "$WORK_DIR/rootfs"
docker export "$CONTAINER_ID" | tar -x -C "$WORK_DIR/rootfs"
docker rm -f "$CONTAINER_ID" >/dev/null
docker rmi cellos-linux-base:temp >/dev/null

echo "[build-linux-base] 4/4 Formatting ext4 filesystem image: $OUTPUT ($IMAGE_SIZE)..."
rm -f "$OUTPUT"
truncate -s "$IMAGE_SIZE" "$OUTPUT"
mke2fs -t ext4 -F -L "CELLOS_LINUX" -d "$WORK_DIR/rootfs" "$OUTPUT" >/dev/null

echo ""
echo "=========================================================================="
echo "  CellOS Unified Linux Base Image Ready: $OUTPUT"
echo "=========================================================================="
ls -lh "$OUTPUT"
echo "  Features: Headless & GUI (Wayland/Cage), secondary app auto-mount (/dev/vdb)"
echo "=========================================================================="
