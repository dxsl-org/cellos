#!/usr/bin/env bash
# Run Tier 3 Hostile QEMU scenarios for ARM64 and parse the result.

# Environment:
#   BOOT_WINDOW     seconds to wait (default: 60)
#   QEMU_ARM64_BIN  emulator executable (default: qemu-system-aarch64)
set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
DISK="${2:-disk_hv_arm.img}"
BOOT_WINDOW="${BOOT_WINDOW:-60}"
QEMU_ARM64_BIN="${QEMU_ARM64_BIN:-qemu-system-aarch64}"
QEMU_KERNEL="$KERNEL"
QEMU_DISK="$DISK"
if [[ "${QEMU_ARM64_BIN,,}" == *.exe ]]; then
    QEMU_KERNEL="$(wslpath -w "$KERNEL")"
    QEMU_DISK="$(wslpath -w "$DISK")"
fi

if ! command -v "$QEMU_ARM64_BIN" &>/dev/null; then
    echo "BLOCKED_ENVIRONMENT: $QEMU_ARM64_BIN not found"
    exit 1
fi

CORPUS_FILE="$(dirname "$0")/tier3-hostile-scenario-matrix.sh"
if [[ ! -f "$CORPUS_FILE" ]]; then
    echo "BLOCKED_ENVIRONMENT: scenario matrix file not found: $CORPUS_FILE"
    exit 1
fi
source "$CORPUS_FILE"

QEMU_VERSION_TEXT="$("$QEMU_ARM64_BIN" --version | sed -n '1p')"
if [[ ! "$QEMU_VERSION_TEXT" =~ QEMU\ emulator\ version\ ([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    echo "BLOCKED_ENVIRONMENT: cannot parse QEMU version: $QEMU_VERSION_TEXT"
    exit 1
fi
QEMU_MAJOR="${BASH_REMATCH[1]}"
QEMU_MINOR="${BASH_REMATCH[2]}"
QEMU_PATCH="${BASH_REMATCH[3]}"
if (( QEMU_MAJOR != 10 || QEMU_MINOR != 2 || QEMU_PATCH != 0 )); then
    echo "BLOCKED_ENVIRONMENT: require QEMU 10.2.0 for strict ARM64 hostile-path evidence, found: $QEMU_VERSION_TEXT"
    exit 1
fi
echo "[hv-hostile-arm64] kernel=$KERNEL disk=$DISK (window=${BOOT_WINDOW}s)"
echo "[hv-hostile-arm64] $QEMU_VERSION_TEXT"



# 0. Build the Hostile image if requested.
if [ ! -f "$KERNEL" ] || [ "${BUILD_HOSTILE_IMAGE:-0}" == "1" ]; then
    echo "Building hostile image..."
    bash scripts/prepare-tier3-hostile-initramfs-arm64.sh
    INITRD_OVERRIDE="build/tier3-hostile-initramfs-arm64.cpio.gz" bash scripts/make-hypervisor-fs.sh --skip-fetch
    RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" EMBEDDED_OVERRIDE="kernel/src/embedded-hv" cargo build --release -p cellos-kernel --features qemu-virt-1g --target aarch64-unknown-none-softfloat
    bash scripts/format-disk-arm.sh "$DISK"
fi

if [[ ! -f "$KERNEL" ]] || [[ ! -f "$DISK" ]]; then
    echo "BLOCKED_ENVIRONMENT: KERNEL or DISK not found"
    exit 1
fi

# Run QEMU in background.
"$QEMU_ARM64_BIN" \
    -machine "virt,virtualization=on,gic-version=2" \
    -cpu cortex-a72 \
    -m 1G \
    -nographic \
    -kernel "$QEMU_KERNEL" \
    -drive "if=none,file=$QEMU_DISK,format=raw,id=hd0" \
    -device virtio-blk-device,drive=hd0 \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 \
    -no-reboot \
    < /dev/null > qemu-hv-hostile-arm64.raw.log 2>&1 &
QEMU_PID=$!

# Observe the runnable probe stimuli when the guest reaches userspace, while
# also terminating promptly on the known TCG fault.
BUDGET_WINDOW="${BUDGET_WINDOW:-1}"
BUDGET_STARTED_AT=""
BUDGET_LIVENESS=0
RESET_STARTED_AT=""
RESET_GUEST_EXIT_OBSERVED=0
end_time=$((SECONDS + BOOT_WINDOW))
while [[ $SECONDS -lt $end_time ]]; do
    if grep -q "\[HOSTILE_PROBE\] BUDGET_TEST_STARTED" qemu-hv-hostile-arm64.raw.log 2>/dev/null; then
        if [[ -z "$BUDGET_STARTED_AT" ]]; then
            BUDGET_STARTED_AT=$SECONDS
        elif (( SECONDS - BUDGET_STARTED_AT >= BUDGET_WINDOW )) && kill -0 "$QEMU_PID" 2>/dev/null; then
            BUDGET_LIVENESS=1
        fi
    fi
    if grep -q "\[HOSTILE_PROBE\] RESET_TEST_STARTED" qemu-hv-hostile-arm64.raw.log 2>/dev/null; then
        if [[ -z "$RESET_STARTED_AT" ]]; then
            RESET_STARTED_AT=$SECONDS
        elif grep -q "\[hv\] guest exited" qemu-hv-hostile-arm64.raw.log 2>/dev/null; then
            RESET_GUEST_EXIT_OBSERVED=1
            break
        elif (( SECONDS - RESET_STARTED_AT >= 3 )); then
            break
        fi
    fi
    if grep -q "unknown vmexit ec=0x20 iss=0x6 pc=0x200\|KERNEL PANIC\|\[fault\] Cell" qemu-hv-hostile-arm64.raw.log 2>/dev/null \
        || ! kill -0 "$QEMU_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done

kill $QEMU_PID 2>/dev/null || true
wait $QEMU_PID 2>/dev/null || true

# Strip NULs and ANSI escapes for the parsed log.
tr -d '\000' < qemu-hv-hostile-arm64.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-hv-hostile-arm64.log

LOG_TAIL="${LOG_TAIL:-200}"

dump_log() {
    echo "--- qemu-hv-hostile-arm64.log tail ---"
    tail -n "$LOG_TAIL" qemu-hv-hostile-arm64.log
    echo "--------------------------------"
}

# 1. Check for hard environment failures / fatal panics.
if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-hv-hostile-arm64.log; then
    echo "FAIL: kernel panic or cell fault detected."
    dump_log
    exit 1
fi

if grep -qi "\[hv\] .*fail\|\[hv\] .*error" qemu-hv-hostile-arm64.log; then
    echo "FAIL: hypervisor cell error detected."
    dump_log
    exit 1
fi

# ARM64 under TCG hits a known spurious address-size fault during early boot.
# The guest OS never reaches userspace, so no hostile axis can execute.
LIVENESS_MARKER='[hv] vCPU ready'
if ! grep -qF "$LIVENESS_MARKER" qemu-hv-hostile-arm64.log; then
    echo "FAIL: liveness marker '$LIVENESS_MARKER' not seen — VMM never brought the guest up"
    dump_log
    exit 1
fi

TOLERATED_VMEXIT='unknown vmexit ec=0x20 iss=0x6 pc=0x200'
guest_fault_is_address_size() {
    local esr value
    while read -r esr; do
        value=$((esr))
        (( (value >> 26 & 0x3f) == 0x25 )) || continue
        (( (value & 0x3f) <= 0x03 )) || continue
        return 0
    done < <(grep -o 'ESR_EL1=0x[0-9a-fA-F]\+' qemu-hv-hostile-arm64.log | cut -d= -f2)
    return 1
}

if grep -qF "$TOLERATED_VMEXIT" qemu-hv-hostile-arm64.log && guest_fault_is_address_size; then
    echo "BLOCKED_ENVIRONMENT: VMM liveness reached; the known TCG address-size fault prevents required hostile payload execution."
    exit 2
fi

if grep -qF "[HOSTILE_PROBE] Starting Hostile Probe" qemu-hv-hostile-arm64.log \
    || grep -qF "Starting Hostile Probe..." qemu-hv-hostile-arm64.log; then
    for row in "${TIER3_HOSTILE_CORPUS[@]}"; do
        IFS='|' read -r scenario marker _mode expected <<<"$row"
        if [[ -z "$scenario" || "$scenario" == \#* ]]; then
            continue
        fi
        if ! grep -qF "$marker" qemu-hv-hostile-arm64.log; then
            echo "FAIL: hostile probe marker missing: $marker"
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
else
    echo "FAIL: guest neither hit the expected TCG fault nor reached the hostile probe."
    dump_log
    exit 1
fi
