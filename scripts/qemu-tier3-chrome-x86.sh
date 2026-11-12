#!/usr/bin/env bash
# Tier 3 Chrome lane (Intel VMX via nested KVM): the taskbar's Chrome boots
# Chromium in the Alpine guest, the guest takes its own DHCP lease beside the
# host stack, and Chromium loads a real HTTPS page typed into its own omnibox.
#
# Builds a private desktop image with the guest kernel and the Chrome initramfs
# (Alpine initramfs + scripts/chrome-guest/stage1.sh), and an NVMe disk whose
# FAT32 P1 (LBA 2048, /mnt/sd) holds chrome-guest.sqfs
# (scripts/build-chrome-guest-image.sh; built here when missing). Boots it with a
# QEMU user-mode NIC and judges it with scripts/tier3_chrome_check.py.
#
# Guest startup is bounded by guest disk reads (~18 min at ~2 MB/s until the
# multi-sector block plan lands), hence the long default CHROME_WINDOW.
#
# Environment:
#   CELLOS_TIER3_WORK  work directory (default: a new /tmp/cellos-tier3-chrome-*)
#   QEMU_X86_BIN       emulator (default: qemu-system-x86_64)
#   BOOT_WINDOW        seconds for the desktop to come up (default: 180)
#   CHROME_WINDOW      seconds for Chromium's UI to appear (default: 1800)
#   CHROME_URL         page typed into the omnibox (default: https://example.com/)
#
# Exit: 0 PASS, 1 FAIL, 2 BLOCKED_ENVIRONMENT.

set -euo pipefail
REPO="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
W="${CELLOS_TIER3_WORK:-$(mktemp -d /tmp/cellos-tier3-chrome-XXXX)}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
CHROME_WINDOW="${CHROME_WINDOW:-1800}"
CHROME_URL="${CHROME_URL:-https://example.com/}"
SQFS="build/chrome-guest/chrome-guest.sqfs"
PART_START=2048
mkdir -p "$W"

# shellcheck source=scripts/lib-tier3-desktop-iso.sh
source scripts/lib-tier3-desktop-iso.sh
tier3_preflight sfdisk mkfs.fat
if [[ ! -f "$SQFS" ]]; then
    command -v docker >/dev/null || blocked "docker is required to build $SQFS"
    bash scripts/build-chrome-guest-image.sh "$(dirname "$SQFS")"
fi

python3 tools/repack-initramfs.py .alpine-cache-x86/initramfs-virt "$W/chrome-initrd.gz" \
    --add /bin/chrome-guest-stage1 scripts/chrome-guest/stage1.sh 100755
tier3_build_iso "$W" tier3-chrome "$W/chrome-initrd.gz" /chrome-initrd.gz

echo "==> [tier3-chrome] writing the guest disk"
disk="$W/outer-nvme.img"
truncate -s 0 "$disk"
truncate -s 512M "$disk"
printf 'label: dos\nunit: sectors\n\nstart=%d,type=c\n' "$PART_START" | sfdisk "$disk" >/dev/null
mkfs.fat -F 32 --offset="$PART_START" -n CELLOSSD "$disk" >/dev/null
mcopy -o -i "$disk@@$((PART_START * 512))" "$SQFS" ::/chrome-guest.sqfs

echo "==> [tier3-chrome] booting"
exec python3 scripts/tier3_chrome_check.py --qemu "$QEMU_X86_BIN" --iso "$W/cellos.iso" \
    --disk "$disk" --work "$W" --boot-window "$BOOT_WINDOW" \
    --chrome-window "$CHROME_WINDOW" --url "$CHROME_URL"
