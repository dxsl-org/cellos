#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Build the x86_64 image for the Monty Tier-2 witness (cells/demos/monty-smoke).
#
# Produces:
#   cells/demos/monty-smoke/target/x86_64-unknown-cellos/release/monty-smoke
#   target/x86-monty-smoke/x86_64-unknown-none/release/cellos-kernel-monty-smoke
#   target/x86-monty-smoke-embedded/{init,kernel_fs.img}
# Boot it with: bash scripts/x86/qemu-monty-smoke.sh
#
# Same shape as scripts/build-x86_64-cpp-hosted-ci.sh: a `test-hooks` kernel, so
# the Tier-2 admission posture is open, in its own CARGO_TARGET_DIR with its own
# embedded ramdisk; the image boots to the shell and the runner types the
# witness name. The witness is a rust-std cell, built like std-smoke
# (scripts/run-std-smoke-qemu.sh) against the CellOS sysroot overlay.
#
# Prerequisites: bash scripts/fetch-monty.sh
#
# Usage: bash scripts/build-x86_64-monty-smoke-ci.sh [--link-only [riscv64|aarch64|x86_64]...]
#   --link-only  build the witness for the named architectures (default: all
#                three) and check the ELF has no undefined symbols; no image

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

TOOLCHAIN="nightly-2026-11-21"
CELL_DIR="cells/demos/monty-smoke"
STAGING_DIR="$REPO_ROOT/target/cellos-rust-src/library"

LINK_ONLY=0
ARCHES=()
if [[ "${1:-}" == "--link-only" ]]; then
    LINK_ONLY=1
    shift
    ARCHES=("$@")
    [[ ${#ARCHES[@]} -gt 0 ]] || ARCHES=(riscv64 aarch64 x86_64)
elif [[ $# -gt 0 ]]; then
    echo "usage: $0 [--link-only [riscv64|aarch64|x86_64]...]" >&2
    exit 64
fi

[[ -f third_party/monty-src/crates/monty/Cargo.toml && -d third_party/monty-deps/jiter-0.19.0 ]] \
    || { echo "FAIL: Monty sources missing; run: bash scripts/fetch-monty.sh" >&2; exit 1; }

sysroot_target() {
    case "$1" in
        riscv64) echo riscv64gc-unknown-cellos ;;
        aarch64) echo aarch64-unknown-cellos ;;
        x86_64) echo x86_64-unknown-cellos ;;
        *) echo "FAIL: unsupported arch: $1" >&2; exit 64 ;;
    esac
}

# build_witness <arch>: builds the std cell; prints the ELF path.
build_witness() {
    local target
    target="$(sysroot_target "$1")"
    bash scripts/build-cellos-sysroot.sh "$target" >&2
    (
        cd "$CELL_DIR"
        __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR" cargo "+$TOOLCHAIN" build --release --locked \
            -Z build-std=core,alloc,std,panic_abort \
            -Z build-std-features=compiler-builtins-mem \
            -Z json-target-spec \
            --target "$REPO_ROOT/targets/$target.json" >&2
    )
    local elf="$CELL_DIR/target/$target/release/monty-smoke"
    [[ -s "$elf" ]] || { echo "FAIL: witness not produced at $elf" >&2; exit 1; }
    # Capture before matching: under pipefail, `nm | grep -q` fails when grep exits early.
    local undefined
    undefined="$(nm -u "$elf" | awk '$1 == "U" { print $2 }')"
    [[ -z "$undefined" ]] || { echo "FAIL: $elf has unresolved symbols: $undefined" >&2; exit 1; }
    echo "MONTY-SMOKE-LINK: PASS ($1, $(stat -c %s "$elf") bytes)" >&2
    echo "$elf"
}

if [[ "$LINK_ONLY" -eq 1 ]]; then
    for arch in "${ARCHES[@]}"; do
        build_witness "$arch" >/dev/null
    done
    exit 0
fi

TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-monty-smoke"
TH_DIR="target/x86-monty-smoke-embedded"
PYTHON_BIN="${PYTHON_BIN:-python3}"

echo "==> Building the monty-smoke witness..."
WITNESS="$(build_witness x86_64)"

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"

echo "==> Building base cells (init, shell, vfs, config, input, platform, tools)..."
mkdir -p "$TH_DIR"
# A failed rebuild must never leave a bootable kernel embedding the previous run's cells.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init"
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-cellbox -p service-vfs -p service-machine-pin -p service-config -p service-input -p service-platform \
    -p app-sys-tools

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-machine-pin" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps" "$REL/free"
    "$WITNESS"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/machine-pin /bin/config /bin/input
    /bin/platform /bin/ls /bin/cat /bin/echo /bin/ps /bin/free
    /bin/monty-smoke
)
FAT_CELL_ARGS=()
for index in "${!CELL_BINARIES[@]}"; do
    binary="${CELL_BINARIES[$index]}"
    [[ -s "$binary" ]] || { echo "FAIL: missing cell binary $binary" >&2; exit 1; }
    FAT_CELL_ARGS+=("$binary" "${CELL_IMAGE_PATHS[$index]}")
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing cells + init with the dev key..."
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

echo "==> Assembling kernel_fs.img..."
"$PYTHON_BIN" tools/mkfat32.py --config-features input "$TH_DIR/kernel_fs.img" "${FAT_CELL_ARGS[@]}"
[[ -s "$TH_DIR/kernel_fs.img" ]] || { echo "FAIL: kernel_fs.img not produced" >&2; exit 1; }
cp "$REL/app-init" "$TH_DIR/init"

echo "==> Building test-hooks kernel (x86_64, static relocation)..."
EMBEDDED_OVERRIDE="$TH_DIR" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-D warnings -C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --features test-hooks --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced at $KERNEL" >&2; exit 1; }
cp "$KERNEL" "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-monty-smoke"
echo "MONTY_SMOKE_KERNEL=$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-monty-smoke"
echo "==> Done"
