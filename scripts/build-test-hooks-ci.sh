#!/usr/bin/env bash
# Build the test-hooks kernel for VFS quota integration tests (Linux CI).
# Bash equivalent of scripts/build-test-hooks-cells.ps1 for Ubuntu runners.
#
# Produces: target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks
#
# Prerequisites (the CI job installs these):
#   apt: gcc-riscv64-unknown-elf libclang-dev qemu-system-misc
#   rustup: nightly with rust-src component

set -euo pipefail

REL="target/riscv64gc-unknown-none-elf/release"
TH_DIR="kernel/src/embedded-test-hooks"

# riscv64 cross-compiler required by littlefs2 C FFI (Ubuntu: gcc-riscv64-unknown-elf).
export CC_riscv64gc_unknown_none_elf="riscv64-unknown-elf-gcc"
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS"

echo "==> Building base cells (init, shell, config)..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-config

echo "==> Building test-hooks cells (service-vfs, app-vfs-test)..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p service-vfs --features test-hooks

cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-vfs-test --features test-hooks

echo "==> Verifying cell binaries..."
for bin in app-init app-shell service-vfs service-config vfs-test; do
    if [[ ! -f "$REL/$bin" ]]; then
        echo "FAIL: missing required binary: $REL/$bin" >&2; exit 1
    fi
done

echo "==> Assembling kernel_fs.img (test-hooks)..."
mkdir -p "$TH_DIR"
TMPDIR_KFS=$(mktemp -d)
printf 'ViCell-test' > "$TMPDIR_KFS/hostname"

python3 tools/mkfat32.py \
    "$TH_DIR/kernel_fs.img" \
    "$REL/app-init"         /bin/init \
    "$REL/app-shell"        /bin/shell \
    "$REL/service-vfs"      /bin/vfs \
    "$REL/service-config"   /bin/config \
    "$REL/vfs-test"         /bin/vfs-test \
    "$TMPDIR_KFS/hostname"  /etc/hostname

if [[ ! -f "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce kernel_fs.img" >&2; exit 1
fi
echo "   kernel_fs.img: $(du -sh "$TH_DIR/kernel_fs.img" | cut -f1)"

# Kernel embed: INIT_ELF (include_bytes!) is separate from kernel_fs.img.
# Copy our freshly-built init so EMBEDDED_OVERRIDE picks it up.
cp "$REL/app-init" "$TH_DIR/init"
echo "   init: $(du -sh "$TH_DIR/init" | cut -f1)"

echo "==> Building test-hooks kernel (riscv64, PIC)..."
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p vicell-kernel

cp "$REL/vicell-kernel" "$REL/vicell-kernel-test-hooks"
echo "==> Done: $REL/vicell-kernel-test-hooks"
