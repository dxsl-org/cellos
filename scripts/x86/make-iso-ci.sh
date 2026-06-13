#!/usr/bin/env bash
# Linux-native ISO builder for CI (Ubuntu runners).
# Uses workspace-relative paths and system xorriso.
# Mirrors build/make-iso.sh (which requires WSL + /mnt/d/ absolute paths).
#
# Usage: bash scripts/x86/make-iso-ci.sh [iso-out]
#   iso-out  default: build/vicell-x86.iso

set -euo pipefail

ISO_OUT="${1:-build/vicell-x86.iso}"
ISO_ROOT="build/x86-iso-root"
LIMINE="limine/limine-8.7.0/bin"
KERNEL="target/x86_64-unknown-none/release/vicell-kernel"
LIMINE_CONF="scripts/x86/limine.conf"

if [[ ! -f "$KERNEL" ]]; then
    echo "FAIL: kernel ELF not found: $KERNEL" >&2
    echo "  Build with: cargo build --release -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc" >&2
    exit 1
fi

if ! command -v xorriso &>/dev/null; then
    echo "FAIL: xorriso not found — install with: sudo apt-get install xorriso" >&2
    exit 1
fi

echo "[make-iso-ci] Assembling ISO root..."

mkdir -p "$ISO_ROOT/EFI/BOOT"
mkdir -p "$ISO_ROOT/boot/limine"
mkdir -p "$(dirname "$ISO_OUT")"

cp "$KERNEL"                    "$ISO_ROOT/boot/kernel.elf"
cp "$LIMINE_CONF"               "$ISO_ROOT/boot/limine.conf"
cp "$LIMINE/limine-bios-cd.bin" "$ISO_ROOT/boot/limine/"
cp "$LIMINE/limine-bios.sys"    "$ISO_ROOT/boot/limine/"
cp "$LIMINE/BOOTX64.EFI"        "$ISO_ROOT/EFI/BOOT/"
cp "$LIMINE/limine-uefi-cd.bin" "$ISO_ROOT/boot/limine/"

echo "[make-iso-ci] Building ISO: $ISO_OUT"

xorriso -as mkisofs \
  -b boot/limine/limine-bios-cd.bin \
  -no-emul-boot -boot-load-size 4 -boot-info-table \
  --efi-boot boot/limine/limine-uefi-cd.bin -efi-boot-part --efi-boot-image \
  -o "$ISO_OUT" "$ISO_ROOT" 2>&1

echo "ISO_SIZE=$(stat -c %s "$ISO_OUT") bytes"
echo "ISO_READY"
