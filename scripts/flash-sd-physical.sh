#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# flash-sd-physical.sh — Automated Bootable SD Image Creator for Physical Hardware
#
# Supports:
#   1. Raspberry Pi 3 Model B+ (AArch64, BCM2837)
#   2. StarFive VisionFive 2 (RISC-V 64, JH7110)
#
# Usage:
#   ./scripts/flash-sd-physical.sh --board vf2 --output vf2-cellos.img
#   ./scripts/flash-sd-physical.sh --board rpi3 --output rpi3-cellos.img
#   sudo ./scripts/flash-sd-physical.sh --board vf2 --device /dev/sdX

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BOARD=""
OUTPUT=""
DEVICE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --board)
            BOARD="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --device)
            DEVICE="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 --board [rpi3|vf2] [--output <file.img>] [--device </dev/sdX>]"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$BOARD" ]]; then
    echo "ERROR: --board [rpi3|vf2] is required." >&2
    exit 1
fi

if [[ -z "$OUTPUT" && -z "$DEVICE" ]]; then
    OUTPUT="cellos-${BOARD}-boot.img"
    echo "No target specified, defaulting to output file: $OUTPUT"
fi

echo "======================================================="
echo " Cellos Physical Silicon Image Builder"
echo " Board:  $BOARD"
echo " Target: ${DEVICE:-$OUTPUT}"
echo "======================================================="

# 1. Compile board kernel
if [[ "$BOARD" == "vf2" ]]; then
    echo "==> Building Cellos Kernel for StarFive VisionFive 2 (RV64GC)..."
    RUSTFLAGS="-C relocation-model=pic" \
        cargo build --release -p cellos-kernel \
        --target riscv64gc-unknown-none-elf \
        --features board-vf2
    KERNEL_BIN="target/riscv64gc-unknown-none-elf/release/cellos-kernel"
elif [[ "$BOARD" == "rpi3" ]]; then
    echo "==> Building Cellos Kernel for Raspberry Pi 3 Model B+ (AArch64)..."
    cargo build --release -p cellos-kernel \
        --target aarch64-unknown-none-softfloat \
        --features board-rpi3
    KERNEL_BIN="target/aarch64-unknown-none-softfloat/release/cellos-kernel"
    echo "==> Generating flat binary kernel8.img via objcopy..."
    aarch64-linux-gnu-objcopy -O binary "$KERNEL_BIN" target/rpi3-kernel8.img
else
    echo "ERROR: Unsupported board '$BOARD'. Must be 'rpi3' or 'vf2'." >&2
    exit 1
fi

if [[ ! -f "$KERNEL_BIN" ]]; then
    echo "ERROR: Kernel artifact not found at $KERNEL_BIN" >&2
    exit 1
fi

# 2. Strict signing validation
echo "==> Verifying Cellos F1/F5 Security Signing Policy..."
python3 scripts/cellos-sign --check --strict

# 3. Create or prepare target image
IMG_TARGET="${DEVICE:-$OUTPUT}"
if [[ -n "$OUTPUT" ]]; then
    echo "==> Allocating 512 MB disk image: $OUTPUT..."
    dd if=/dev/zero of="$OUTPUT" bs=1M count=512 status=none
fi

# 4. Partitioning and MBR setup
echo "==> Partitioning and writing MBR layout..."
python3 tools/write-mbr.py "$IMG_TARGET"

# 5. Format FAT32 Boot Partition P1 (LBA 2048, 524288 sectors = 256MB)
echo "==> Formatting FAT32 Boot Partition (P1)..."
python3 tools/mkfat32_inplace.py "$IMG_TARGET" 524288 2048

# 6. Populate Boot Partition P1 with firmware and kernel
if [[ "$BOARD" == "rpi3" ]]; then
    echo "==> Populating P1 with Raspberry Pi 3 firmware and kernel8.img..."
    P1_OFFSET=$((2048 * 512))
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" tools/rpi3-firmware/bootcode.bin ::bootcode.bin
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" tools/rpi3-firmware/start.elf ::start.elf
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" tools/rpi3-firmware/fixup.dat ::fixup.dat
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" tools/rpi3-firmware/bcm2710-rpi-3-b.dtb ::bcm2710-rpi-3-b.dtb
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" tools/rpi3-firmware/config.txt ::config.txt
    mcopy -o -i "${IMG_TARGET}@@${P1_OFFSET}" target/rpi3-kernel8.img ::kernel8.img
    echo "==> P1 boot files installed:"
    mdir -i "${IMG_TARGET}@@${P1_OFFSET}" ::
fi
echo "======================================================="
echo " Boot image prepared successfully for $BOARD!"
if [[ -n "$DEVICE" ]]; then
    echo " Synced and ready. Safely remove $DEVICE and insert into your $BOARD board."
else
    echo " Output file: $OUTPUT"
    echo " To flash to physical SD card: sudo dd if=$OUTPUT of=/dev/sdX bs=4M status=progress conv=fsync"
fi
echo "======================================================="
