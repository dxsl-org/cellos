#!/usr/bin/env bash
# Write the CellOS SD disk that carries the Tier 3 Chrome guest's root: a raw MBR
# image whose FAT32 P1 (LBA 2048, 256 MiB, mounted by CellOS as /mnt/sd) holds
# chrome-guest.sqfs from scripts/build-chrome-guest-image.sh (built when missing;
# needs docker). The guest kernel and initramfs are in the desktop image itself.
# Attach the disk to QEMU as an NVMe drive:
#   -drive file=IMG,if=none,id=sd,format=raw -device nvme,drive=sd,serial=CELLOSSD
#
# Usage: bash scripts/make-chrome-guest-disk.sh IMG [GUEST_DIR]
#   GUEST_DIR  builder output (default build/chrome-guest)
set -euo pipefail

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
IMG="${1:?usage: make-chrome-guest-disk.sh IMG [GUEST_DIR]}"
GUEST_DIR="${2:-$REPO_ROOT/build/chrome-guest}"
FILES=(chrome-guest.sqfs)
PART_START=2048

for tool in sfdisk mkfs.fat mcopy; do
    command -v "$tool" >/dev/null || { echo "ERROR: $tool is required" >&2; exit 1; }
done
stale=0
for file in "${FILES[@]}"; do [[ -f "$GUEST_DIR/$file" ]] || stale=1; done
# An image built from an older builder or guest init is rebuilt, not reused.
[[ "$(cat "$GUEST_DIR/INPUTS.sha256" 2>/dev/null)" == "$(bash "$REPO_ROOT/scripts/chrome-guest/inputs-digest.sh")" ]] \
    || stale=1
if (( stale )); then
    bash "$REPO_ROOT/scripts/build-chrome-guest-image.sh" "$GUEST_DIR"
fi
(cd "$GUEST_DIR" && sha256sum --quiet -c SHA256SUMS) \
    || { echo "ERROR: $GUEST_DIR does not match its SHA256SUMS" >&2; exit 1; }

truncate -s 0 "$IMG"
truncate -s 512M "$IMG"
printf 'label: dos\nunit: sectors\n\nstart=%d,type=c\n' "$PART_START" | sfdisk "$IMG" >/dev/null
mkfs.fat -F 32 --offset="$PART_START" -n CELLOSSD "$IMG" >/dev/null
for file in "${FILES[@]}"; do
    mcopy -o -i "$IMG@@$((PART_START * 512))" "$GUEST_DIR/$file" "::/$file"
done
echo "[chrome-guest-disk] $IMG: ${FILES[*]}"
