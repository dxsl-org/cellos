#!/usr/bin/env bash
# build-mlibc.sh — Build mlibc libc.a for ViCell (riscv64 + aarch64).
# Must run inside WSL2 on Windows; the riscv xpack toolchain is at /mnt/c/RISCV.
#
# Prerequisites (one-time setup in WSL2):
#   sudo apt update && sudo apt install -y meson ninja-build \
#       gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
#   # Verify riscv toolchain accessible:
#   /mnt/c/RISCV/riscv-none-elf-gcc-15.2.0-1/bin/riscv-none-elf-gcc --version
#
# Usage:
#   cd /path/to/ViCell                        # WSL2 path, e.g. /mnt/d/ViCell
#   bash scripts/build-mlibc.sh
#
# Outputs:
#   third_party/mlibc/build/libc.a            (riscv64)
#   third_party/mlibc/build-aarch64/libc.a    (aarch64)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MLIBC_SRC="$REPO_ROOT/third_party/mlibc"
SCRIPTS="$REPO_ROOT/scripts"

COMMON_OPTS=(
    -Dsysdeps=vicell
    -Ddefault_library=static
    -Dposix_option=disabled
    -Dlinux_option=disabled
    -Dheaders_only=false
)

# ─── riscv64 ──────────────────────────────────────────────────────────────────
echo "=== Building mlibc for riscv64 ==="
BUILD_RV="$MLIBC_SRC/build"
meson setup "$BUILD_RV" "$MLIBC_SRC" \
    --cross-file="$SCRIPTS/mlibc-riscv64.cross" \
    "${COMMON_OPTS[@]}" \
    --wipe 2>/dev/null || \
meson setup "$BUILD_RV" "$MLIBC_SRC" \
    --cross-file="$SCRIPTS/mlibc-riscv64.cross" \
    "${COMMON_OPTS[@]}"

ninja -C "$BUILD_RV"
echo "riscv64 libc.a: $(ls -lh "$BUILD_RV/libc.a" | awk '{print $5}')"

# ─── aarch64 ──────────────────────────────────────────────────────────────────
echo "=== Building mlibc for aarch64 ==="
BUILD_A64="$MLIBC_SRC/build-aarch64"
meson setup "$BUILD_A64" "$MLIBC_SRC" \
    --cross-file="$SCRIPTS/mlibc-aarch64.cross" \
    "${COMMON_OPTS[@]}" \
    --wipe 2>/dev/null || \
meson setup "$BUILD_A64" "$MLIBC_SRC" \
    --cross-file="$SCRIPTS/mlibc-aarch64.cross" \
    "${COMMON_OPTS[@]}"

ninja -C "$BUILD_A64"
echo "aarch64 libc.a: $(ls -lh "$BUILD_A64/libc.a" | awk '{print $5}')"

echo ""
echo "Done. Rust build.rs will pick up:"
echo "  $BUILD_RV/libc.a   (riscv64)"
echo "  $BUILD_A64/libc.a  (aarch64)"
