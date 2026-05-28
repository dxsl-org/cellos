#!/usr/bin/env bash
# Boot ViOS kernel in QEMU and assert the stable boot banner appears.
# Usage: scripts/qemu-boot-test.sh [path/to/kernel-elf]
set -euo pipefail

KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/vios-kernel}"

timeout 60 qemu-system-riscv64 \
  -machine virt \
  -nographic \
  -bios default \
  -kernel "$KERNEL" 2>&1 | tee qemu.log | grep -q "\[ViOS\] kernel boot" || {
    echo "FAIL: boot banner not seen within 60s"
    cat qemu.log
    exit 1
}

echo "PASS: kernel booted successfully"
