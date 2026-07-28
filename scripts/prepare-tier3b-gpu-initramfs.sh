#!/usr/bin/env bash
set -euo pipefail

CACHE_DIR="${1:-.alpine-cache}"
OUTPUT="${2:-$CACHE_DIR/initramfs-tier3b-gpu}"
TARGET="aarch64-unknown-none-softfloat"
MANIFEST="tests/guests/tier3b-gpu-probe/Cargo.toml"
PROBE="tests/guests/tier3b-gpu-probe/target/$TARGET/release/tier3b-gpu-probe"

for required in "$CACHE_DIR/initramfs-virt" "$CACHE_DIR/modloop-virt"; do
    [[ -f "$required" ]] || { echo "ERROR: missing $required" >&2; exit 1; }
done

cargo build --manifest-path "$MANIFEST" --target "$TARGET" --release
PYTHON_BIN="${PYTHON_BIN:-python}"
"$PYTHON_BIN" tools/repack-initramfs.py "$CACHE_DIR/initramfs-virt" "$OUTPUT" \
    --add bin/sh tests/guests/tier3b-gpu-probe/guest-init.sh 100755 \
    --add tier3b-gpu-probe "$PROBE" 100755 \
    --add modloop.squashfs "$CACHE_DIR/modloop-virt" 100644
echo "[tier3b-initramfs] created $OUTPUT"
