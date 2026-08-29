#!/usr/bin/env bash
# Build the dedicated x86 VirtIO evidence initramfs without changing Alpine cache.

set -euo pipefail

SOURCE="${ALPINE_X86_INITRAMFS:-.alpine-cache-x86/initramfs-virt}"
OUTPUT="${VIRTIO_E2E_INITRAMFS:-build/x86-virtio-e2e-initramfs.cpio.gz}"
PROBE="tests/guests/x86-virtio-e2e/guest-init.sh"
PYTHON_BIN="${PYTHON_BIN:-python3}"

if [[ ! -f "$SOURCE" ]]; then
    if [[ "$SOURCE" != ".alpine-cache-x86/initramfs-virt" ]]; then
        echo "FAIL: Alpine initramfs not found: $SOURCE" >&2
        exit 1
    fi
    bash scripts/fetch-alpine-x86.sh .alpine-cache-x86
fi
[[ -f "$PROBE" ]] || { echo "FAIL: probe not found: $PROBE" >&2; exit 1; }
command -v "$PYTHON_BIN" >/dev/null 2>&1 \
    || { echo "FAIL: Python 3 interpreter not found: $PYTHON_BIN" >&2; exit 1; }

source_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$SOURCE")"
output_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$OUTPUT")"
[[ "$source_real" != "$output_real" ]] \
    || { echo "FAIL: evidence output must not replace cached Alpine input" >&2; exit 1; }
source_sha="$(sha256sum "$SOURCE" | cut -d ' ' -f 1)"

"$PYTHON_BIN" tools/repack-initramfs.py "$SOURCE" "$OUTPUT" \
    --add /bin/virtio-e2e-init "$PROBE" 100755

[[ "$(sha256sum "$SOURCE" | cut -d ' ' -f 1)" == "$source_sha" ]] \
    || { echo "FAIL: cached Alpine initramfs changed during repack" >&2; exit 1; }
echo "VIRTIO_E2E_INITRAMFS_READY=$OUTPUT"
sha256sum "$OUTPUT"
