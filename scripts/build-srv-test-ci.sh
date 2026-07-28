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

# On Windows the bare name `python3` is the Microsoft Store alias stub, which
# exits without running anything. Probe for an interpreter that actually works.
if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

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

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

echo "==> Signing cells..."
sign_cells "$REL/app-init" "$REL/app-shell" "$REL/service-vfs" "$REL/service-config" \
           "$REL/srv-test" "$REL/platform" "$REL/driver-virtio-blk"

echo "==> Assembling kernel_fs.img (srv-test)..."
mkdir -p "$SRV_DIR"
# Keep the temp dir inside target/: a POSIX /tmp path from Git Bash is not a
# path a native Windows Python can open.
TMPDIR_KFS=$(mktemp -d "target/srv-test-tmp.XXXXXX")
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-srv-test' > "$TMPDIR_KFS/hostname"

# MSYS2_ARG_CONV_EXCL: without it Git Bash rewrites every /bin/... DESTINATION
# argument into a Windows path before Python sees it, and mkfat32.py silently
# builds an image containing "C:/Program Files/Git/bin/..." instead of /bin/*.
# The kernel then boots, mounts VIFS1, and finds none of its cells.
MSYS2_ARG_CONV_EXCL='*' "$PYTHON_BIN" tools/mkfat32.py \
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
# Prove the layout rather than trusting the exit code — a mangled destination
# path produces a well-formed image that simply has no /bin.
"$PYTHON_BIN" tools/inspect_fat.py "${SRV_DIR}/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- "LFN 'srv-test'" "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img does not contain /bin/srv-test" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
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
