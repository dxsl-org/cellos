#!/usr/bin/env bash
# Build bootable Cellos SD card image for Raspberry Pi 3 (BCM2837).
# Uses mtools for unprivileged FAT32 partition creation (no root / losetup needed).

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUTPUT="${1:-disk_rpi3.img}"
FIRMWARE="tools/rpi3-firmware"
CELL_DIR="target/aarch64-unknown-none-softfloat/release"
RAW_KERNEL="kernel8.img"

echo "[rpi3] Building $OUTPUT with mtools..."

# 1. Ensure kernel8.img is built and up to date
if [ ! -f "$RAW_KERNEL" ] || [ "$CELL_DIR/cellos-kernel" -nt "$RAW_KERNEL" ]; then
    echo "[rpi3] Generating raw kernel8.img from cellos-kernel..."
    aarch64-linux-gnu-objcopy -O binary "$CELL_DIR/cellos-kernel" "$RAW_KERNEL"
fi

# 2. Build Partition 1: BOOT (FAT32, 256 MiB)
BOOT_IMG=$(mktemp)
dd if=/dev/zero of="$BOOT_IMG" bs=1M count=256 status=none
mformat -i "$BOOT_IMG" -F -v "CELLOS-BOOT" ::

mcopy -i "$BOOT_IMG" "$FIRMWARE/bootcode.bin" ::
mcopy -i "$BOOT_IMG" "$FIRMWARE/start.elf" ::
mcopy -i "$BOOT_IMG" "$FIRMWARE/fixup.dat" ::
mcopy -i "$BOOT_IMG" "$FIRMWARE/config.txt" ::
if [ -f "$FIRMWARE/bcm2710-rpi-3-b.dtb" ]; then
    mcopy -i "$BOOT_IMG" "$FIRMWARE/bcm2710-rpi-3-b.dtb" ::
fi
mcopy -i "$BOOT_IMG" "$RAW_KERNEL" ::kernel8.img
echo "[rpi3]   P1 (BOOT): VideoCore firmware + kernel8.img"

# 3. Build Partition 2: CELL (FAT32, 256 MiB)
DATA_IMG=$(mktemp)
dd if=/dev/zero of="$DATA_IMG" bs=1M count=256 status=none
mformat -i "$DATA_IMG" -F -v "CELLOS-CELL" ::

CELLS=(
    "desktop"
    "ocel"
    "ocel-js"
    "app-shell"
    "service-compositor"
    "service-vfs"
    "service-net"
    "service-input"
    "service-config"
    "service-power"
    "supervisor"
    "driver-gpio-bcm"
)

for c in "${CELLS[@]}"; do
    if [ -f "$CELL_DIR/$c" ]; then
        mcopy -i "$DATA_IMG" "$CELL_DIR/$c" "::$c"
        echo "[rpi3]   P2 (CELL): $c"
    fi
done

# 4. Construct final MBR disk image (512 MiB total)
# 4. Construct final MBR disk image (512 MiB total = 1,050,624 sectors)
rm -f "$OUTPUT"
dd if=/dev/zero of="$OUTPUT" bs=1M count=512 status=none

# Write MBR partition table (P1 @ 2048, P2 @ 526336)
python3 tools/write-rpi3-mbr.py "$OUTPUT"

# Write partitions into the disk image
dd if="$BOOT_IMG" of="$OUTPUT" bs=512 seek=2048 conv=notrunc status=none
dd if="$DATA_IMG" of="$OUTPUT" bs=512 seek=526336 conv=notrunc status=none

rm -f "$BOOT_IMG" "$DATA_IMG"

echo "[rpi3] Successfully created $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
echo "[rpi3] Flash to SD card: sudo dd if=$OUTPUT of=/dev/sdX bs=4M status=progress conv=fsync"
