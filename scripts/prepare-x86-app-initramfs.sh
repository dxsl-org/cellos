#!/usr/bin/env bash
# Repack the cached Alpine initramfs with a Tier 3 in-guest application fixture.
# Set the guest clock in the fixture: this emulated guest has no useful RTC,
# and TLS certificates cannot be verified until its clock is initialized.
# Prune unused modules to leave memory for apk and the application in the
# bounded Alpine guest RAM profiles.
#
# Environment:
#   ALPINE_X86_INITRAMFS  cached source (default .alpine-cache-x86/initramfs-virt)
#   APP_GATE_INITRAMFS    output path (required)
#   APP_GATE_FIXTURE      guest init script (required)

set -euo pipefail

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SOURCE="${ALPINE_X86_INITRAMFS:-.alpine-cache-x86/initramfs-virt}"
OUTPUT="${APP_GATE_INITRAMFS:?APP_GATE_INITRAMFS output path required}"
FIXTURE="${APP_GATE_FIXTURE:?APP_GATE_FIXTURE guest fixture required}"
PYTHON_BIN="${PYTHON_BIN:-python3}"

if [[ ! -f "$SOURCE" ]]; then
    if [[ "$SOURCE" != ".alpine-cache-x86/initramfs-virt" ]]; then
        echo "FAIL: Alpine initramfs not found: $SOURCE" >&2
        exit 1
    fi
    bash scripts/fetch-alpine-x86.sh .alpine-cache-x86
fi
command -v "$PYTHON_BIN" >/dev/null 2>&1 \
    || { echo "FAIL: Python 3 interpreter not found: $PYTHON_BIN" >&2; exit 1; }
[[ -f "$FIXTURE" ]] || { echo "FAIL: fixture not found: $FIXTURE" >&2; exit 1; }

source_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$SOURCE")"
output_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$OUTPUT")"
[[ "$source_real" != "$output_real" ]] \
    || { echo "FAIL: evidence output must not replace cached Alpine input" >&2; exit 1; }
source_sha="$(sha256sum "$SOURCE" | cut -d ' ' -f 1)"

stage_init="$(mktemp)"
trap 'rm -f "$stage_init"' EXIT
sed -e "s/@BUILD_UTC@/$(date -u +%s)/" \
    -e "s/@BUILD_UTC_STR@/$(date -u '+%Y-%m-%d %H:%M:%S')/" \
    "$FIXTURE" > "$stage_init"
if grep -q '@BUILD_UTC' "$stage_init"; then
    echo "FAIL: build-time stamp placeholder not substituted in $FIXTURE" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
"$PYTHON_BIN" tools/repack-initramfs.py "$SOURCE" "$OUTPUT" \
    --add /bin/virtio-e2e-init "$stage_init" 100755 \
    --prune-modules-to \
        kernel/drivers/block/virtio_blk.ko \
        kernel/drivers/net/virtio_net.ko \
        kernel/drivers/net/net_failover.ko \
        kernel/net/core/failover.ko

[[ "$(sha256sum "$SOURCE" | cut -d ' ' -f 1)" == "$source_sha" ]] \
    || { echo "FAIL: cached Alpine initramfs changed during repack" >&2; exit 1; }
echo "APP_GATE_INITRAMFS_READY=$OUTPUT"
sha256sum "$OUTPUT"
