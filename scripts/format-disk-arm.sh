#!/usr/bin/env bash
# Create disk_arm_virt.img for AArch64 QEMU & RPi3 boot testing.
#
# Generates a partitioned disk image:
#   MBR: LBA 0 (tools/write-mbr.py)
#   P1:  LBA 2048, 256 MB FAT32 interop volume (/mnt/sd, /bin)
#   P5:  LBA 931072, 64 MB raw persistent partition (/srv; the VFS formats it as
#        CellosFS Native on first mount)
#
# Usage: bash scripts/format-disk-arm.sh [output.img]
#   output.img  default: disk_arm_virt.img

set -euo pipefail

OUT="${1:-disk_arm_virt.img}"
TARGET="aarch64-unknown-none-softfloat"
PROFILE="release"
BIN_DIR="target/$TARGET/$PROFILE"

PART_FAT32_BASE_LBA=2048
PART_FAT32_SECTORS=524288
PART_SRV_BASE_LBA=931072
PART_SRV_SECTORS=131072
PART_CELLSTORE_BASE_LBA=1062144
PART_CELLSTORE_SECTORS=65536
FULL_SECTORS=$((PART_CELLSTORE_BASE_LBA + PART_CELLSTORE_SECTORS)) # 1_127_680 sectors (~577 MB sparse)
echo "[format-disk-arm] Output: $OUT"
echo "[format-disk-arm] Collecting cell binaries from $BIN_DIR..."

declare -A CELLS=(
    [app-init]=init
    [app-shell]=shell
    [service-vfs]=vfs
    [service-config]=config
    [service-net]=net
    [service-input]=input
    [service-compositor]=compositor
    [robot-demo]=robot-demo
    [periph-demo]=periph-demo
    [periph-test]=periph-test
    [sensor-demo]=sensor-demo
    [spi-demo]=spi-demo
    [pwm-demo]=pwm-demo
    [adc-demo]=adc-demo
    [can-demo]=can-demo
)

# shellcheck source=scripts/lib-sign-cells.sh
source "$(dirname "$0")/lib-sign-cells.sh"
SIGN_LIST=()
for src_name in "${!CELLS[@]}"; do
    if [[ -f "$BIN_DIR/$src_name" ]]; then
        SIGN_LIST+=("$BIN_DIR/$src_name")
    fi
done
if [[ ${#SIGN_LIST[@]} -gt 0 ]]; then
    echo "[format-disk-arm] Signing aarch64 cells..."
    sign_cells "${SIGN_LIST[@]}"
fi
MKFAT_ARGS=()
for src_name in "${!CELLS[@]}"; do
    dst_name="${CELLS[$src_name]}"
    src="$BIN_DIR/$src_name"
    if [[ -f "$src" ]]; then
        echo "  /bin/$dst_name <- $src"
        MKFAT_ARGS+=("$src" "/bin/$dst_name")
    else
        echo "  WARNING: $src not found, skipping /bin/$dst_name"
    fi
done

# Include /etc/hostname
HOSTNAME_TMP=$(mktemp)
echo "ViCell-ARM" > "$HOSTNAME_TMP"
MKFAT_ARGS+=("$HOSTNAME_TMP" "/etc/hostname")

TMPDIR_WORK=$(mktemp -d)
trap 'rm -rf "$TMPDIR_WORK" "$HOSTNAME_TMP"' EXIT

# ---------- 1. Build P1 FAT32 Partition Image ----------
P1_IMG="$TMPDIR_WORK/p1_fat32.img"
echo "[format-disk-arm] Formatting P1 FAT32 with tools/mkfat32.py..."
python3 tools/mkfat32.py "$P1_IMG" "${MKFAT_ARGS[@]}"

# ---------- 2. Build P6 Cell-Store FAT Image (/bin) ----------
P6_IMG="$TMPDIR_WORK/cell_store.img"
CELLSTORE_ARGS=()
TABLE_ARGS=()
for src_name in "${!CELLS[@]}"; do
    dst_name="${CELLS[$src_name]}"
    src="$BIN_DIR/$src_name"
    if [[ -f "$src" ]]; then
        CELLSTORE_ARGS+=("$src" "/$dst_name")
        TABLE_ARGS+=("/bin/$dst_name=$src")
    fi
done
echo "[format-disk-arm] Formatting P6 FAT cell-store..."
python3 tools/mkfat32.py "$P6_IMG" "${CELLSTORE_ARGS[@]}"

# ---------- 3. Assemble Full Sparse MBR Disk Image ----------
rm -f "$OUT"
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
python3 tools/write-mbr.py "$OUT" >/dev/null

# Splice P1 at LBA 2048 and P6 at LBA 1062144
dd if="$P1_IMG" of="$OUT" bs=512 seek="$PART_FAT32_BASE_LBA" conv=notrunc status=none
dd if="$P6_IMG" of="$OUT" bs=512 seek="$PART_CELLSTORE_BASE_LBA" conv=notrunc status=none

# Write bootstrap cell table at LBA 526336
python3 tools/write-cell-table.py "$OUT" "${TABLE_ARGS[@]}"

echo "[format-disk-arm] Done: $OUT (MBR + P1 FAT32 + P5 raw/CellosFS + P6 cell-store)"
