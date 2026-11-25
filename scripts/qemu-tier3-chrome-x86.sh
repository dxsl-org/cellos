#!/usr/bin/env bash
# Tier 3 Chrome lane (Intel VMX via nested KVM): the desktop image as shipped
# (assemble-x86-image.py --profile desktop, which carries the guest's /vmlinux and
# /chrome-initrd.gz) plus /chrome-guest-vcpus asking for four vCPUs (the shipped
# image gives one; the lane's machine has eight CPUs), and the guest root disk
# (scripts/make-chrome-guest-disk.sh: chrome-guest.sqfs on /mnt/sd).
# scripts/tier3_chrome_check.py drives it: taskbar
# launch -> the guest's own DHCP lease beside the host stack -> the launcher's
# status, then Chromium's UI in the window -> a URL typed into Chromium's omnibox
# renders -> nothing else of the guest shows (no window buttons or warning bar of
# Chromium's own, no guest pointer, no Linux console on Ctrl+Alt+F2) -> the window
# is resized and Chromium follows -> Close ends the launcher and the hypervisor ->
# a second session carries text between Chromium and the Terminal, downloads a file
# into the shared Downloads folder (read back from the data disk image) and opens a
# page put on that disk; it is ended from inside Chromium, powers the guest off and
# closes the window.
#
# Not a CI lane: runners have no nested VMX. Without it the lane exits 2.
#
# Environment:
#   CELLOS_TIER3_WORK  work directory (default: a new /tmp/cellos-tier3-chrome-*)
#   QEMU_X86_BIN       emulator (default: qemu-system-x86_64)
#   BOOT_WINDOW        seconds for the desktop to come up (default: 180)
#   CHROME_WINDOW      seconds for Chromium's UI to appear (default: 900)
#   CHROME_URL         page typed into the omnibox (default: https://example.com/)
#
# Exit: 0 PASS, 1 FAIL, 2 BLOCKED_ENVIRONMENT.

set -euo pipefail
REPO="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
W="${CELLOS_TIER3_WORK:-$(mktemp -d /tmp/cellos-tier3-chrome-XXXX)}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
CHROME_WINDOW="${CHROME_WINDOW:-900}"
CHROME_URL="${CHROME_URL:-https://example.com/}"
mkdir -p "$W"

# shellcheck source=scripts/lib-tier3-desktop-iso.sh
source scripts/lib-tier3-desktop-iso.sh
tier3_preflight sfdisk mkfs.fat mmd mcopy
[[ -f build/chrome-guest/SHA256SUMS ]] || command -v docker >/dev/null \
    || blocked "docker is required to build the Chrome guest (build/chrome-guest)"

echo 4 > "$W/chrome-guest-vcpus"
tier3_build_iso "$W" tier3-chrome "$W/chrome-guest-vcpus" /chrome-guest-vcpus
disk="$W/chrome-guest-disk.img"
bash scripts/make-chrome-guest-disk.sh "$disk"
# Phase 10: a fresh data disk whose Chrome folder holds a page for the guest to open.
files="$W/chrome-files.img"
rm -f "$files"
bash scripts/make-chrome-files-disk.sh "$files"
printf '<!doctype html><title>host</title><body style="background:#3c6e9f"></body>\n' \
    > "$W/from-host.html"
mcopy -o -i "$files@@$((2048 * 512))" "$W/from-host.html" ::/Chrome/from-host.html

echo "==> [tier3-chrome] booting"
exec python3 scripts/tier3_chrome_check.py --qemu "$QEMU_X86_BIN" --iso "$W/cellos.iso" \
    --disk "$disk" --files-disk "$files" --work "$W" --boot-window "$BOOT_WINDOW" \
    --chrome-window "$CHROME_WINDOW" --url "$CHROME_URL"
