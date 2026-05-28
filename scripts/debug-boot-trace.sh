#!/usr/bin/env bash
# Boot ViOS kernel in QEMU with full CPU+interrupt tracing for boot-hang diagnosis.
# Output is written to qemu-trace.log; serial output is printed to stdout.
# Usage: bash scripts/debug-boot-trace.sh [path/to/kernel-elf]
set -euo pipefail

KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/vios-kernel}"
TRACE_LOG="qemu-trace.log"

echo "Booting with debug trace → ${TRACE_LOG}"
echo "Expected output: [ViOS] kernel boot … → Hi from U-mode!"
echo ""

qemu-system-riscv64 \
  -machine virt \
  -nographic \
  -bios default \
  -kernel "$KERNEL" \
  -d cpu_reset,int,in_asm \
  -D "$TRACE_LOG" \
  2>&1
