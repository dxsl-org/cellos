#!/usr/bin/env bash
# Run the isolated x86 IDT actual-entry image with a strict serial/status oracle.
#
# Default: baseline profile (no SMEP/SMAP/UMIP) on qemu64. The protection
# profile runner sets X86_IDT_CPU and X86_IDT_PROTECT=on; see
# scripts/x86/qemu-idt-protection-test.sh.

set -euo pipefail

ISO="${1:-build/x86-idt-test/vicell-x86-idt-test.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"
CPU="${X86_IDT_CPU:-qemu64,+pdpe1gb,+pku}"
PROTECT="${X86_IDT_PROTECT:-off}"
OUT_DIR="$(dirname "$ISO")"
RAW_LOG="$OUT_DIR/qemu.raw.log"
LOG="$OUT_DIR/qemu.log"
CPL0_MARKER="X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32"
IST_MARKER="X86-IST: PASS gates=2/8/18->ist1/2/3 tss=own-cpu guards=8 guard-read=pf int2=ist1/bad-rsp int18=ist3/bad-rsp df=ist2/cascade nmi=ist1/lapic"
CPL3_MARKER="X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544"
case "$PROTECT" in
    off)
        KERNEL_PROTECT_MARKER="X86-PROTECT: smap=off read=ok window=ok smep=off syscall-flags=df+ac-cleared"
        UMIP_MARKER="X86-PROTECT: umip=off sgdt=ran"
        ;;
    on)
        KERNEL_PROTECT_MARKER="X86-PROTECT: smap=probe read=pf window=ok smep=on exec=pf syscall-flags=df+ac-cleared"
        UMIP_MARKER="X86-PROTECT: umip=on sgdt=gp"
        ;;
    *)
        echo "FAIL: X86_IDT_PROTECT must be off or on, got '$PROTECT'" >&2
        exit 1
        ;;
esac

if ! command -v qemu-system-x86_64 &>/dev/null; then
    echo "FAIL: qemu-system-x86_64 not found on PATH" >&2
    exit 1
fi
if [[ ! -f "$ISO" ]]; then
    echo "FAIL: IDT test ISO not found: $ISO" >&2
    echo "  Build with: bash scripts/build-x86_64-idt-test-ci.sh" >&2
    exit 1
fi

status=0
timeout --signal=KILL "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    -cpu "$CPU" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    < /dev/null > "$RAW_LOG" 2>&1 || status=$?

tr -d '\000\r' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"
if [[ "$status" -ne 33 ]]; then
    echo "FAIL: IDT test QEMU status $status (expected 33)" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
for marker in "$CPL0_MARKER" "$IST_MARKER" "$KERNEL_PROTECT_MARKER" "$UMIP_MARKER" "$CPL3_MARKER"; do
    count="$(grep -Fxc "$marker" "$LOG" || true)"
    if [[ "$count" -ne 1 ]]; then
        echo "FAIL: expected exactly one marker, saw $count: $marker" >&2
        tail -40 "$LOG" >&2
        exit 1
    fi
done
# IST high-water: each stack used, and at most half of its 8 KiB.
if ! grep -Eq '^X86-IST-HW: nmi=[0-9a-f]+ df=[0-9a-f]+ mc=[0-9a-f]+ limit=2000$' "$LOG"; then
    echo "FAIL: missing IST high-water report" >&2
    exit 1
fi
if grep -Eqi '(^|[^[:alpha:]])(FAIL|PANIC|FAULT|SKIP|RESET|TRIPLE[- ]FAULT)([^[:alpha:]]|$)' "$LOG"; then
    echo "FAIL: forbidden failure/skip/reset marker present despite success status" >&2
    tail -40 "$LOG" >&2
    exit 1
fi

echo "PASS: x86 IDT CPL0, IST, protect=$PROTECT and real-CPL3 GS/PKRU/flags oracle ($(grep -E '^X86-IST-HW' "$LOG"))"
