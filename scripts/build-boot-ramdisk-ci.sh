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
# Keep the temp dir inside target/: a POSIX /tmp path from Git Bash is not a
# path a native Windows Python can open.
TMPDIR_KFS=$(mktemp -d "target/boot-ramdisk-tmp.XXXXXX")
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell' > "$TMPDIR_KFS/hostname"
printf 'Welcome to ViCell!' > "$TMPDIR_KFS/readme"

# Signed operator policy. sign-policy.py round-trip-decodes the blob before it
# writes, so an entry outside the kernel's domain masks fails HERE rather than
# turning into PolicyState::Invalid → DenyAll on every booted device.
#
# The blob is signed with the DEV fleet key, which only verifies while the kernel
# carries the default `dev-policy-key` feature. Building an image that contains
# this blob WITHOUT that feature makes the policy Invalid → every cell outside
# vfs/shell/net boots with no capabilities.
"$PYTHON_BIN" scripts/sign-policy.py --out "$TMPDIR_KFS/POLICY.BIN" >/dev/null

# MSYS2_ARG_CONV_EXCL: without it Git Bash rewrites every /bin/... DESTINATION
# argument into a Windows path before Python sees it, and mkfat32.py silently
# builds an image containing "C:/Program Files/Git/bin/..." instead of /bin/*.
# The kernel then boots, mounts VIFS1, and finds none of its cells — while the
# boot gate's "FAT16 mounted" oracle still reports success.
MSYS2_ARG_CONV_EXCL='*' "$PYTHON_BIN" tools/mkfat32.py \
    "$EMB/kernel_fs.img" \
    "$REL/app-init"          /bin/init \
    "$REL/app-shell"         /bin/shell \
    "$REL/service-vfs"       /bin/vfs \
    "$REL/service-config"    /bin/config \
    "$REL/platform"          /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$TMPDIR_KFS/hostname"   /etc/hostname \
    "$TMPDIR_KFS/readme"     /readme.txt \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

# Prove the layout rather than trusting the exit code — a mangled destination
# path produces a well-formed image that simply has no /bin.
"$PYTHON_BIN" tools/inspect_fat.py "$EMB/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- "LFN 'vfs'" "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img does not contain /bin/vfs" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi
# POLICY.BIN is read by the kernel at /POLICY.BIN (root, 8.3-uppercase). If it
# lands anywhere else the kernel reports "absent" and silently falls back to the
# dev-permissive posture — a policy that does nothing, which is worse than none
# because the image looks provisioned.
if ! grep -q -- "SFN POLICY.BIN" "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img has no /POLICY.BIN in the root directory" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi

# INIT_ELF (include_bytes! in main.rs) is embedded separately from kernel_fs.img;
# refresh it so the committed copy can never go stale relative to the image.
cp "$REL/app-init" "$EMB/init"

echo "==> Done: $(du -sh "$EMB/kernel_fs.img" | cut -f1) at $EMB/kernel_fs.img"
