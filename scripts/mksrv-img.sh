#!/usr/bin/env bash
# mksrv-img.sh — Build the sparse disk image used by the /srv CellosFS integration test.
#
# Partition P5 (LBA 931_072, 64 MB) is a raw zero-filled partition: the VFS formats it
# as CellosFS Native on first mount, and the two-boot persistence test then exercises
# that volume. P1–P4 are zero-filled as well; the VFS service degrades gracefully when
# FAT32/littlefs fail to mount.
#
# This image used to be formatted as RedoxFS on the host and seeded with a hello.txt
# that nothing read. The VFS has not spoken RedoxFS since the CellosFS Native switch,
# so it re-formatted the partition on the first boot of every run: the host-side file
# system was dead weight that only cost a full dependency build in this job.
#
# Usage: bash scripts/mksrv-img.sh [OUT_IMG]
# Default output: build/disk_srv.img
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
echo "[mksrv-img] P5 at LBA $PART_SRV_BASE_LBA, $PART_SRV_SECTORS sectors (raw — the guest formats CellosFS)"

mkdir -p "$(dirname "$OUT")"

# ---------- Assemble full disk image (sparse) --------------------------------
# Recreate, never reuse: `truncate` alone would leave a previous image's P5 bytes
# in place, and this script must hand the guest a raw partition to format.
rm -f "$OUT"
truncate -s "$((FULL_SECTORS * 512))" "$OUT"

echo "[mksrv-img] Done: $OUT"
echo "[mksrv-img]   sparse on disk : $(du -sh "$OUT" | cut -f1)"
echo "[mksrv-img]   file size      : $(( FULL_SECTORS * 512 / 1024 / 1024 )) MB"
