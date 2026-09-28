#!/usr/bin/env bash
# Build the Tier 3 nginx-gate initramfs from the cached Alpine one.
#
# Two things happen here:
#   1. The guest fixture replaces `/bin/virtio-e2e-init` (the x86 E2E profile's
#      guest slot) and gets the build-time UTC stamp substituted in — the
#      emulated guest RTC reads back no time, so TLS chain verification fails
#      without it.
#   2. The module tree is pruned to the VirtIO set the guest needs. The full
#      Alpine tree is ~16.5 MiB and the guest carve is 128 MiB; leaving it in
#      pushed apk into the guest OOM killer (`Out of memory: Killed process …
#      (apk)`) before nginx could be installed.
#
# Usage: bash scripts/prepare-x86-nginx-initramfs.sh
# Environment (defaults match the other x86 lanes):
#   ALPINE_X86_INITRAMFS  source initramfs (default .alpine-cache-x86/initramfs-virt)
#   NGINX_GATE_INITRAMFS  output (default build/x86-nginx-initramfs.cpio.gz)
#   NGINX_GATE_FIXTURE    guest fixture (default tests/guests/x86-nginx/guest-init.sh)

set -euo pipefail

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SOURCE="${ALPINE_X86_INITRAMFS:-.alpine-cache-x86/initramfs-virt}"
OUTPUT="${NGINX_GATE_INITRAMFS:-build/x86-nginx-initramfs.cpio.gz}"
FIXTURE="${NGINX_GATE_FIXTURE:-tests/guests/x86-nginx/guest-init.sh}"
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
echo "NGINX_GATE_INITRAMFS_READY=$OUTPUT"
sha256sum "$OUTPUT"
