#!/usr/bin/env bash
# build-native-workload-ci.sh — Build the native-workload kernel for Phase 05 integration tests.
set -euo pipefail

if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

REL="target/riscv64gc-unknown-none-elf/release"
WORKLOAD_DIR="kernel/src/embedded-native-workload"

echo "==> Building base cells..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-config \
    -p service-platform -p driver-virtio-blk

echo "==> Building service-vfs..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p service-vfs

echo "==> Building supervisor with hostile-backend-recovery..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p supervisor --features hostile-backend-recovery

echo "==> Building hotswap demos..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p hotswap-demo-v1 -p hotswap-demo-v2

echo "==> Building app-bench..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-bench

source scripts/lib-sign-cells.sh

echo "==> Signing cells..."
sign_cells "$REL/app-init" "$REL/app-shell" "$REL/service-vfs" "$REL/service-config" \
           "$REL/platform" "$REL/driver-virtio-blk" "$REL/supervisor" \
           "$REL/hotswap-demo-v1" "$REL/hotswap-demo-v2" "$REL/bench" "$REL/bench-probe"

echo "==> Assembling kernel_fs.img (native-workload)..."
mkdir -p "$WORKLOAD_DIR"
TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-native-workload' > "$TMPDIR_KFS/hostname"

source scripts/lib-bake-policy.sh
bake_policy "$TMPDIR_KFS/POLICY.BIN"

"$PYTHON_BIN" tools/mkfat32.py \
    "$WORKLOAD_DIR/kernel_fs.img" \
    "$REL/app-init"          /bin/init \
    "$REL/app-shell"         /bin/shell \
    "$REL/service-vfs"       /bin/vfs \
    "$REL/service-config"    /bin/config \
    "$REL/platform"          /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$REL/supervisor"        /bin/supervisor \
    "$REL/hotswap-demo-v1"   /bin/hotswap-demo-v1 \
    "$REL/hotswap-demo-v2"   /bin/hotswap-demo-v2 \
    "$REL/bench"             /bin/bench \
    "$REL/bench-probe"       /bin/bench-probe \
    "$TMPDIR_KFS/hostname"   /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

if [[ ! -f "$WORKLOAD_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce kernel_fs.img" >&2; exit 1
fi
"$PYTHON_BIN" tools/inspect_fat.py "${WORKLOAD_DIR}/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
assert_policy_in_image "$TMPDIR_KFS/fat-layout.txt" || exit 1
echo "   kernel_fs.img: $(du -sh "$WORKLOAD_DIR/kernel_fs.img" | cut -f1)"

cp "$REL/app-init" "$WORKLOAD_DIR/init"
echo "   init: $(du -sh "$WORKLOAD_DIR/init" | cut -f1)"

echo "==> Building native-workload kernel (riscv64, PIC)..."
EMBEDDED_OVERRIDE="$WORKLOAD_DIR" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p cellos-kernel

cp "$REL/cellos-kernel" "$REL/cellos-kernel-native-workload"
echo "==> Done: $REL/cellos-kernel-native-workload"
