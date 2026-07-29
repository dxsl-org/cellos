#!/usr/bin/env bash
# Build the test-hooks kernel for VFS quota integration tests (Linux CI).
# Bash equivalent of scripts/build-test-hooks-cells.ps1 for Ubuntu runners.
#
# Produces: target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks
#
# Prerequisites (the CI job installs these):
#   apt: gcc-riscv64-unknown-elf libclang-dev qemu-system-misc
#   rustup: nightly with rust-src component
#
# POSIX only. Under Git Bash on Windows, MSYS rewrites the `/bin/...`
# destination arguments below into Windows paths and the image comes out with
# no /bin at all — run this in WSL2 or on Linux. The inspect_fat.py assertion
# further down fails loudly if that ever happens.

set -euo pipefail

# `python3` is not universal: some distros ship only `python`, and on Windows the
# bare name is the Microsoft Store alias stub. Probe once; the shared libs in
# lib-sign-cells.sh / lib-bake-policy.sh consume $PYTHON_BIN from here.
if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

REL="target/riscv64gc-unknown-none-elf/release"
TH_DIR="kernel/src/embedded-test-hooks"

# riscv64 cross-compiler required by littlefs2 C FFI (Ubuntu: gcc-riscv64-unknown-elf).
# Honor a pre-set compiler (local xpack riscv-none-elf-gcc); default to the CI one.
export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include}"

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

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

echo "==> Signing cells..."
sign_cells "$REL/app-init" "$REL/app-shell" "$REL/service-vfs" \
           "$REL/service-config" "$REL/vfs-test"

echo "==> Assembling kernel_fs.img (test-hooks)..."
mkdir -p "$TH_DIR"
TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-test' > "$TMPDIR_KFS/hostname"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$TMPDIR_KFS/POLICY.BIN"

"$PYTHON_BIN" tools/mkfat32.py \
    "$TH_DIR/kernel_fs.img" \
    "$REL/app-init"         /bin/init \
    "$REL/app-shell"        /bin/shell \
    "$REL/service-vfs"      /bin/vfs \
    "$REL/service-config"   /bin/config \
    "$REL/vfs-test"         /bin/vfs-test \
    "$TMPDIR_KFS/hostname"  /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

if [[ ! -f "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce kernel_fs.img" >&2; exit 1
fi
# Prove the layout rather than trusting the exit code. mkfat32.py exits 0 for a
# well-formed image whose destination paths went astray, so a /bin-less image
# only surfaces later as a confusing "cell not found" at boot.
"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$TMPDIR_KFS/fat-layout.txt" ||
   ! grep -q -- "LFN 'vfs-test'" "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img does not contain /bin/vfs-test" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi
assert_policy_in_image "$TMPDIR_KFS/fat-layout.txt" || exit 1
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
