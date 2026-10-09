#!/usr/bin/env bash
# Build the x86_64 Tier-2 domain-entry test image (phase 02, step 2, x86 half).
#
# Produces:
#   target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel
#   target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test
#   target/x86-domain-test-embedded/{init,kernel_fs.img}
# Boot it with: bash scripts/x86/qemu-domain-test.sh <kernel-elf> [iso]
#
# Why this is a separate lane and not the CI boot-to-shell image:
#   * the kernel needs `--features test-hooks`, and `test-hooks` + `native-domains`
#     is what reopens `switch_ordering_qualified()` for x86_64 — the production
#     image (same cells absent, feature off) must keep refusing a domain launch,
#     which `scripts/qemu-x86_64-test.sh` still asserts;
#   * the embedded VIFS1 ramdisk is assembled into an override directory
#     (`target/x86-domain-test-embedded`), so the committed
#     `kernel/src/embedded-x86_64` artifact other x86 lanes ship is never written;
#   * the kernel is built into an isolated CARGO_TARGET_DIR, so the test-hooks
#     kernel can never be picked up as `…/release/cellos-kernel` by an unrelated
#     x86 lane (a test-hooks kernel boots a different cell set).
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-domain-test"
TH_DIR="target/x86-domain-test-embedded"

# x86_64 CELLS build with relocation-model=pic (the kernel profile in
# .cargo/config.toml keeps relocation-model=static). Freestanding CFLAGS keep
# host-cc builtins out of the bare-metal link — byte-for-byte the CI x86 cell
# build (`.github/workflows/ci.yml`, `qemu-x86_64-boot`).
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
# Invalidate every final/staged output before invoking Cargo: a failed rebuild
# must never leave a bootable kernel embedding the previous run's cells.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init"

echo "==> Building base cells (init with tier2-entry, shell, vfs, config, platform, drivers)..."
# `tier2-entry` is init-only: a multi-package `--features` would have to exist on
# every selected package, so init is built on its own.
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    --features tier2-entry,tier2-rpc-entry \
    -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-shell -p service-vfs -p service-config -p service-platform
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p driver-nvme -p driver-e1000 -p app-sys-tools

echo "==> Building Tier-2 domain cells (tier2-smoke, tier2-exploit)..."
# Both carry a `PROTECTION_CLASS_UNTRUSTED` manifest, which is what makes the
# loader classify them domain-class. They are the only cells in this image the
# admission policy can admit to a private root.
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p tier2-smoke -p tier2-exploit

echo "==> Building the cross-tier exchange cells (tier2-rpc-*), phase-02 slice B..."
# The provider carries `PROTECTION_CLASS_UNTRUSTED` (domain-class); the driver is
# an ordinary Tier-1 Cell. Neither holds a capability: the copied IPC path and the
# shared `LookupService`/`LookupServiceBound` allowlist bit are all this fixture
# needs, and `tier2-rpc-entry` in init is what schedules the pair.
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p tier2-rpc-provider -p tier2-rpc-driver

CELL_BINARIES=(
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/platform"
    "$REL/driver-nvme"
    "$REL/driver-e1000"
    "$REL/ls"
    "$REL/cat"
    "$REL/echo"
    "$REL/ps"
    "$REL/tier2-smoke"
    "$REL/tier2-exploit"
    "$REL/tier2-rpc-provider"
    "$REL/tier2-rpc-driver"
)
CELL_IMAGE_PATHS=(
    /bin/shell
    /bin/vfs
    /bin/config
    /bin/platform
    /bin/nvme
    /bin/e1000
    /bin/ls
    /bin/cat
    /bin/echo
    /bin/ps
    /bin/tier2-smoke
    /bin/tier2-exploit
    /bin/tier2-rpc-provider
    /bin/tier2-rpc-driver
)

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

echo "==> Signing ${#CELL_BINARIES[@]} cells + init (F1/F5)..."
# Signed cells keep the same classification (the manifest's protection class
# decides domain-class-ness either way) and satisfy the image's signature gate.
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

echo "==> Assembling kernel_fs.img (VIFS1 ramdisk)..."
"$PYTHON_BIN" tools/mkfat32.py \
    "$TH_DIR/kernel_fs.img" \
    "${FAT_CELL_ARGS[@]}"

if [[ ! -s "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2
    exit 1
fi

TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
awk '/--- \/bin ---/ { capture = 1; next } capture && (/dir \(SFN=/ || /^--- /) { exit } capture' \
    "$TMPDIR_KFS/fat-layout.txt" > "$TMPDIR_KFS/bin-layout.txt"
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
echo "   kernel_fs.img: $(du -sh "$TH_DIR/kernel_fs.img" | cut -f1)"

# INIT_ELF is embedded separately from kernel_fs.img.
cp "$REL/app-init" "$TH_DIR/init"
echo "   init: $(du -sh "$TH_DIR/init" | cut -f1)"

echo "==> Building test-hooks kernel (x86_64, static relocation)..."
EMBEDDED_OVERRIDE="$TH_DIR" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-D warnings -C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release \
    -p cellos-kernel \
    --features test-hooks \
    --target "$TARGET" \
    -Z build-std=core,alloc

KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi
cp "$KERNEL" "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-domain-test"
echo "DOMAIN_TEST_KERNEL=$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel-domain-test"
echo "DOMAIN_TEST_EMBEDDED=$TH_DIR"
echo "==> Done"
