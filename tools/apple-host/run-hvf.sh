#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Run Cellos AArch64 guest on Apple Silicon macOS using QEMU HVF accelerator.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL="${1:-$ROOT/target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
DISK="${2:-$ROOT/disk_arm_virt.img}"
MEMORY="${QEMU_MEMORY:-2G}"
CPUS="${QEMU_SMP:-4}"

echo "==> Running Cellos on Apple Silicon macOS via QEMU HVF..."
qemu-system-aarch64 \
    -M virt,gic-version=3,highmem=on \
    -accel hvf \
    -cpu host \
    -smp "$CPUS" \
    -m "$MEMORY" \
    -nographic \
    -kernel "$KERNEL" \
    -drive "file=$DISK,if=none,id=drive0,format=raw" \
    -device virtio-blk-pci,drive=drive0 \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 \
    -append "cellos.profile=aarch64-vm"
