#!/usr/bin/env bash
# Build the test-hooks kernel for AArch64 integration tests (Linux CI).
#
# Produces: target/aarch64-unknown-none-softfloat/release/cellos-kernel-test-hooks
#
# POSIX only.

set -euo pipefail

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

echo "==> Building base cells (init, shell, config)..."
cargo build --release \
    --target aarch64-unknown-none-softfloat \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-config

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

echo "==> Verifying cell binaries..."
for bin in app-init app-shell service-vfs service-config vfs-test service-net driver-virtio-net atomic-publication-probe service-input; do
    if [[ ! -f "$REL/$bin" ]]; then
        echo "FAIL: expected cell binary not found: $REL/$bin" >&2
        exit 1
    fi
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

echo "==> Signing cells..."
sign_cells "$REL/app-init" "$REL/app-shell" "$REL/service-vfs" \
           "$REL/service-config" "$REL/vfs-test" "$REL/service-net" \
           "$REL/driver-virtio-net" "$REL/atomic-publication-probe" \
           "$REL/service-input"

echo "==> Assembling kernel_fs.img (test-hooks)..."
mkdir -p "$TH_DIR"
TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-aarch64-test' > "$TMPDIR_KFS/hostname"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$TMPDIR_KFS/POLICY.BIN"

"$PYTHON_BIN" tools/mkfat32.py \
    "$TH_DIR/kernel_fs.img" \
    "$REL/app-init"                 /bin/init \
    "$REL/app-shell"                /bin/shell \
    "$REL/service-vfs"              /bin/vfs \
    "$REL/service-config"           /bin/config \
    "$REL/vfs-test"                 /bin/vfs-test \
    "$REL/service-net"              /bin/net \
    "$REL/driver-virtio-net"        /bin/virtio-net \
    "$REL/atomic-publication-probe" /bin/atomic-probe \
    "$REL/service-input"            /bin/input \
    "$TMPDIR_KFS/hostname"          /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN"        /POLICY.BIN

if [[ ! -f "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce kernel_fs.img" >&2; exit 1
fi

"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- 'init' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- 'vfs' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- 'vfs-test' "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img has invalid layout:" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi
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
