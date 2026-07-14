#!/usr/bin/env bash
# Assemble a minimal bootable VIFS1 ramdisk (kernel/src/embedded/kernel_fs.img)
# for the CI Build (riscv64) job, so the uploaded kernel artifact actually
# boots in the QEMU boot gate. kernel_fs.img is gitignored (4-36 MB build
# artifact); without this step build.rs embeds an empty stub that compiles but
# cannot boot.
#
# Contents = the bootstrap chain only (loader::early::BOOTSTRAP_CELLS + init):
# everything else lives in the disk cell-store, which the boot gate does not
# need (no disk attached → gate asserts VIFS1 FAT16 mount, not shell).
#
# Run from the repo root BEFORE `cargo build --target riscv64gc-unknown-none-elf`.
# Prerequisites: gcc-riscv64-unknown-elf, libclang-dev (littlefs2-sys bindgen).

set -euo pipefail

REL="target/riscv64gc-unknown-none-elf/release"
EMB="kernel/src/embedded"

export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
# -I …/freestanding-include: Ubuntu's bare-metal cross gcc ships no libc
# headers; littlefs includes <string.h>. The vendored freestanding header set
# (already used for aarch64/x86_64 clang builds) fills the gap.
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include}"

echo "==> Building bootstrap cells (init, shell, vfs, config, platform, block)..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-vfs -p service-config \
    -p service-platform -p driver-virtio-blk

for bin in app-init app-shell service-vfs service-config platform driver-virtio-blk; do
    if [[ ! -f "$REL/$bin" ]]; then
        echo "FAIL: missing bootstrap binary: $REL/$bin" >&2; exit 1
    fi
done

echo "==> Assembling $EMB/kernel_fs.img..."
TMPDIR_KFS=$(mktemp -d)
printf 'ViCell' > "$TMPDIR_KFS/hostname"
printf 'Welcome to ViCell!' > "$TMPDIR_KFS/readme"

python3 tools/mkfat32.py \
    "$EMB/kernel_fs.img" \
    "$REL/app-init"          /bin/init \
    "$REL/app-shell"         /bin/shell \
    "$REL/service-vfs"       /bin/vfs \
    "$REL/service-config"    /bin/config \
    "$REL/platform"          /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$TMPDIR_KFS/hostname"   /etc/hostname \
    "$TMPDIR_KFS/readme"     /readme.txt
rm -rf "$TMPDIR_KFS"

# INIT_ELF (include_bytes! in main.rs) is embedded separately from kernel_fs.img;
# refresh it so the committed copy can never go stale relative to the image.
cp "$REL/app-init" "$EMB/init"

echo "==> Done: $(du -sh "$EMB/kernel_fs.img" | cut -f1) at $EMB/kernel_fs.img"
