#!/bin/bash
# Create 8MB FAT32 disk image with init and shell binaries

set -e

DISK_IMG="disk_8mb.img"
INIT_BIN="target/riscv64gc-unknown-none-elf/debug/app-init"
SHELL_BIN="target/riscv64gc-unknown-none-elf/debug/app-shell"

echo "Creating 8MB FAT32 image..."
# 8MB = 16384 sectors of 512 bytes
dd if=/dev/zero of="$DISK_IMG" bs=512 count=16384 2>/dev/null

echo "Formatting as FAT32..."
mkfs.fat -F 32 -n "VIOS_BOOT" "$DISK_IMG" >/dev/null

echo "Copying binaries..."
if [ -f "$INIT_BIN" ]; then
    mcopy -i "$DISK_IMG" "$INIT_BIN" ::/init
    echo "  ✓ Copied init"
else
    echo "  ✗ Warning: $INIT_BIN not found"
fi

if [ -f "$SHELL_BIN" ]; then
    mcopy -i "$DISK_IMG" "$SHELL_BIN" ::/shell
    echo "  ✓ Copied shell"
else
    echo "  ✗ Warning: $SHELL_BIN not found"
fi

echo "Listing disk contents:"
mdir -i "$DISK_IMG" ::/

echo ""
echo "✓ Created $DISK_IMG (8MB FAT32)"
ls -lh "$DISK_IMG"
