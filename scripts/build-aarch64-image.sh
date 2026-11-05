#!/usr/bin/env bash
# Produce the raw arm64 `Image` next to an AArch64 kernel ELF.
#
# The ELF must come from a backend whose `_start` carries the Linux arm64 Image
# header (`ArmPlatform::LINUX_IMAGE_HEADER`, QEMU virt today). The raw image is
# the ELF's load segments flattened from the link address, so an Image loader
# (QEMU `-kernel Image`, m1n1, VZLinuxBootLoader) enters at offset 0 with the
# FDT address in x0. The header's magic is checked so an ELF built without the
# header is refused rather than written out as an unbootable blob.
#
# Usage: scripts/build-aarch64-image.sh [kernel-elf] [out-image]
#   kernel-elf  default: target/aarch64-unknown-none-softfloat/release/cellos-kernel
#   out-image   default: <kernel-elf>.Image
# Environment:
#   OBJCOPY     objcopy to use (default: first of aarch64-linux-gnu-objcopy,
#               llvm-objcopy, rust-objcopy on PATH)

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
OUT="${2:-$KERNEL.Image}"

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: kernel ELF not found: $KERNEL" >&2
    exit 1
fi

if [[ -z "${OBJCOPY:-}" ]]; then
    for candidate in aarch64-linux-gnu-objcopy llvm-objcopy rust-objcopy; do
        if command -v "$candidate" &>/dev/null; then
            OBJCOPY="$candidate"
            break
        fi
    done
fi
if [[ -z "${OBJCOPY:-}" ]]; then
    echo "FAIL: no objcopy found (install binutils-aarch64-linux-gnu or set OBJCOPY)" >&2
    exit 1
fi

"$OBJCOPY" -O binary "$KERNEL" "$OUT"

# booting.rst: magic "ARM\x64" at offset 0x38.
MAGIC="$(od -An -tx1 -j 56 -N 4 "$OUT" | tr -d ' \n')"
if [[ "$MAGIC" != "41524d64" ]]; then
    rm -f "$OUT"
    echo "FAIL: $KERNEL has no arm64 Image header (magic at 0x38 = '$MAGIC')" >&2
    exit 1
fi

echo "Image: $OUT ($(wc -c < "$OUT") bytes, from $KERNEL)"
