#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Run Cellos AArch64 guest on Apple Silicon macOS via Virtualization.framework / vfkit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL="${1:-$ROOT/build/aarch64-image/Image}"
DISK="${2:-$ROOT/disk_arm_virt.img}"
MEMORY="${VZ_MEMORY:-2048}"
CPUS="${VZ_CPUS:-4}"

if ! command -v vfkit >/dev/null 2>&1; then
    echo "ERROR: vfkit not found. Install via: brew install vfkit" >&2
    exit 1
fi

echo "==> Running Cellos on Apple Silicon macOS via vfkit (Virtualization.framework)..."
vfkit \
    --cpus "$CPUS" \
    --memory "$MEMORY" \
    --bootloader "linux,kernel=$KERNEL,cmdline=cellos.profile=aarch64-vm" \
    --device "virtio-blk,path=$DISK" \
    --device "virtio-net,nat" \
    --device "virtio-serial,stdio"
