#!/usr/bin/env bash
# Create disk_arm_virt.img for AArch64 QEMU & RPi3 boot testing.
#
# Generates a partitioned disk image:
#   MBR: LBA 0 (tools/write-mbr.py)
#   P1:  LBA 2048, 256 MB FAT32 interop volume (/mnt/sd, /bin)
#   P5:  LBA 931072, 64 MB RedoxFS persistent volume (/srv)
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
FULL_SECTORS=$((PART_SRV_BASE_LBA + PART_SRV_SECTORS)) # 1_062_144 sectors (~519 MB sparse)

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
)

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

# ---------- 2. Build P5 RedoxFS Partition Image ----------
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
REDOXFS_TARGET_DIR="third_party/redoxfs/target"
REDOXFS_AR="$REDOXFS_TARGET_DIR/$HOST_TRIPLE/release/redoxfs-ar"
[[ -x "$REDOXFS_AR" ]] || REDOXFS_AR="$REDOXFS_TARGET_DIR/release/redoxfs-ar"
if [[ ! -x "$REDOXFS_AR" ]]; then
    echo "[format-disk-arm] Building redoxfs-ar (host $HOST_TRIPLE)..."
    cargo build \
        --manifest-path third_party/redoxfs/Cargo.toml \
        --features std --release --bin redoxfs-ar \
        --target "$HOST_TRIPLE" \
        --target-dir "$REDOXFS_TARGET_DIR"
    REDOXFS_AR="$REDOXFS_TARGET_DIR/$HOST_TRIPLE/release/redoxfs-ar"
fi

P5_IMG="$TMPDIR_WORK/p5_redoxfs.img"
SEED_DIR="$TMPDIR_WORK/seed"
mkdir -p "$SEED_DIR"
printf 'ViCell RedoxFS /srv persistent storage on RPi3\n' > "$SEED_DIR/hello.txt"
dd if=/dev/zero of="$P5_IMG" bs=512 count="$PART_SRV_SECTORS" status=none
"$REDOXFS_AR" "$P5_IMG" "$SEED_DIR"
truncate -s "$((PART_SRV_SECTORS * 512))" "$P5_IMG"
echo "[format-disk-arm] P5 RedoxFS formatted (64 MB)"

# ---------- 3. Assemble Full Sparse MBR Disk Image ----------
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
python3 tools/write-mbr.py "$OUT" >/dev/null

# Splice P1 at LBA 2048
dd if="$P1_IMG" of="$OUT" bs=512 seek="$PART_FAT32_BASE_LBA" conv=notrunc status=none
# Splice P5 at LBA 931072
dd if="$P5_IMG" of="$OUT" bs=512 seek="$PART_SRV_BASE_LBA" conv=notrunc status=none

echo "[format-disk-arm] Done: $OUT (MBR + P1 FAT32 + P5 RedoxFS)"
