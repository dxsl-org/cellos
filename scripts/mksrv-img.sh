#!/usr/bin/env bash
# mksrv-img.sh — Build the 519 MB sparse disk image used by the RedoxFS /srv
# integration test.  Partition P5 (LBA 931_072, 64 MB) is formatted as RedoxFS
# and seeded with hello.txt.  Partitions P1–P4 are zero-filled; the VFS service
# degrades gracefully when FAT32/littlefs fail to mount.
#
# Usage: bash scripts/mksrv-img.sh [OUT_IMG]
# Default output: build/disk_srv.img
#
# Prerequisites (installed by CI job):
#   - cargo (for building redoxfs-ar from third_party/redoxfs)
#   - Rust nightly with the host target available
#
# Disk layout (matches libs/api/src/disk.rs):
#   PART_SRV_BASE_LBA  = 931_072   sectors
#   PART_SRV_SECTORS   = 131_072   sectors (64 MB)
#   Full image         = 1_062_144 sectors (~519 MB sparse)

set -euo pipefail

OUT="${1:-build/disk_srv.img}"
PART_SRV_BASE_LBA=931072
PART_SRV_SECTORS=131072
FULL_SECTORS=$((PART_SRV_BASE_LBA + PART_SRV_SECTORS))   # 1_062_144

echo "[mksrv-img] Output: $OUT"
echo "[mksrv-img] Full disk: $FULL_SECTORS sectors ($(( FULL_SECTORS * 512 / 1024 / 1024 )) MB sparse)"
echo "[mksrv-img] P5 at LBA $PART_SRV_BASE_LBA, $PART_SRV_SECTORS sectors"

mkdir -p "$(dirname "$OUT")"

# ---------- Build redoxfs-ar from source (host target, std features) ----------
# The host triple must be named explicitly. .cargo/config.toml sets a bare-metal
# default target for the workspace, and scripts/dev-setup.sh writes that file, so
# an unqualified `cargo build` here compiles this std host tool for riscv64 and
# dies in the dependency tree with errors that mention neither the target nor this
# script. CI never saw it because that config is gitignored and absent there.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
REDOXFS_TARGET_DIR="third_party/redoxfs/target"
REDOXFS_AR="$REDOXFS_TARGET_DIR/$HOST_TRIPLE/release/redoxfs-ar"
# Older trees built without --target, which lands the binary one level up.
[[ -x "$REDOXFS_AR" ]] || REDOXFS_AR="$REDOXFS_TARGET_DIR/release/redoxfs-ar"
if [[ ! -x "$REDOXFS_AR" ]]; then
    echo "[mksrv-img] Building redoxfs-ar (host $HOST_TRIPLE, --features std)..."
    cargo build \
        --manifest-path third_party/redoxfs/Cargo.toml \
        --features std --release --bin redoxfs-ar \
        --target "$HOST_TRIPLE" \
        --target-dir "$REDOXFS_TARGET_DIR"
    REDOXFS_AR="$REDOXFS_TARGET_DIR/$HOST_TRIPLE/release/redoxfs-ar"
fi
echo "[mksrv-img] redoxfs-ar: $REDOXFS_AR"

# ---------- Create staging folder with seed files ----------------------------
SEED=$(mktemp -d)
trap 'rm -rf "$SEED"' EXIT
printf 'ViCell RedoxFS' > "$SEED/hello.txt"
echo "[mksrv-img] Seeded: hello.txt (14 bytes)"

# ---------- Create 64 MB RedoxFS partition image ----------------------------
# redoxfs-ar opens this file, formats it as RedoxFS, archives the seed folder
# into it, then truncates to the smallest usable size.  We restore the full
# partition size afterwards so dd can splice it at the right byte offset.
PART_IMG=$(mktemp --suffix=.img)
trap 'rm -f "$PART_IMG"' EXIT

# Pre-allocate the full partition so create_reserved has the space it needs.
dd if=/dev/zero of="$PART_IMG" bs=512 count="$PART_SRV_SECTORS" status=none
"$REDOXFS_AR" "$PART_IMG" "$SEED"
# redoxfs-ar truncated the file to fs size; restore to full partition size.
truncate -s "$((PART_SRV_SECTORS * 512))" "$PART_IMG"
echo "[mksrv-img] P5 partition image: $(du -sh "$PART_IMG" | cut -f1)"

# ---------- Assemble full disk image (sparse) --------------------------------
# truncate creates a sparse file — P1–P4 bytes are logical zeros (no storage).
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
# Splice the RedoxFS partition at the P5 byte offset.
dd if="$PART_IMG" of="$OUT" bs=512 seek="$PART_SRV_BASE_LBA" conv=notrunc status=none

echo "[mksrv-img] Done: $OUT"
echo "[mksrv-img]   sparse on disk : $(du -sh "$OUT" | cut -f1)"
echo "[mksrv-img]   file size      : $(( FULL_SECTORS * 512 / 1024 / 1024 )) MB"
