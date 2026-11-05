#!/usr/bin/env bash
# Build the x86_64 image for the mlibc runtime witnesses (runtime plan slice RE).
#
# Produces:
#   target/x86-mlibc-witness/x86_64-unknown-none/release/cellos-kernel-mlibc-witness
#   target/x86-mlibc-witness-embedded/{init,kernel_fs.img}
# Boot it with: bash scripts/x86/qemu-mlibc-witness.sh
#
# Same shape as scripts/build-x86_64-domain-test-ci.sh (H-4: a new standalone
# script, the domain lane is not edited): a `test-hooks` kernel, so the Tier-2
# admission posture is open, built into its own CARGO_TARGET_DIR with its own
# embedded ramdisk. The image boots to the shell; the runner types the witness
# names. mlibc must be built first (`bash scripts/build-mlibc.sh`).

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-mlibc-witness"
TH_DIR="target/x86-mlibc-witness-embedded"

for need in third_party/mlibc/build-x86_64/libc.a third_party/mlibc/build-x86_64/sysroot/include; do
    [[ -e "$need" ]] || { echo "FAIL: $need missing; run: bash scripts/build-mlibc.sh" >&2; exit 1; }
done

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init"

echo "==> Building base cells (init, shell, vfs, config, input, platform, tools)..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
    -p app-sys-tools

echo "==> Building the mlibc witnesses..."
# Not built with -Z build-std: their libc.a and libgcc.a come from build-mlibc.sh.
cargo build --release --target "$TARGET" -p app-mlibc-smoke -p app-mlibc-posix-x86
# Capture the dump before matching: under pipefail, `nm | grep -q` fails when
# nm dies of SIGPIPE after grep's first match.
for witness in mlibc-smoke mlibc-posix-x86; do
    symbols="$(nm "$REL/$witness")"
    if ! grep -q ' T __cellos_libc_start$' <<<"$symbols"; then
        echo "FAIL: $REL/$witness lacks mlibc's strong __cellos_libc_start" >&2
        exit 1
    fi
done
if ! grep -q ' T mlibc_posix_witness$' <<<"$(nm "$REL/mlibc-posix-x86")"; then
    echo "FAIL: mlibc-posix-x86 was built without its C witness (NOT-BUILT path)" >&2
    exit 1
fi

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps" "$REL/free"
    "$REL/mlibc-smoke" "$REL/mlibc-posix-x86"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/ls /bin/cat /bin/echo /bin/ps /bin/free
    /bin/mlibc-smoke /bin/mlibc-posix-x86
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
cp "$KERNEL" "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-mlibc-witness"
echo "MLIBC_WITNESS_KERNEL=$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-mlibc-witness"
echo "==> Done"
