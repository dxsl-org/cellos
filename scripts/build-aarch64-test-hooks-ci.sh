#!/usr/bin/env bash
# Build the test-hooks kernel for AArch64 integration tests (Linux CI).
#
# Produces: target/aarch64-unknown-none-softfloat/release/cellos-kernel-test-hooks
# Development-Silo lane:
#   CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO=1 bash scripts/build-aarch64-test-hooks-ci.sh
#
# Bash only.

set -euo pipefail

DEVELOPMENT_SILO_SWITCH="CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO"
DEVELOPMENT_SILO="${CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO:-0}"
case "$DEVELOPMENT_SILO" in
    0|1) ;;
    *)
        echo "FAIL: $DEVELOPMENT_SILO_SWITCH must be exactly 0 or 1" >&2
        exit 1
        ;;
esac
if [[ "$DEVELOPMENT_SILO" == "1" && -n "${CELLOS_PRODUCTION+x}" ]]; then
    echo "FAIL: $DEVELOPMENT_SILO_SWITCH=1 is forbidden when CELLOS_PRODUCTION is set" >&2
    exit 1
fi

resolve_development_llvm_objcopy() {
    local candidates candidate resolved version
    if [[ -n "${LLVM_OBJCOPY+x}" ]]; then
        candidates=("$LLVM_OBJCOPY")
    else
        candidates=(
            llvm-objcopy
            llvm-objcopy-18
            /usr/lib/llvm-18/bin/llvm-objcopy
        )
    fi

    for candidate in "${candidates[@]}"; do
        [[ -n "$candidate" ]] || continue
        resolved=$(command -v "$candidate" 2>/dev/null || true)
        [[ -n "$resolved" && -f "$resolved" && -x "$resolved" ]] || continue
        if [[ "$resolved" != /* ]]; then
            resolved="$PWD/$resolved"
        fi
        version=$("$resolved" --version 2>&1) || continue
        case "$version" in
            *LLVM*|*llvm*) ;;
            *) continue ;;
        esac
        LLVM_OBJCOPY="$resolved"
        export LLVM_OBJCOPY
        echo "==> Development-Silo guest objcopy: $LLVM_OBJCOPY"
        return 0
    done

    if [[ -n "${LLVM_OBJCOPY+x}" ]]; then
        echo "FAIL: explicit LLVM_OBJCOPY is not an executable LLVM objcopy: ${LLVM_OBJCOPY:-<empty>}" >&2
    else
        echo "FAIL: LLVM objcopy not found (tried: llvm-objcopy, llvm-objcopy-18, /usr/lib/llvm-18/bin/llvm-objcopy)" >&2
    fi
    return 1
}

if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    resolve_development_llvm_objcopy || exit 1
fi

if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN="python"
else
    echo "FAIL: python3/python not found on PATH" >&2
    exit 1
fi
export PYTHON_BIN
export OBJCOPY="${OBJCOPY:-aarch64-linux-gnu-objcopy}"

REL="target/aarch64-unknown-none-softfloat/release"
TH_DIR="kernel/src/embedded-test-hooks"

# Invalidate every final/staged output before invoking Cargo. A failed rebuild
# must never leave a bootable kernel that embeds cells from the opposite mode.
mkdir -p "$TH_DIR"
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$REL/cellos-kernel-test-hooks"

if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    echo "==> Building base cells (shell, config)..."
    cargo build --release \
        --target aarch64-unknown-none-softfloat \
        -Z build-std=core,alloc \
        -p app-shell -p service-config

    echo "==> Building development-Silo cells (init, service-silo, service-kms)..."
    SOURCE_DATE_EPOCH=1 CARGO_INCREMENTAL=0 \
    cargo build --locked --release \
        --target aarch64-unknown-none-softfloat \
        -Z build-std=core,alloc \
        --no-default-features \
        --features development-silo-provider \
        -p app-init -p service-silo -p service-kms

    echo "==> Building development-Silo containment probe (app-silo-test)..."
    SOURCE_DATE_EPOCH=1 CARGO_INCREMENTAL=0 \
    cargo build --locked --release \
        --target aarch64-unknown-none-softfloat \
        -Z build-std=core,alloc \
        -p app-silo-test
else
    echo "==> Building base cells (init, shell, config)..."
    cargo build --release \
        --target aarch64-unknown-none-softfloat \
        -Z build-std=core,alloc \
        -p app-init -p app-shell -p service-config
fi

echo "==> Building test-hooks cells (service-vfs, app-vfs-test, atomic-publication-probe)..."
cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    --no-default-features \
    --features test-hooks \
    -p service-vfs

cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    -p app-vfs-test --features test-hooks

cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    -p atomic-publication-probe

echo "==> Building stack-sizing paths (service-net, driver-virtio-net, service-input)..."
cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    -p service-net -p driver-virtio-net -p service-input

CELL_BINARIES=(
    "$REL/app-init"
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/vfs-test"
    "$REL/service-net"
    "$REL/driver-virtio-net"
    "$REL/atomic-publication-probe"
    "$REL/service-input"
)
CELL_IMAGE_PATHS=(
    /bin/init
    /bin/shell
    /bin/vfs
    /bin/config
    /bin/vfs-test
    /bin/net
    /bin/virtio-net
    /bin/atomic-probe
    /bin/input
)
if [[ "$DEVELOPMENT_SILO" == "1" ]]; then
    CELL_BINARIES+=("$REL/silo" "$REL/service-kms" "$REL/silo-test")
    CELL_IMAGE_PATHS+=(/bin/silo /bin/kms /bin/silo-test)
fi

echo "==> Verifying ${#CELL_BINARIES[@]} cell binaries..."
FAT_CELL_ARGS=()
for index in "${!CELL_BINARIES[@]}"; do
    binary="${CELL_BINARIES[$index]}"
    if [[ ! -s "$binary" ]]; then
        echo "FAIL: expected nonempty cell binary not found: $binary" >&2
        exit 1
    fi
    FAT_CELL_ARGS+=("$binary" "${CELL_IMAGE_PATHS[$index]}")
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

echo "==> Signing ${#CELL_BINARIES[@]} cells (mandatory F1/F5)..."
sign_cells "${CELL_BINARIES[@]}"

echo "==> Assembling kernel_fs.img (test-hooks)..."
TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-aarch64-test' > "$TMPDIR_KFS/hostname"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$TMPDIR_KFS/POLICY.BIN"

"$PYTHON_BIN" tools/mkfat32.py \
    "$TH_DIR/kernel_fs.img" \
    "${FAT_CELL_ARGS[@]}" \
    "$TMPDIR_KFS/hostname" /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

if [[ ! -s "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2; exit 1
fi

"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
sed -n '/--- \/bin ---/,$p' "$TMPDIR_KFS/fat-layout.txt" > "$TMPDIR_KFS/bin-layout.txt"
BIN_FILE_COUNT=$(grep -c -- ' attr=20 ' "$TMPDIR_KFS/bin-layout.txt" || true)
if [[ "$BIN_FILE_COUNT" -ne "${#CELL_IMAGE_PATHS[@]}" ]]; then
    echo "FAIL: kernel_fs.img contains $BIN_FILE_COUNT /bin cells; expected ${#CELL_IMAGE_PATHS[@]}:" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi
for image_path in "${CELL_IMAGE_PATHS[@]}"; do
    image_name="${image_path#/bin/}"
    if ! grep -Fq -- "-> LFN '$image_name'  attr=20" "$TMPDIR_KFS/bin-layout.txt"; then
        echo "FAIL: kernel_fs.img is missing exact path $image_path:" >&2
        cat "$TMPDIR_KFS/fat-layout.txt" >&2
        exit 1
    fi
done
assert_policy_in_image "$TMPDIR_KFS/fat-layout.txt" || exit 1
echo "   kernel_fs.img: $(du -sh "$TH_DIR/kernel_fs.img" | cut -f1)"

# Kernel embed: INIT_ELF is separate from kernel_fs.img.
cp "$REL/app-init" "$TH_DIR/init"
echo "   init: $(du -sh "$TH_DIR/init" | cut -f1)"

echo "==> Building test-hooks kernel (aarch64, PIC)..."
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    --features test-hooks \
    -p cellos-kernel

cp "$REL/cellos-kernel" "$REL/cellos-kernel-test-hooks"
echo "==> Done: $REL/cellos-kernel-test-hooks"
