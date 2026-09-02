#!/usr/bin/env bash
# Run the isolated x86 IDT actual-entry image with a strict serial/status oracle.

set -euo pipefail

ISO="${1:-build/x86-idt-test/vicell-x86-idt-test.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"
RAW_LOG="build/x86-idt-test/qemu.raw.log"
LOG="build/x86-idt-test/qemu.log"
CPL0_MARKER="X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32"
CPL3_MARKER="X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544"

if ! command -v qemu-system-x86_64 &>/dev/null; then
    echo "FAIL: qemu-system-x86_64 not found on PATH" >&2
    exit 1
fi
if [[ ! -f "$ISO" ]]; then
    echo "FAIL: IDT test ISO not found: $ISO" >&2
    echo "  Build with: bash scripts/build-x86_64-idt-test-ci.sh" >&2
    exit 1
fi

mkdir -p build/x86-idt-test
status=0
timeout --signal=KILL "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    -cpu qemu64,+pdpe1gb,+pku \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    < /dev/null > "$RAW_LOG" 2>&1 || status=$?

tr -d '\000\r' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"
cpl0_count="$(grep -Fxc "$CPL0_MARKER" "$LOG" || true)"
cpl3_count="$(grep -Fxc "$CPL3_MARKER" "$LOG" || true)"
if [[ "$status" -ne 33 ]]; then
    echo "FAIL: IDT test QEMU status $status (expected 33)" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
if [[ "$cpl0_count" -ne 1 ]]; then
    echo "FAIL: expected exactly one CPL0 IDT PASS marker, saw $cpl0_count" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
if [[ "$cpl3_count" -ne 1 ]]; then
    echo "FAIL: expected exactly one real-CPL3 IDT PASS marker, saw $cpl3_count" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
if grep -Eqi '(^|[^[:alpha:]])(FAIL|PANIC|FAULT|SKIP|RESET|TRIPLE[- ]FAULT)([^[:alpha:]]|$)' "$LOG"; then
    echo "FAIL: forbidden failure/skip/reset marker present despite success status" >&2
    tail -40 "$LOG" >&2
    exit 1
fi

echo "PASS: x86 IDT CPL0 and mandatory real-CPL3 GS/PKRU transition oracle"
