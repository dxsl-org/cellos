#!/usr/bin/env bash
# Build Cellos for VisionFive2 and create a Limine UEFI SD card image.
#
# Usage:
#   ./scripts/vf2-flash.sh              # build + create vf2-boot.img in repo root
#   ./scripts/vf2-flash.sh /dev/sdX     # build + flash to SD card (Linux host, run as root)
#
# Requirements (Linux host):
#   - Rust toolchain (riscv64gc-unknown-none-elf target)
#   - parted, losetup, mkfs.fat (util-linux + dosfstools)
#   - curl or wget (for Limine download)
#
# Windows users: run from WSL2, or use scripts/vf2-build.ps1 (PowerShell) which
# delegates the image-creation step to WSL2.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO_ROOT/vf2-boot.img"
KERNEL="$REPO_ROOT/target/riscv64gc-unknown-none-elf/release/vicell-kernel"
LIMINE_EFI="$REPO_ROOT/tools/limine-riscv64"

# ── 1. Ensure Limine UEFI binary is present (non-empty) ───────────────────────
# Use -s (size > 0) to catch stale zero-byte files from interrupted downloads.
if [[ ! -s "$LIMINE_EFI" ]]; then
    echo "[vf2-flash] Downloading Limine..."
    rm -f "$LIMINE_EFI"
    bash "$REPO_ROOT/scripts/download-limine.sh"
fi

# ── 2. Build kernel (board-vf2 feature, release) ──────────────────────────────
echo "[vf2-flash] Building Cellos kernel for VisionFive2..."
cd "$REPO_ROOT"
RUSTFLAGS="-C relocation-model=pic" \
    cargo build --release -p vicell-kernel \
    --target riscv64gc-unknown-none-elf \
    --features board-vf2

# ── 3. Create 256 MB GPT disk image ───────────────────────────────────────────
echo "[vf2-flash] Creating disk image: $OUT"
dd if=/dev/zero of="$OUT" bs=1M count=256 status=progress

# GPT + EFI System Partition (ESP), 200 MB
parted -s "$OUT" mklabel gpt
parted -s "$OUT" mkpart ESP fat32 2MiB 202MiB
parted -s "$OUT" set 1 esp on

# ── 4. Format and populate FAT32 boot partition ────────────────────────────────
LOOP=$(losetup --find --partscan --show "$OUT")
# Install cleanup trap immediately after losetup to avoid leaking the loop device
# if any subsequent command fails in the few lines before we set up the full trap.
cleanup() {
    mountpoint -q /tmp/vf2-mnt 2>/dev/null && umount /tmp/vf2-mnt || true
    losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

# Wait for the kernel to create the partition device node (${LOOP}p1).
# udevadm settle is more reliable than a fixed sleep on slow/CI hosts.
udevadm settle 2>/dev/null || partprobe "$LOOP" 2>/dev/null || sleep 1
[[ -b "${LOOP}p1" ]] || { echo "[vf2-flash] ERROR: ${LOOP}p1 not found after settle"; exit 1; }

mkfs.fat -F 32 -n CELLOS "${LOOP}p1"

MNT=/tmp/vf2-mnt
mkdir -p "$MNT"
mount "${LOOP}p1" "$MNT"

# EFI standard path for RISC-V 64-bit UEFI
mkdir -p "$MNT/EFI/BOOT"
cp "$LIMINE_EFI"                 "$MNT/EFI/BOOT/BOOTRISCV64.EFI"
cp "$REPO_ROOT/limine-vf2.conf"  "$MNT/limine.conf"
cp "$KERNEL"                     "$MNT/vicell-kernel"

echo "[vf2-flash] Boot partition contents:"
ls -lh "$MNT/EFI/BOOT/BOOTRISCV64.EFI" "$MNT/limine.conf" "$MNT/vicell-kernel"

umount "$MNT"
losetup -d "$LOOP"
trap - EXIT

echo ""
echo "[vf2-flash] Image ready: $OUT"
echo "            Size: $(du -sh "$OUT" | cut -f1)"

# ── 5. Optional: flash to SD card ──────────────────────────────────────────────
if [[ -n "${1:-}" ]]; then
    echo ""
    echo "[vf2-flash] WARNING: This will ERASE $1"
    echo "            Target: $(lsblk -nd -o NAME,SIZE,MODEL "$1" 2>/dev/null || echo "$1")"
    echo ""
    read -rp "Type YES to continue: " confirm
    if [[ "$confirm" != "YES" ]]; then
        echo "[vf2-flash] Aborted."
        exit 1
    fi
    echo "[vf2-flash] Flashing to $1..."
    dd if="$OUT" of="$1" bs=4M status=progress conv=fsync
    sync
    echo "[vf2-flash] Done. Safely remove $1."
fi
