#!/usr/bin/env bash
# build-mlibc.sh — Build the pinned mlibc libc.a for Cellos Tier-2 Cells.
#
# Flow (mirrors scripts/setup-mlibc.ps1 for Windows):
#   1. Fetch managarm/mlibc at MLIBC_PIN into third_party/mlibc-src/ (gitignored)
#   2. Copy sysdeps/vicell into the clone
#   3. Patch mlibc's meson.build with the 'vicell' host_machine.system() branch
#   4. Build each architecture whose compiler is installed
#   5. Copy libc.a to third_party/mlibc/build{,-aarch64,-x86_64}/libc.a
#
# Pin: the newest upstream commit that GCC 13 (Ubuntu 24.04) still builds.
# The next commit, 5c81d6fc (2026-03-31), starts using C++23 explicit object
# parameters, which need GCC >= 14; clang 18 also fails earlier on alias CTAD.
# Moving the pin is a deliberate edit: rebuild every row and rerun the
# mlibc-smoke link proofs.
#
# Rows and compilers:
#   riscv64  riscv64-unknown-elf-g++   scripts/mlibc-riscv64.cross
#   aarch64  aarch64-linux-gnu-g++     scripts/mlibc-aarch64.cross
#   x86_64   g++ (host, freestanding)  scripts/mlibc-x86_64.cross
# A row whose compiler is missing is skipped with a message, never faked.
#
# Prerequisites (Ubuntu/WSL2):
#   sudo apt install -y meson ninja-build gcc-riscv64-unknown-elf \
#       g++-aarch64-linux-gnu
#
# Usage: bash scripts/build-mlibc.sh
set -euo pipefail

MLIBC_PIN="ae1092eb211e42e80d2d1c73881c8c25fde80fe6"
MLIBC_REPO="https://github.com/managarm/mlibc.git"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MLIBC_SRC_DIR="$REPO_ROOT/third_party/mlibc-src"
OUR_SYSDEPS="$REPO_ROOT/third_party/mlibc/sysdeps/vicell"
OUT_ROOT="$REPO_ROOT/third_party/mlibc"
SCRIPTS="$REPO_ROOT/scripts"

# ─── Step 1: mlibc source at the pin ─────────────────────────────────────────
echo "=== Step 1: mlibc source @ $MLIBC_PIN ==="
if [[ ! -d "$MLIBC_SRC_DIR/.git" ]]; then
    git init -q "$MLIBC_SRC_DIR"
    git -C "$MLIBC_SRC_DIR" remote add origin "$MLIBC_REPO"
fi
if ! git -C "$MLIBC_SRC_DIR" cat-file -e "$MLIBC_PIN^{commit}" 2>/dev/null; then
    git -C "$MLIBC_SRC_DIR" fetch --depth 1 origin "$MLIBC_PIN"
fi
git -C "$MLIBC_SRC_DIR" checkout -q -f "$MLIBC_PIN"
# Drops the previous patch, sysdeps copy, builds and meson wrap checkouts, so
# subprojects (frigg, freestnd headers) come back at the pin's wrap revisions.
git -C "$MLIBC_SRC_DIR" clean -q -ffdx -e subprojects/packagecache
echo "  mlibc commit: $(git -C "$MLIBC_SRC_DIR" rev-parse HEAD)"

# ─── Step 2: Install Cellos sysdeps ──────────────────────────────────────────
echo "=== Step 2: Installing sysdeps/vicell and Cellos patches ==="
cp -r "$OUR_SYSDEPS" "$MLIBC_SRC_DIR/sysdeps/vicell"
# Small upstream-shaped changes to mlibc itself, one file each, applied in order.
for patch in "$OUT_ROOT"/patches/*.patch; do
    git -C "$MLIBC_SRC_DIR" apply "$patch"
    echo "  applied ${patch#"$REPO_ROOT"/}"
done

# ─── Step 3: Patch mlibc's meson.build ───────────────────────────────────────
echo "=== Step 3: Patching meson.build ==="
python3 - "$MLIBC_SRC_DIR/meson.build" <<'PYEOF'
import re
import sys

path = sys.argv[1]
content = open(path).read()
branch = "\nelif host_machine.system() == 'vicell'\n\tsubdir('sysdeps/vicell')\n"
patched = re.sub(r"(\n\s*else\s*\n\s*error\('No sysdeps defined)", branch + r"\1", content, count=1)
if patched == content:
    sys.exit("ERROR: no `error('No sysdeps defined ...')` block; insert the vicell branch by hand")
open(path, "w").write(patched)
PYEOF

COMMON_OPTS=(
    --prefix=/
    -Ddefault_library=static
    -Dposix_option=enabled
    -Dlinux_option=disabled
    -Dheaders_only=false
)

# ─── Step 4: one row per architecture ────────────────────────────────────────
# build_row <name> <compiler> <cross file> <output dir>
build_row() {
    local name="$1" compiler="$2" cross="$3" out="$4"
    echo "=== Step 4: $name ==="
    if ! command -v "$compiler" >/dev/null 2>&1; then
        echo "  SKIP $name: $compiler not installed (libc.a not rebuilt)"
        return 0
    fi
    local build="$MLIBC_SRC_DIR/build-$name"
    meson setup "$build" "$MLIBC_SRC_DIR" --cross-file="$cross" "${COMMON_OPTS[@]}"
    ninja -C "$build"
    mkdir -p "$out"
    cp "$build/libc.a" "$out/libc.a"
    # C/C++ code linked against this libc.a compiles with -nostdinc and
    # -isystem <out>/sysroot/include (mlibc's installed headers).
    rm -rf "$out/sysroot"
    DESTDIR="$out/sysroot" meson install -C "$build" --no-rebuild --quiet
    # mlibc calls libgcc helpers (__bswapsi2, __multf3, ...). Rust's prebuilt
    # compiler_builtins may carry them with another float ABI (riscv64: its
    # soft-float objects are rejected by lld next to lp64d code), so ship the
    # helpers of the compiler that built libc.a; libs/mlibc-shim links them
    # right after libc.a.
    local cflags
    cflags="$(sed -n "s/^c_args *= *\[\(.*\)\]/\1/p" "$cross" | tr -d "',")"
    # shellcheck disable=SC2086 # cflags is a list of single-word flags
    cp "$("$compiler" $cflags -print-libgcc-file-name)" "$out/libgcc.a"
    echo "  $name libc.a: $(du -sh "$out/libc.a" | cut -f1) → ${out#"$REPO_ROOT"/}"
}

build_row riscv64 riscv64-unknown-elf-g++ "$SCRIPTS/mlibc-riscv64.cross" "$OUT_ROOT/build"
build_row aarch64 aarch64-linux-gnu-g++ "$SCRIPTS/mlibc-aarch64.cross" "$OUT_ROOT/build-aarch64"
build_row x86_64 g++ "$SCRIPTS/mlibc-x86_64.cross" "$OUT_ROOT/build-x86_64"

echo ""
echo "mlibc build complete (pin $MLIBC_PIN)."
