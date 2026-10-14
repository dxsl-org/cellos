#!/usr/bin/env bash
# mksrv-img.sh — Build the sparse disk image used by the /srv CellosFS integration test.
#
# P4 /data and P5 /srv are provisioned offline with the actual CellosFS library.
# Runtime mounts open existing volumes only; blank/corrupt volumes remain unavailable.
# P1–P3 remain zero-filled. This is a fresh disposable integration fixture, not a
# retained-state migration or durability qualification.
#
# Usage: bash scripts/mksrv-img.sh [OUT_IMG]
# Default output: build/disk_srv.img
#
# Disk layout (matches libs/api/src/abi/disk.rs):
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
echo "[mksrv-img] P4/P5: offline blank-only CellosFS provisioning"

mkdir -p "$(dirname "$OUT")"

# ---------- Assemble full disk image (sparse) --------------------------------
# Recreate, never reuse: truncate alone is not proof of a blank native partition.
rm -f "$OUT"
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
bash "$(dirname "$0")/../tools/cellos-fs-provision/provision.sh" "$OUT" fresh-blank

echo "[mksrv-img] Done: $OUT"
echo "[mksrv-img]   sparse on disk : $(du -sh "$OUT" | cut -f1)"
echo "[mksrv-img]   file size      : $(( FULL_SECTORS * 512 / 1024 / 1024 )) MB"
