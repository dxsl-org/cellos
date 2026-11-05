#!/usr/bin/env bash
# Boot the Cellos AArch64 kernel in QEMU and assert the system reaches the shell
# prompt ("Cellos >").
#
# Mirrors scripts/qemu-boot-test.sh for the ARM64 virt machine.
#
# Usage: scripts/qemu-aarch64-test.sh [kernel] [disk.img]
#   kernel      default: target/aarch64-unknown-none-softfloat/release/cellos-kernel
#               An ELF or a raw arm64 `Image` (scripts/build-aarch64-image.sh);
#               QEMU passes its generated FDT in x0 to an Image.
#   disk.img    default: disk_arm_virt.img
#
# Environment:
#   LANE        virt (default): non-VHE kernel, `-machine virt`, cortex-a57, 1 CPU.
#               virt-vhe: a kernel built with CELLOS_AARCH64_HOST_REGIME=vhe on
#               `virt,virtualization=on` with a VHE CPU and 2 CPUs. Asserts the
#               VHE host regime line, EL0 Cell output and, with more than one
#               CPU, hart 1 online.
#   QEMU_CPU    override the lane's CPU (virt-vhe: cortex-a76)
#   QEMU_SMP    override the lane's CPU count (virt-vhe: 2)

set -euo pipefail

KERNEL="${1:-target/aarch64-unknown-none-softfloat/release/cellos-kernel}"
DISK="${2:-disk_arm_virt.img}"
BOOT_WINDOW="${BOOT_WINDOW:-55}"
ASSERT_PHASE04_BASELINE="${ASSERT_PHASE04_BASELINE:-0}"
OMIT_OPTIONAL_DEVICES="${OMIT_OPTIONAL_DEVICES:-0}"
MARKER_HELPER="$(dirname "$0")/assert-boot-markers.sh"
LANE="${LANE:-virt}"
case "$LANE" in
    virt)
        QEMU_MACHINE="virt"
        QEMU_CPU="${QEMU_CPU:-cortex-a57}"
        QEMU_SMP="${QEMU_SMP:-1}"
        ;;
    virt-vhe)
        # gic-version=2: the EL2 host's vGIC uses GICv2 GICH/GICV.
        QEMU_MACHINE="virt,virtualization=on,gic-version=2"
        QEMU_CPU="${QEMU_CPU:-cortex-a76}"
        QEMU_SMP="${QEMU_SMP:-2}"
        ;;
    *)
        echo "FAIL: LANE must be 'virt' or 'virt-vhe' (got '$LANE')" >&2
        exit 1
        ;;
esac

if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "FAIL: qemu-system-aarch64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: kernel ELF not found: $KERNEL" >&2
    echo "  Build with: RUSTFLAGS='-C relocation-model=pic' cargo build --release --target aarch64-unknown-none-softfloat -p cellos-kernel" >&2
    exit 1
fi

if [[ ! -f "$DISK" ]]; then
    echo "FAIL: disk image not found: $DISK" >&2
    echo "  Build with: .\\format-disk-arm.ps1  (or tools/mkfat32.py)" >&2
    exit 1
fi

echo "[qemu-aarch64-test] lane=$LANE Booting kernel=$KERNEL disk=$DISK (cpu=$QEMU_CPU smp=$QEMU_SMP window=${BOOT_WINDOW}s)"

QEMU_ARGS=(
    -machine "$QEMU_MACHINE"
    -cpu "$QEMU_CPU"
    -smp "$QEMU_SMP"
    -m 256M
    -nographic
    -kernel "$KERNEL"
    -drive "if=none,file=$DISK,format=raw,id=hd0"
    -device virtio-blk-device,drive=hd0
    -device virtio-keyboard-device
    -no-reboot
)
if [[ "$OMIT_OPTIONAL_DEVICES" != "1" ]]; then
    QEMU_ARGS+=(
        -netdev user,id=net0
        -device virtio-net-device,netdev=net0
        -device virtio-gpu-device
    )
fi
# Add VirtIO RNG only when /dev/random is available (Linux CI).
# On Windows QEMU 10, rng-random is invalid; skip it — boot tests don't need RNG.
if [[ -c /dev/random ]]; then
    QEMU_ARGS+=(-object rng-random,id=rng0 -device virtio-rng-device,rng=rng0)
fi

timeout "$BOOT_WINDOW" qemu-system-aarch64 "${QEMU_ARGS[@]}" \
    < /dev/null > qemu-aarch64.raw.log 2>&1 || true

# Strip NULs and ANSI escape sequences so patterns match cleanly.
tr -d '\000' < qemu-aarch64.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-aarch64.log

if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-aarch64.log; then
    echo "FAIL: kernel panic / cell fault detected during aarch64 boot" >&2
    grep -ai "fault\|PANIC" qemu-aarch64.log | head
    exit 1
fi

if grep -q "Cellos >" qemu-aarch64.log; then
    if [[ "$ASSERT_PHASE04_BASELINE" == "1" ]]; then
        REQUIRED_MARKERS=(
            "block registration:::[driver_cell] block driver registered" \
            "input registration:::[input] registered input service TID"
        )
        if [[ "$OMIT_OPTIONAL_DEVICES" != "1" ]]; then
            REQUIRED_MARKERS+=("gpu readiness:::VirtIO GPU Driver Cell registered")
        fi
        bash "$MARKER_HELPER" qemu-aarch64.log "aarch64-qemu" "${REQUIRED_MARKERS[@]}"
    fi
    if [[ "$LANE" == "virt-vhe" ]]; then
        # "USER:" lines are EL0 Cells writing through the console syscall.
        VHE_MARKERS=(
            "VHE host regime:::[INFO] host regime: EL2 VHE (E2H=1 TGE=1)"
            "EL0 Cell syscalls:::USER: Init: OS ready."
        )
        if [[ "$QEMU_SMP" -gt 1 ]]; then
            VHE_MARKERS+=("secondary hart:::[smp] hart 1 online")
        fi
        bash "$MARKER_HELPER" qemu-aarch64.log "aarch64-virt-vhe" "${VHE_MARKERS[@]}"
    fi
    echo "PASS: aarch64 shell prompt reached — full boot successful"
    exit 0
fi

echo "FAIL: 'Cellos >' prompt not seen within ${BOOT_WINDOW}s" >&2
tail -40 qemu-aarch64.log
exit 1
