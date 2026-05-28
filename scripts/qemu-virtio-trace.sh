#!/usr/bin/env bash
# Run QEMU with VirtIO MMIO tracing enabled for debugging block/input hangs.
# Usage: bash scripts/qemu-virtio-trace.sh [kernel-elf]
set -euo pipefail

KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/vios-kernel}"
LOG="qemu-virtio.log"

echo "Tracing VirtIO events → ${LOG}"
qemu-system-riscv64 \
  -machine virt \
  -nographic \
  -bios default \
  -drive file=disk_v3.img,if=virtio,format=raw \
  -device virtio-keyboard-device \
  -kernel "$KERNEL" \
  -trace "virtio_*" \
  -D "$LOG" \
  2>&1 | tee qemu-serial.log
