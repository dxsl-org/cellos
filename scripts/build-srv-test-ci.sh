#!/usr/bin/env bash
# build-srv-test-ci.sh — Build the srv-test kernel for the RedoxFS /srv
# integration test (Linux CI).
#
# Produces: target/riscv64gc-unknown-none-elf/release/vicell-kernel-srv-test
#
# Key difference from build-test-hooks-ci.sh:
#   - service-vfs is built WITHOUT --features test-hooks (full quota + full
#     RedoxFS backend — no artificial 1.1 KiB limit).
#   - app-srv-test replaces app-vfs-test in the embedded kernel_fs.img.
#
# Prerequisites (the CI job installs these):
#   apt: gcc-riscv64-unknown-elf libclang-dev qemu-system-misc
#   rustup: nightly with rust-src component

set -euo pipefail

REL="target/riscv64gc-unknown-none-elf/release"
SRV_DIR="kernel/src/embedded-srv-test"

# Honor a pre-set compiler (local xpack riscv-none-elf-gcc); default to the CI one.
export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include}"

echo "==> Building base cells (init, shell, config, platform, block)..."
# platform + virtio-blk are REQUIRED: the /srv tests attach a disk, and
# without /bin/platform + /bin/block in VIFS1 the VFS has no block driver —
# every sector read fails and RedoxFS P5 can never open.
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-config \
    -p service-platform -p driver-virtio-blk

echo "==> Building service-vfs (full — no test-hooks, full quota, RedoxFS enabled)..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p service-vfs

echo "==> Building app-srv-test..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-srv-test

echo "==> Verifying cell binaries..."
for bin in app-init app-shell service-vfs service-config srv-test platform driver-virtio-blk; do
    if [[ ! -f "$REL/$bin" ]]; then
        echo "FAIL: missing required binary: $REL/$bin" >&2; exit 1
    fi
done

echo "==> Assembling kernel_fs.img (srv-test)..."
mkdir -p "$SRV_DIR"
TMPDIR_KFS=$(mktemp -d)
printf 'ViCell-srv-test' > "$TMPDIR_KFS/hostname"

python3 tools/mkfat32.py \
    "$SRV_DIR/kernel_fs.img" \
    "$REL/app-init"       /bin/init \
    "$REL/app-shell"      /bin/shell \
    "$REL/service-vfs"    /bin/vfs \
    "$REL/service-config" /bin/config \
    "$REL/platform"       /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$REL/srv-test"       /bin/srv-test \
    "$TMPDIR_KFS/hostname" /etc/hostname

if [[ ! -f "$SRV_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce kernel_fs.img" >&2; exit 1
fi
echo "   kernel_fs.img: $(du -sh "$SRV_DIR/kernel_fs.img" | cut -f1)"

cp "$REL/app-init" "$SRV_DIR/init"
echo "   init: $(du -sh "$SRV_DIR/init" | cut -f1)"

echo "==> Building srv-test kernel (riscv64, PIC)..."
EMBEDDED_OVERRIDE="$SRV_DIR" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p vicell-kernel

cp "$REL/vicell-kernel" "$REL/vicell-kernel-srv-test"
echo "==> Done: $REL/vicell-kernel-srv-test"
