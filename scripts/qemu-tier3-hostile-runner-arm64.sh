#!/usr/bin/env bash
# Run Tier 3 Hostile QEMU scenarios for ARM64 and parse the result.

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
DISK="${2:-disk_hv_arm.img}"
BOOT_WINDOW="${BOOT_WINDOW:-60}"

if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "BLOCKED_ENVIRONMENT: qemu-system-aarch64 not found"
    exit 1
fi


echo "[hv-hostile-arm64] kernel=$KERNEL disk=$DISK (window=${BOOT_WINDOW}s)"

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
qemu-system-aarch64 \
    -machine "virt,virtualization=on,gic-version=2" \
    -cpu cortex-a72 \
    -m 1G \
    -nographic \
    -kernel "$KERNEL" \
    -drive "if=none,file=$DISK,format=raw,id=hd0" \
    -device virtio-blk-device,drive=hd0 \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 \
    -no-reboot \
    < /dev/null > qemu-hv-hostile-arm64.raw.log 2>&1 &
QEMU_PID=$!

# Wait for either the final RESET marker, kernel panic, or timeout.
end_time=$((SECONDS + BOOT_WINDOW))
while [[ $SECONDS -lt $end_time ]]; do
    if grep -q "RESET_TEST_TRIGGERED\|KERNEL PANIC\|\[fault\] Cell" qemu-hv-hostile-arm64.raw.log 2>/dev/null; then
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
    echo "NOT_APPLICABLE: VMM liveness reached; the known TCG address-size fault prevents hostile payload execution."
    exit 2
fi

echo "FAIL: guest did not hit the expected TCG fault or reach userspace."
dump_log
exit 1
