#!/usr/bin/env bash
# Write the data disk the Chrome guest's Downloads folder lives on: a raw MBR image
# whose FAT32 P1 (LBA 2048, 256 MiB, the layout CellOS mounts as /mnt/disk/<device>)
# holds an empty folder `Chrome`. The hypervisor shares that folder with the guest
# over virtio-9p. An existing image is kept: it holds the user's files.
#
# Attach it on a controller no other disk uses (each CellOS block driver Cell drives one),
# e.g. q35's AHCI: -drive file=IMG,if=none,id=files,format=raw -device ide-hd,drive=files,bus=ide.1
#
# Usage: bash scripts/make-chrome-files-disk.sh IMG
set -euo pipefail

IMG="${1:?usage: make-chrome-files-disk.sh IMG}"
PART_START=2048

if [[ -s "$IMG" ]]; then
    echo "[chrome-files-disk] $IMG exists; kept"
    exit 0
fi
for tool in sfdisk mkfs.fat mmd; do
    command -v "$tool" >/dev/null || { echo "ERROR: $tool is required" >&2; exit 1; }
done
truncate -s 512M "$IMG"
printf 'label: dos\nunit: sectors\n\nstart=%d,size=524288,type=c\n' "$PART_START" \
    | sfdisk "$IMG" >/dev/null
# One sector per cluster: 256 MiB in larger clusters falls under FAT32's cluster minimum,
# and a reader that types the volume by its cluster count would take it for FAT16.
mkfs.fat -F 32 -s 1 --offset="$PART_START" -n CHROMEFILES "$IMG" $((524288 / 2)) >/dev/null 2>&1
mmd -i "$IMG@@$((PART_START * 512))" ::/Chrome
echo "[chrome-files-disk] $IMG: empty /Chrome"
