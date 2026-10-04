#!/usr/bin/env bash
# Boot the ViCell x86_64 kernel in QEMU q35 (Limine BIOS ISO) and assert the
# system reaches the interactive shell prompt ("Cellos >").
#
# Mirrors scripts/qemu-aarch64-test.sh for the x86_64 q35 machine.
#
# Usage: BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh [iso]
#   iso   default: build/vicell-x86.iso
# Optional: X86_NIC_MODEL=e1000e runs the fail-closed NIC identity check.

set -euo pipefail

ISO="${1:-build/vicell-x86.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"
X86_NIC_MODEL="${X86_NIC_MODEL:-}"
# Optional raw SATA image for the q35 ICH9 AHCI controller (phase 02a). When
# set, the same file is attached as `-device ide-hd`, which is how the AHCI
# Driver Cell gets a disk to IDENTIFY. Unset keeps the pre-AHCI device set.
X86_SATA_IMAGE="${X86_SATA_IMAGE:-}"
# CPU model decides whether the guest has PCID at all: `qemu64` does not, `max`
# does. The lane asserts the kernel's own decision when X86_EXPECT_PCID is set,
# so the PCID-off and PCID-on paths are both witnessed rather than assumed.
X86_CPU_MODEL="${X86_CPU_MODEL:-qemu64,+pdpe1gb}"
X86_EXPECT_PCID="${X86_EXPECT_PCID:-}"
# TCG cannot emulate PCID (QEMU clears the feature with a warning), so the
# PCID-on path needs hardware acceleration: X86_ACCEL=kvm X86_CPU_MODEL=host.
X86_ACCEL="${X86_ACCEL:-}"

case "$X86_ACCEL" in
    ""|tcg|kvm) ;;
    *)
        echo "FAIL: X86_ACCEL must be empty, tcg, or kvm" >&2
        exit 1
        ;;
esac

case "$X86_EXPECT_PCID" in
    ""|0|1) ;;
    *)
        echo "FAIL: X86_EXPECT_PCID must be empty, 0, or 1" >&2
        exit 1
        ;;
esac

case "$X86_NIC_MODEL" in
    "") NIC_ARGS=() ;;
    e1000|e1000e) NIC_ARGS=(-device "$X86_NIC_MODEL") ;;
    *)
        echo "FAIL: X86_NIC_MODEL must be empty, e1000, or e1000e" >&2
        exit 1
        ;;
esac

if [[ -n "$X86_SATA_IMAGE" ]]; then
    if [[ ! -f "$X86_SATA_IMAGE" ]]; then
        echo "FAIL: X86_SATA_IMAGE not found: $X86_SATA_IMAGE" >&2
        exit 1
    fi
    SATA_ARGS=(-drive "file=$X86_SATA_IMAGE,if=none,id=sata0,format=raw" -device "ide-hd,drive=sata0")
else
    SATA_ARGS=()
fi

if ! command -v qemu-system-x86_64 &>/dev/null; then
    echo "FAIL: qemu-system-x86_64 not found on PATH" >&2
    exit 1
fi

if [[ ! -f "$ISO" ]]; then
    echo "FAIL: ISO not found: $ISO" >&2
    echo "  Build with: cargo build --release -p cellos-kernel --target x86_64-unknown-none && bash scripts/x86/make-iso-ci.sh" >&2
    exit 1
fi

echo "[qemu-x86_64-test] Booting ISO=$ISO (window=${BOOT_WINDOW}s)"

ACCEL_ARGS=()
if [[ -n "$X86_ACCEL" ]]; then
    ACCEL_ARGS=(-accel "$X86_ACCEL")
fi

timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    "${NIC_ARGS[@]}" \
    "${SATA_ARGS[@]}" \
    < /dev/null > qemu-x86_64.raw.log 2>&1 || true

# Strip NULs and ANSI escape sequences so patterns match cleanly.
tr -d '\000' < qemu-x86_64.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu-x86_64.log

if grep -qia "KERNEL PANIC\|\[fault\] Cell" qemu-x86_64.log; then
    echo "FAIL: kernel panic / cell fault detected during x86_64 boot" >&2
    grep -ai "fault\|PANIC" qemu-x86_64.log | head
    exit 1
fi

if [[ "$X86_NIC_MODEL" == "e1000e" ]] \
    && ! grep -q "\[e1000\] unsupported Ethernet 8086:10d3; driver gate closed" qemu-x86_64.log; then
    echo "FAIL: e1000e endpoint was not rejected by vendor/device ID" >&2
    exit 1
fi

# Phase 02a: with a SATA image attached, the AHCI Driver Cell must bind the ICH9
# controller, bring a port up, and complete IDENTIFY DEVICE before the shell.
if [[ -n "$X86_SATA_IMAGE" ]]; then
    if ! grep -qa "\[ahci\] controller bound" qemu-x86_64.log; then
        echo "FAIL: AHCI Driver Cell did not bind the SATA controller" >&2
        grep -ai "ahci" qemu-x86_64.log | head -5
        exit 1
    fi
    if ! grep -qa "\[ahci\] IDENTIFY DEVICE ok" qemu-x86_64.log; then
        echo "FAIL: AHCI Driver Cell did not complete IDENTIFY DEVICE" >&2
        grep -ai "ahci" qemu-x86_64.log | head -10
        exit 1
    fi
    if ! grep -qa "\[driver_cell\] ahci storage driver ready" qemu-x86_64.log; then
        echo "FAIL: AHCI Driver Cell did not report ready" >&2
        exit 1
    fi
fi

# Phase 02, the other half of the x86 domain gate. This is the *production* image
# (`native-domains` on, `test-hooks` off), so `switch_ordering_qualified()` is
# false, the boot must say so, and a domain-class artifact must be denied rather
# than silently downgraded to the shared address space. The test image that does
# admit a real Tier-2 cell is scripts/x86/qemu-domain-test.sh; the two lanes must
# keep disagreeing about this build.
if ! grep -qa "Tier 2 admission: DISABLED" qemu-x86_64.log; then
    echo "FAIL: production x86_64 image did not report a disabled Tier 2 admission posture" >&2
    grep -ai "Tier 2 admission" qemu-x86_64.log | head -3
    tail -40 qemu-x86_64.log
    exit 1
fi
if grep -qa "Tier 2 admission: ENABLED" qemu-x86_64.log; then
    echo "FAIL: production x86_64 image enabled Tier 2 admission" >&2
    exit 1
fi

# The PCID decision is the kernel's, read from CPUID and CR4 at boot: assert it
# in both directions so "PCID on" and "PCID off" are each a real observation.
if [[ "$X86_EXPECT_PCID" == "1" ]] \
    && ! grep -q "x86_64 paging: PCID enabled" qemu-x86_64.log; then
    echo "FAIL: expected PCID to be enabled on cpu '$X86_CPU_MODEL'" >&2
    grep -ai "paging:" qemu-x86_64.log | head -3
    exit 1
fi
if [[ "$X86_EXPECT_PCID" == "0" ]] \
    && ! grep -q "x86_64 paging: PCID disabled" qemu-x86_64.log; then
    echo "FAIL: expected PCID to be disabled on cpu '$X86_CPU_MODEL'" >&2
    grep -ai "paging:" qemu-x86_64.log | head -3
    exit 1
fi

if grep -q "Cellos >" qemu-x86_64.log; then
    echo "PASS: x86_64 shell prompt reached — full boot successful"
    exit 0
fi

echo "FAIL: 'Cellos >' prompt not seen within ${BOOT_WINDOW}s" >&2
tail -40 qemu-x86_64.log
exit 1
