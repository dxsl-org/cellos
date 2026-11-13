#!/usr/bin/env bash
# Build the x86_64 image for the `cpp-hosted` witness (ADR-0034, runtime plan R4).
#
# Produces:
#   target/x86_64-unknown-none/release/cpp-hosted-smoke (link-proof checks below)
#   target/x86-cpp-hosted/x86_64-unknown-none/release/cellos-kernel-cpp-hosted
#   target/x86-cpp-hosted-embedded/{init,kernel_fs.img}
# Boot it with: bash scripts/x86/qemu-cpp-hosted-smoke.sh
#
# Same shape as scripts/build-x86_64-mlibc-witness-ci.sh (H-4: a new
# standalone script; the shared lanes are not edited): a `test-hooks` kernel,
# so the Tier-2 admission posture is open, in its own CARGO_TARGET_DIR with its
# own embedded ramdisk. The image boots to the shell; the runner types the
# witness name. The shell launches /bin/cpp-hosted-smoke only once its launch
# rows exist (owner H-1).
#
# Prerequisites: bash scripts/build-mlibc.sh; bash scripts/build-llvm-runtimes.sh x86_64
#
# Usage: bash scripts/build-x86_64-cpp-hosted-ci.sh [--link-only]
#   --link-only  build and check the witness Cell; skip signing, image and kernel

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

LINK_ONLY=0
case "${1:-}" in "") ;; --link-only) LINK_ONLY=1 ;; *) echo "usage: $0 [--link-only]" >&2; exit 64 ;; esac

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-cpp-hosted"
TH_DIR="target/x86-cpp-hosted-embedded"
RUNTIME="${CELLOS_LLVM_RUNTIMES_DIR:-target/llvm-runtimes/x86_64}"

for need in third_party/mlibc/build-x86_64/libc.a third_party/mlibc/build-x86_64/sysroot/include; do
    [[ -e "$need" ]] || { echo "FAIL: $need missing; run: bash scripts/build-mlibc.sh" >&2; exit 1; }
done
for need in "$RUNTIME"/lib/lib{c++,c++abi,unwind}.a "$RUNTIME/include/c++/v1"; do
    [[ -e "$need" ]] || { echo "FAIL: $need missing; run: bash scripts/build-llvm-runtimes.sh x86_64" >&2; exit 1; }
done

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building the cpp-hosted witness..."
# Not built with -Z build-std: libc.a and libgcc.a come from build-mlibc.sh.
cargo build --release --target "$TARGET" -p app-cpp-hosted-smoke
WITNESS="$REL/cpp-hosted-smoke"

# Link proof (phase R4 witness requirements). Capture each dump before
# matching: under pipefail, `nm | grep -q` fails when grep exits early.
symbols="$(nm "$WITNESS")"
for sym in cpp_hosted_witness __cellos_libc_start __cxa_throw _Unwind_RaiseException __gxx_personality_v0; do
    grep -q " T $sym$" <<<"$symbols" || { echo "FAIL: $WITNESS lacks $sym (NOT-BUILT path or runtime missing)" >&2; exit 1; }
done
grep -q " [TWV] _ZSt" <<<"$symbols" || { echo "FAIL: $WITNESS carries no _ZSt* runtime symbol" >&2; exit 1; }
strong_undefined="$(nm -u "$WITNESS" | awk '$1 == "U" { print $2 }')"
[[ -z "$strong_undefined" ]] || { echo "FAIL: unresolved symbols: $strong_undefined" >&2; exit 1; }
# The thread-safe-static guard must be libc++abi's (pthread-backed), not the
# single-threaded Tier-A one from libs/posix-shim: same size as the archive's copy.
cell_guard="$(nm -S "$WITNESS" | awk '$4 == "__cxa_guard_acquire" { print $2 }')"
abi_guard="$(nm -S --defined-only "$RUNTIME/lib/libc++abi.a" 2>/dev/null | awk '$4 == "__cxa_guard_acquire" { print $2 }')"
[[ -n "$cell_guard" && "$cell_guard" == "$abi_guard" ]] \
    || { echo "FAIL: __cxa_guard_acquire is not libc++abi's (cell size '$cell_guard', libc++abi '$abi_guard')" >&2; exit 1; }
headers="$(readelf -lW "$WITNESS")"
grep -q "GNU_EH_FRAME" <<<"$headers" || { echo "FAIL: $WITNESS has no PT_GNU_EH_FRAME; libunwind cannot find .eh_frame" >&2; exit 1; }
relocs="$(readelf -rW "$WITNESS" | awk '$3 ~ /^R_/ && $3 != "R_X86_64_RELATIVE" { print $3 }' | sort -u)"
[[ -z "$relocs" ]] || { echo "FAIL: non-RELATIVE relocations: $relocs" >&2; exit 1; }
echo "CPP-HOSTED-LINK: PASS ($WITNESS: no unresolved symbols, libc++abi guard, PT_GNU_EH_FRAME, RELATIVE only)"
[[ "$LINK_ONLY" -eq 1 ]] && exit 0

echo "==> Building base cells (init, shell, vfs, config, input, platform, tools)..."
mkdir -p "$TH_DIR"
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
    /bin/cpp-hosted-smoke
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
cp "$KERNEL" "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-cpp-hosted"
echo "CPP_HOSTED_KERNEL=$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-cpp-hosted"
echo "==> Done"
