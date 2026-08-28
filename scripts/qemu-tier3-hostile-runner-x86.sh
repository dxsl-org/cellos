#!/usr/bin/env bash
# Run Tier 3 Hostile QEMU scenarios for x86 and parse the result.
#
# Usage:
#   bash scripts/qemu-tier3-hostile-runner-x86.sh [iso]
#   iso default: build/vicell-x86-hv.iso
#
# Environment:
#   BOOT_WINDOW    seconds to wait (default: 900)
#   QEMU_X86_BIN   emulator executable (default: qemu-system-x86_64)
#   QEMU_MEMORY    outer Cellos VM memory (default: 1G)
#   LOG_TAIL       lines of qemu-hv-hostile.raw.log printed on failure (default 200)

set -euo pipefail

ISO="${1:-build/vicell-x86-hv.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-900}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-1G}"
# 0. Build the Hostile ISO if requested.
if [ ! -f "$ISO" ] || [ "${BUILD_HOSTILE_ISO:-0}" == "1" ]; then
    echo "Building hostile ISO..."
    bash scripts/prepare-tier3-hostile-initramfs-x86.sh
    INITRD_OVERRIDE="build/tier3-hostile-initramfs-x86.cpio.gz" bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
    RUSTFLAGS="-C relocation-model=pic -C code-model=kernel -C target-feature=-red-zone" EMBEDDED_OVERRIDE="kernel/src/embedded-hv-x86" cargo build --release -p cellos-kernel --target x86_64-unknown-none
    bash scripts/x86/make-iso-ci.sh "$ISO"
fi

if ! command -v "$QEMU_X86_BIN" &>/dev/null; then
    echo "BLOCKED_ENVIRONMENT: $QEMU_X86_BIN not found"
    exit 1
fi

CORPUS_FILE="$(dirname "$0")/tier3-hostile-scenario-matrix.sh"
if [[ ! -f "$CORPUS_FILE" ]]; then
    echo "BLOCKED_ENVIRONMENT: scenario matrix file not found: $CORPUS_FILE"
    exit 1
fi
source "$CORPUS_FILE"

QEMU_VERSION_TEXT="$("$QEMU_X86_BIN" --version | sed -n '1p')"
if [[ ! "$QEMU_VERSION_TEXT" =~ QEMU\ emulator\ version\ ([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    echo "BLOCKED_ENVIRONMENT: cannot parse QEMU version: $QEMU_VERSION_TEXT"
    exit 1
fi
QEMU_MAJOR="${BASH_REMATCH[1]}"
QEMU_MINOR="${BASH_REMATCH[2]}"
QEMU_PATCH="${BASH_REMATCH[3]}"
if (( QEMU_MAJOR != 10 || QEMU_MINOR != 2 || QEMU_PATCH != 0 )); then
    echo "BLOCKED_ENVIRONMENT: require QEMU 10.2.0 for strict hostile-path evidence, found: $QEMU_VERSION_TEXT"
    exit 1
fi

if [[ ! -f "$ISO" ]]; then
    echo "BLOCKED_ENVIRONMENT: ISO not found: $ISO"
    exit 1
fi

QEMU_ISO="$ISO"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    QEMU_ISO="$(wslpath -w "$ISO")"
fi
BOOT_WINDOW="${BOOT_WINDOW:-60}"

QEMU_VERSION="$QEMU_VERSION_TEXT"
echo "[hv-hostile-x86] iso=$ISO memory=$QEMU_MEMORY (window=${BOOT_WINDOW}s)"
echo "[hv-hostile-x86] $QEMU_VERSION"

# Run QEMU in background.
"$QEMU_X86_BIN" \
    -machine q35 \
    -accel tcg \
    -cpu qemu64,+pdpe1gb,+svm \
    -m "$QEMU_MEMORY" \
    -nographic \
    -cdrom "$QEMU_ISO" \
    -boot d \
    -no-reboot \
    < /dev/null > qemu-hv-hostile.raw.log 2>&1 &
QEMU_PID=$!

# Observe the real budget and reset stimuli from the host. Their guest-side
# start markers do not prove VMM preemption or supervisor recovery.
BUDGET_WINDOW="${BUDGET_WINDOW:-1}"
RESET_WINDOW="${RESET_WINDOW:-3}"
BUDGET_STARTED_AT=""
BUDGET_LIVENESS=0
RESET_STARTED_AT=""
RESET_GUEST_EXIT_OBSERVED=0
end_time=$((SECONDS + BOOT_WINDOW))
while [[ $SECONDS -lt $end_time ]]; do
    if grep -q "\[HOSTILE_PROBE\] BUDGET_TEST_STARTED" qemu-hv-hostile.raw.log 2>/dev/null; then
        if [[ -z "$BUDGET_STARTED_AT" ]]; then
            BUDGET_STARTED_AT=$SECONDS
        elif (( SECONDS - BUDGET_STARTED_AT >= BUDGET_WINDOW )) && kill -0 "$QEMU_PID" 2>/dev/null; then
            BUDGET_LIVENESS=1
        fi
    fi
    if grep -q "\[HOSTILE_PROBE\] RESET_TEST_STARTED" qemu-hv-hostile.raw.log 2>/dev/null; then
        if [[ -z "$RESET_STARTED_AT" ]]; then
            RESET_STARTED_AT=$SECONDS
        elif grep -q "\[hv-x86\] guest exited" qemu-hv-hostile.raw.log 2>/dev/null; then
            RESET_GUEST_EXIT_OBSERVED=1
            break
        elif (( SECONDS - RESET_STARTED_AT >= RESET_WINDOW )); then
            break
        fi
    fi
    if grep -q "KERNEL PANIC\|\[fault\] Cell\|\[hv-x86\] guest triple-fault" qemu-hv-hostile.raw.log 2>/dev/null \
        || ! kill -0 "$QEMU_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done

if kill -0 "$QEMU_PID" 2>/dev/null; then
    kill "$QEMU_PID" 2>/dev/null || true
fi
wait "$QEMU_PID" 2>/dev/null || true
# Strip NULs and ANSI escapes for the parsed log.
tr -d '\000' < qemu-hv-hostile.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-hv-hostile.log

LOG_TAIL="${LOG_TAIL:-200}"

dump_log() {
    echo "--- qemu-hv-hostile.log tail ---"
    tail -n "$LOG_TAIL" qemu-hv-hostile.log
    echo "--------------------------------"
}

# 1. Check for hard environment failures / fatal panics.
if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-hv-hostile.log; then
    echo "FAIL: kernel panic or cell fault detected."
    dump_log
    exit 1
fi
if grep -qi "\[hv-x86\] guest triple-fault" qemu-hv-hostile.log; then
    # In some QEMU versions this is deterministic.
    echo "BLOCKED_ENVIRONMENT: Guest triple-fault (possibly QEMU environment issue)"
    dump_log
    exit 1
fi

if grep -qi "\[hv-x86\] .*fail\|\[hv-x86\] .*error\|\[hv-x86\] unhandled guest MMIO\|\[hv-x86\] unexpected" qemu-hv-hostile.log; then
    echo "FAIL: hypervisor cell error or unhandled state detected."
    dump_log
    exit 1
fi

for row in "${TIER3_HOSTILE_CORPUS[@]}"; do
    IFS='|' read -r scenario marker _mode expected <<<"$row"
    if [[ -z "$scenario" || "$scenario" == \#* ]]; then
        continue
    fi
    if ! grep -qF "$marker" qemu-hv-hostile.log; then
        echo "FAIL: hostile probe did not emit expected marker: $marker ($scenario)"
        dump_log
        exit 1
    fi
done

if [ "$BUDGET_LIVENESS" -eq 1 ]; then
    echo "OBSERVED: outer QEMU remained live after the vCPU-budget stimulus."
fi
if [ "$RESET_GUEST_EXIT_OBSERVED" -eq 1 ]; then
    echo "OBSERVED: nested VMM guest exited after the guest reset stimulus."
else
    echo "UNOBSERVED: guest reset stimulus produced no nested-VMM exit or supervisor restart."
fi
echo "BLOCKED_SCOPE: bounds, descriptor, and backend lack guest-visible VMM/VirtIO transport; VMM preemption and supervisor restart remain unobserved."
exit 2
