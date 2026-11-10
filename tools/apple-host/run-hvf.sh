#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Run the Cellos AArch64 hosted-VM kernel on Apple Silicon macOS: QEMU `virt`
# with a GICv3 under the HVF accelerator.
#
# Build the kernel for the hosted VM backend and flatten it to an arm64 Image
# (an Image boot hands the kernel QEMU's FDT, from which it reads RAM, the GICv3
# windows and the PSCI conduit):
#   cargo build --release -p cellos-kernel --features board-aarch64-vm \
#       --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
#   bash scripts/build-aarch64-image.sh
#
# What the backend supports, and therefore what this launches:
#   * `gic-version=3`: the only interrupt controller the backend drives.
#   * `highmem=off`: keeps RAM, PCIe ECAM and the GIC redistributors below
#     4 GiB, inside the kernel's MMIO identity maps (HVF on M1/M2 also caps the
#     guest IPA size, which highmem layouts can exceed).
#   * virtio-mmio devices (`virtio-*-device`): the AArch64 driver cells use
#     QEMU virt's virtio-mmio slots, not virtio-pci.
#   * PL011 console on stdio (`-nographic`); the kernel reads no command line.
#   * At most 16 vCPUs: the kernel's hart index is MPIDR.Aff0, and QEMU virt
#     with a GICv3 numbers 16 CPUs per Aff1 cluster.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL="${1:-$ROOT/target/aarch64-unknown-none-softfloat/release/cellos-kernel.Image}"
DISK="${2:-$ROOT/disk_arm_virt.img}"
MEMORY="${QEMU_MEMORY:-1G}"
CPUS="${QEMU_SMP:-4}"

if [[ ! -f "$KERNEL" ]]; then
    echo "ERROR: kernel not found: $KERNEL (see the build steps at the top of $0)" >&2
    exit 1
fi
if (( CPUS > 16 )); then
    echo "ERROR: QEMU_SMP=$CPUS; the aarch64-vm kernel supports at most 16 vCPUs" >&2
    exit 1
fi

echo "==> Running Cellos (aarch64-vm) on Apple Silicon macOS via QEMU HVF..."
exec qemu-system-aarch64 \
    -M virt,gic-version=3,highmem=off \
    -accel hvf \
    -cpu host \
    -smp "$CPUS" \
    -m "$MEMORY" \
    -nographic \
    -kernel "$KERNEL" \
    -drive "file=$DISK,if=none,id=drive0,format=raw" \
    -device virtio-blk-device,drive=drive0 \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 \
    -device virtio-keyboard-device \
    -no-reboot
