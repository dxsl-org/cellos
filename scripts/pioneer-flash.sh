#!/usr/bin/env bash
# Build Cellos for Milk-V Pioneer (SG2042) and create a Limine UEFI NVMe/SD image.
#
# Usage:
#   ./scripts/pioneer-flash.sh              # build + create pioneer-boot.img in repo root
#   ./scripts/pioneer-flash.sh /dev/sdX     # build + write to a USB/SD device (run as root)
#
# Requirements (Linux host):
#   - Rust toolchain (riscv64gc-unknown-none-elf target)
#   - parted, losetup, mkfs.fat (util-linux + dosfstools)
#   - curl or wget (for Limine download)
#
# Windows users: run from WSL2, or use scripts/pioneer-build.ps1 (PowerShell).
#
# Hardware notes (SG2042 / Pioneer):
#   - UART at 0x7040000000 is sv39-inaccessible; console I/O uses SBI DBCN extension.
#   - PLIC/CLINT (thead,c900-plic / thead,c900-clint) at same addresses as RISC-V virt defaults.
#   - DRAM starts at 0x8000_0000 — same as QEMU virt, no separate board-specific fallback needed.
#   - Flash to an NVMe SSD or USB drive inserted in the Pioneer's M.2/USB port.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO_ROOT/pioneer-boot.img"
KERNEL="$REPO_ROOT/target/riscv64gc-unknown-none-elf/release/vicell-kernel"
LIMINE_EFI="$REPO_ROOT/tools/limine-riscv64"

# ── 1. Ensure Limine UEFI binary is present ────────────────────────────────────
if [[ ! -s "$LIMINE_EFI" ]]; then
    echo "[pioneer-flash] Downloading Limine..."
    rm -f "$LIMINE_EFI"
    bash "$REPO_ROOT/scripts/download-limine.sh"
fi

# ── 2. Build kernel (board-pioneer feature, release) ──────────────────────────
echo "[pioneer-flash] Building Cellos kernel for Pioneer SG2042..."
cd "$REPO_ROOT"
RUSTFLAGS="-C relocation-model=pic" \
    cargo build --release -p vicell-kernel \
    --target riscv64gc-unknown-none-elf \
    --features board-pioneer

# ── 3. Create 256 MB GPT disk image ───────────────────────────────────────────
echo "[pioneer-flash] Creating disk image: $OUT"
dd if=/dev/zero of="$OUT" bs=1M count=256 status=progress

# GPT + EFI System Partition (ESP), 200 MB
parted -s "$OUT" mklabel gpt
parted -s "$OUT" mkpart ESP fat32 2MiB 202MiB
parted -s "$OUT" set 1 esp on

# ── 4. Format and populate FAT32 boot partition ────────────────────────────────
LOOP=$(losetup --find --partscan --show "$OUT")
cleanup() {
    mountpoint -q /tmp/pioneer-mnt 2>/dev/null && umount /tmp/pioneer-mnt || true
    losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

udevadm settle 2>/dev/null || partprobe "$LOOP" 2>/dev/null || sleep 1
[[ -b "${LOOP}p1" ]] || { echo "[pioneer-flash] ERROR: ${LOOP}p1 not found after settle"; exit 1; }

mkfs.fat -F 32 -n CELLOS "${LOOP}p1"

MNT=/tmp/pioneer-mnt
mkdir -p "$MNT"
mount "${LOOP}p1" "$MNT"

mkdir -p "$MNT/EFI/BOOT"
cp "$LIMINE_EFI"                    "$MNT/EFI/BOOT/BOOTRISCV64.EFI"
cp "$REPO_ROOT/limine-pioneer.conf" "$MNT/limine.conf"
cp "$KERNEL"                        "$MNT/vicell-kernel"

echo "[pioneer-flash] Boot partition contents:"
ls -lh "$MNT/EFI/BOOT/BOOTRISCV64.EFI" "$MNT/limine.conf" "$MNT/vicell-kernel"

umount "$MNT"
losetup -d "$LOOP"
trap - EXIT

echo ""
echo "[pioneer-flash] Image ready: $OUT"
echo "            Size: $(du -sh "$OUT" | cut -f1)"

# ── 5. Optional: write to target device ───────────────────────────────────────
if [[ -n "${1:-}" ]]; then
    echo ""
    echo "[pioneer-flash] WARNING: This will ERASE $1"
    echo "            Target: $(lsblk -nd -o NAME,SIZE,MODEL "$1" 2>/dev/null || echo "$1")"
    echo ""
    read -rp "Type YES to continue: " confirm
    if [[ "$confirm" != "YES" ]]; then
        echo "[pioneer-flash] Aborted."
        exit 1
    fi
    echo "[pioneer-flash] Writing to $1..."
    dd if="$OUT" of="$1" bs=4M status=progress conv=fsync
    sync
    echo "[pioneer-flash] Done. Safely remove $1."
fi
