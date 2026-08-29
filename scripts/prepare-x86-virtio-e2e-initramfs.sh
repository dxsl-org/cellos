#!/usr/bin/env bash
# Build the dedicated x86 VirtIO evidence initramfs without changing Alpine cache.

set -euo pipefail

SOURCE="${ALPINE_X86_INITRAMFS:-.alpine-cache-x86/initramfs-virt}"
OUTPUT="${VIRTIO_E2E_INITRAMFS:-build/x86-virtio-e2e-initramfs.cpio.gz}"
NORMAL_INIT="tests/guests/x86-virtio-e2e/guest-init.sh"
HOSTILE_INIT="tests/guests/x86-virtio-e2e/hostile-init.sh"
HOSTILE_SOURCE="tests/guests/x86-virtio-e2e/hostile-mmio.c"
MODE="${VIRTIO_E2E_MODE:-normal}"
HELPER="${VIRTIO_HOSTILE_HELPER:-build/x86-virtio-hostile-mmio}"
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
case "$MODE" in
    normal)
        INIT="$NORMAL_INIT"
        [[ -f "$INIT" ]] || { echo "FAIL: fixture not found: $INIT" >&2; exit 1; }
        ;;
    hostile)
        INIT="$HOSTILE_INIT"
        for input in "$HOSTILE_INIT" "$HOSTILE_SOURCE"; do
            [[ -f "$input" ]] || { echo "FAIL: fixture not found: $input" >&2; exit 1; }
        done
        ;;
    *) echo "FAIL: VIRTIO_E2E_MODE must be normal or hostile" >&2; exit 1 ;;
esac
if [[ "$MODE" == hostile ]]; then
    command -v "${CC:-cc}" >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: C compiler required for hostile guest helper" >&2; exit 1; }
    mkdir -p "$(dirname "$HELPER")"
    "${CC:-cc}" -Os -static -nostdlib -ffreestanding -fno-builtin \
        -fno-pie -no-pie -fno-stack-protector -Wl,--build-id=none \
        -s -o "$HELPER" "$HOSTILE_SOURCE"
fi

source_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$SOURCE")"
output_real="$($PYTHON_BIN -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$OUTPUT")"
[[ "$source_real" != "$output_real" ]] \
    || { echo "FAIL: evidence output must not replace cached Alpine input" >&2; exit 1; }
source_sha="$(sha256sum "$SOURCE" | cut -d ' ' -f 1)"

repack_args=(--add /bin/virtio-e2e-init "$INIT" 100755)
if [[ "$MODE" == hostile ]]; then
    repack_args+=(--add /bin/virtio-hostile-mmio "$HELPER" 100755)
fi
"$PYTHON_BIN" tools/repack-initramfs.py "$SOURCE" "$OUTPUT" "${repack_args[@]}"

[[ "$(sha256sum "$SOURCE" | cut -d ' ' -f 1)" == "$source_sha" ]] \
    || { echo "FAIL: cached Alpine initramfs changed during repack" >&2; exit 1; }
echo "VIRTIO_E2E_INITRAMFS_READY=$OUTPUT"
sha256sum "$OUTPUT"
