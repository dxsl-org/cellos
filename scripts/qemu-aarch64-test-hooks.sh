#!/usr/bin/env bash
# Boot the Cellos AArch64 test-hooks kernel in QEMU with semihosting enabled.
#
# Dedicated test-hooks runner; does not alter production boot runner defaults.
#
# Usage: scripts/qemu-aarch64-test-hooks.sh [kernel-elf] [disk.img]

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel-test-hooks}"
DISK="${2:-disk_arm_virt.img}"
BOOT_WINDOW="${BOOT_WINDOW:-35}"

if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "FAIL: qemu-system-aarch64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: test-hooks kernel ELF not found: $KERNEL" >&2
    echo "  Build with: bash scripts/build-aarch64-test-hooks-ci.sh" >&2
    exit 1
fi

# Create a temporary test disk if not existing
if [[ ! -f "$DISK" ]]; then
    if [[ -f "scripts/format-disk-arm.sh" ]]; then
        bash scripts/format-disk-arm.sh "$DISK"
    fi
fi

echo "[qemu-aarch64-test-hooks] Booting kernel=$KERNEL (window=${BOOT_WINDOW}s, semihosting enabled)"

QEMU_ARGS=(
    -machine virt
    -cpu cortex-a57
    -m 256M
    -nographic
    -kernel "$KERNEL"
    -no-reboot
    -semihosting
)

if [[ -f "$DISK" ]]; then
    QEMU_ARGS+=(
        -drive "if=none,file=$DISK,format=raw,id=hd0"
        -device virtio-blk-device,drive=hd0
    )
fi

QEMU_EXIT_CODE=0
timeout "$BOOT_WINDOW" qemu-system-aarch64 "${QEMU_ARGS[@]}" \
    < /dev/null > qemu-aarch64-test-hooks.raw.log 2>&1 || QEMU_EXIT_CODE=$?

# Strip NULs and ANSI escape sequences
tr -d '\000' < qemu-aarch64-test-hooks.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-aarch64-test-hooks.log

if [[ $QEMU_EXIT_CODE -ne 0 ]]; then
    echo "FAIL: QEMU exited with code $QEMU_EXIT_CODE (expected 0 via semihosting)" >&2
    tail -40 qemu-aarch64-test-hooks.log
    exit 1
fi

if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-aarch64-test-hooks.log; then
    echo "FAIL: kernel panic / cell fault detected during aarch64 test-hooks boot" >&2
    grep -ai "fault\|PANIC" qemu-aarch64-test-hooks.log | head -20
    exit 1
fi

# Verify core test-hooks markers
REQUIRED_MARKERS=(
    "vfs-lifetime self-test PASS"
    "stack-probe self-test PASS"
    "stack-sizing policy self-test PASS"
    "admission-core self-test PASS"
    "ATOMIC_PUBLICATION_ARMING: PASS"
    "ATOMIC_PUBLICATION_AP-15: armed for trusted init"
    "[vfs-test] ALL TESTS PASSED"
)

ALL_PASSED=1
for marker in "${REQUIRED_MARKERS[@]}"; do
    if ! grep -Fq "$marker" qemu-aarch64-test-hooks.log; then
        echo "FAIL: missing required test marker: '$marker'" >&2
        ALL_PASSED=0
    fi
done

if [[ $ALL_PASSED -eq 0 ]]; then
    echo "FAIL: test-hooks assertions failed" >&2
    tail -40 qemu-aarch64-test-hooks.log
    exit 1
fi

echo "PASS: AArch64 test-hooks self-tests passed (semihosting enabled)"
cat qemu-aarch64-test-hooks.log | grep -E "PASS|spawned init"
exit 0
