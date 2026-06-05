#!/usr/bin/env bash
# Downloads the Limine RISC-V S-mode bootloader binary for CI and local use.
# Limine acts as an OpenSBI payload (S-mode), loaded via QEMU -kernel.
#
# Usage: ./scripts/download-limine.sh [output-path]
# Default output: tools/limine-riscv64

set -euo pipefail

# Pin to Limine v8.x stable binary release.
LIMINE_TAG="v8.9.2-binary"
LIMINE_FILE="limine-riscv64"
LIMINE_URL="https://github.com/limine-bootloader/limine/releases/download/${LIMINE_TAG}/${LIMINE_FILE}"
DEST="${1:-tools/limine-riscv64}"

mkdir -p "$(dirname "$DEST")"

if [[ -f "$DEST" ]]; then
  echo "[limine] Already present: $DEST"
  exit 0
fi

echo "[limine] Downloading Limine ${LIMINE_TAG} for RISC-V..."
if command -v curl &>/dev/null; then
  curl -fsSL -o "$DEST" "$LIMINE_URL"
elif command -v wget &>/dev/null; then
  wget -q -O "$DEST" "$LIMINE_URL"
else
  echo "[limine] ERROR: neither curl nor wget found" >&2
  exit 1
fi

chmod +x "$DEST"
echo "[limine] Saved to $DEST ($(du -sh "$DEST" | cut -f1))"
