#!/usr/bin/env bash
# Boot the ViCell x86_64 kernel in QEMU q35 (Limine BIOS ISO) and assert the
# system reaches the interactive shell prompt ("ViCell >").
#
# Mirrors scripts/qemu-aarch64-test.sh for the x86_64 q35 machine.
#
# Usage: BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh [iso]
#   iso   default: build/vicell-x86.iso

set -euo pipefail

ISO="${1:-build/vicell-x86.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"

if ! command -v qemu-system-x86_64 &>/dev/null; then
    echo "FAIL: qemu-system-x86_64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$ISO" ]]; then
    echo "FAIL: ISO not found: $ISO" >&2
    echo "  Build with: cargo build --release -p vicell-kernel --target x86_64-unknown-none && bash scripts/x86/make-iso-ci.sh" >&2
    exit 1
fi

echo "[qemu-x86_64-test] Booting ISO=$ISO (window=${BOOT_WINDOW}s)"

timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    -cpu qemu64 \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -serial stdio \
    < /dev/null > qemu-x86_64.raw.log 2>&1 || true

# Strip NULs and ANSI escape sequences so patterns match cleanly.
tr -d '\000' < qemu-x86_64.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-x86_64.log

if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-x86_64.log; then
    echo "FAIL: kernel panic / cell fault detected during x86_64 boot" >&2
    grep -ai "fault\|PANIC" qemu-x86_64.log | head
    exit 1
fi

if grep -q "ViCell >" qemu-x86_64.log; then
    echo "PASS: x86_64 shell prompt reached — full boot successful"
    exit 0
fi

echo "FAIL: 'ViCell >' prompt not seen within ${BOOT_WINDOW}s" >&2
tail -40 qemu-x86_64.log
exit 1
