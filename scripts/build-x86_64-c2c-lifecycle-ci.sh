#!/usr/bin/env bash
# Build an isolated x86_64 QEMU C2C consumer image. The default workload is
# `bench local-service-lifecycle`; C2C_WORKLOAD=render packages the 3D renderer.
#
# WHAT IT PRODUCES
#   target/x86-c2c-${WORKLOAD}/x86_64-unknown-none/release/cellos-kernel
#   target/x86-c2c-${WORKLOAD}-embedded/{init,kernel_fs.img}
#   build/vicell-x86-c2c-${WORKLOAD}.iso
#
# WHY IT IS ITS OWN LANE
#   * the committed `kernel/src/embedded-x86_64/**` artifact every other x86 lane
#     ships is never written: the ramdisk goes to an override directory
#     (`EMBEDDED_OVERRIDE`), the kernel and cells to an isolated
#     `CARGO_TARGET_DIR`, and the ISO to its own root;
#   * it packages only the selected consumer cells (bench/bench-probe or
#     c2c-render plus trusted/domain workers), not unrelated application fixtures;
#   * it is a **production-feature** kernel (no `test-hooks`). Consumer spawn
#     authority is restricted by exact launch edges. The render workload also
#     bakes the signed operator policy; the default lifecycle image is unchanged.
#
# WHAT IT DOES NOT CHANGE
#   No public ABI, no syscall, no kernel source, no cell source. It only adds
#   cells to an image. The witness proves current behaviour; it fixes nothing.
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
WORKLOAD="${C2C_WORKLOAD:-lifecycle}"
case "$WORKLOAD" in
    lifecycle)
        WORKLOAD_PACKAGE="app-bench"
        WORKLOAD_CELLS=(bench bench-probe)
        ;;
    render)
        WORKLOAD_PACKAGE="app-c2c-render"
        WORKLOAD_CELLS=(c2c-render c2c-render-worker c2c-render-domain-worker)
        ;;
    *)
        echo "FAIL: unknown C2C_WORKLOAD=$WORKLOAD (expected lifecycle or render)" >&2
        exit 1
        ;;
esac
TARGET_DIR="target/x86-c2c-$WORKLOAD"
REL="$TARGET_DIR/$TARGET/release"
TH_DIR="target/x86-c2c-$WORKLOAD-embedded"
ISO_OUT="build/vicell-x86-c2c-$WORKLOAD.iso"
ISO_ROOT="build/x86-c2c-$WORKLOAD-iso-root"
KERNEL="$REL/cellos-kernel"

export CARGO_TARGET_DIR="$TARGET_DIR"
# x86_64 CELLS build with relocation-model=pic (the kernel profile in
# .cargo/config.toml keeps relocation-model=static). Freestanding CFLAGS keep host
# builtins out of the bare-metal link — byte-for-byte the CI x86 cell build.
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
# Invalidate every final/staged output before invoking Cargo: a failed rebuild must
# never leave a bootable kernel or ISO embedding the previous run's cells.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$KERNEL" "$ISO_OUT"

echo "==> Building base cells (init, shell, vfs, config, platform, drivers, sys tools)..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-shell -p service-vfs -p service-config -p service-platform
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p driver-nvme -p driver-e1000 -p app-sys-tools

echo "==> Building the $WORKLOAD workload cells..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p "$WORKLOAD_PACKAGE"

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
)
for cell in "${WORKLOAD_CELLS[@]}"; do
    CELL_BINARIES+=("$REL/$cell")
    CELL_IMAGE_PATHS+=("/bin/$cell")
done

echo "==> Verifying ${#CELL_BINARIES[@]} cell binaries..."
FAT_CELL_ARGS=()
for index in "${!CELL_BINARIES[@]}"; do
    if [[ ! -s "${CELL_BINARIES[$index]}" ]]; then
        echo "FAIL: expected nonempty cell binary not found: ${CELL_BINARIES[$index]}" >&2
        exit 1
    fi
    FAT_CELL_ARGS+=("${CELL_BINARIES[$index]}" "${CELL_IMAGE_PATHS[$index]}")
done

# The witness spawns `/bin/bench-probe` from `/bin/bench`, so the probe path is
# load-bearing: assert it is the string the scenario actually uses.
if [[ "$WORKLOAD" == lifecycle ]] && ! grep -qa -- "/bin/bench-probe" "$REL/bench"; then
    echo "FAIL: $REL/bench does not carry the /bin/bench-probe spawn path" >&2
    exit 1
fi

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

echo "==> Signing $(( ${#CELL_BINARIES[@]} + 1 )) cells (F1/F5)..."
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

if [[ "$WORKLOAD" == render ]]; then
    echo "==> Baking signed operator policy for the render consumer..."
    "$PYTHON_BIN" scripts/sign-policy.py --out "$TH_DIR/POLICY.BIN"
    FAT_CELL_ARGS+=("$TH_DIR/POLICY.BIN" /POLICY.BIN)
fi

echo "==> Assembling kernel_fs.img (VIFS1 ramdisk)..."
"$PYTHON_BIN" tools/mkfat32.py "$TH_DIR/kernel_fs.img" "${FAT_CELL_ARGS[@]}"
if [[ ! -s "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2
    exit 1
fi

TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
for image_name in "${WORKLOAD_CELLS[@]}"; do
    if ! grep -Fq -- "LFN '$image_name'" "$TMPDIR_KFS/fat-layout.txt"; then
        echo "FAIL: kernel_fs.img is missing /bin/$image_name:" >&2
        cat "$TMPDIR_KFS/fat-layout.txt" >&2
        exit 1
    fi
done
if [[ "$WORKLOAD" == render ]] && ! grep -Fq -- "SFN POLICY.BIN" "$TMPDIR_KFS/fat-layout.txt"; then
    echo "FAIL: render kernel_fs.img is missing root /POLICY.BIN" >&2
    exit 1
fi
echo "    kernel_fs.img: $(du -sh "$TH_DIR/kernel_fs.img" | cut -f1)"

# INIT_ELF is embedded separately from kernel_fs.img.
cp "$REL/app-init" "$TH_DIR/init"

echo "==> Building the production-feature kernel (no test-hooks)..."
# RUSTFLAGS carries the kernel profile: the target-config default is not applied
# to this package when RUSTFLAGS is set, so the two must agree.
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release \
    -p cellos-kernel \
    --target "$TARGET" \
    -Z build-std=core,alloc

if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi

# Assert the posture structurally rather than by reading the command line: a
# test-hooks kernel boots a different cell set and would not be this witness.
if grep -qa "S22-X86-DOMAIN-LIVE" "$KERNEL"; then
    echo "FAIL: $KERNEL carries test-hooks fixtures — it is not a production image" >&2
    exit 1
fi

echo "==> Building the bootable ISO..."
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash "$SCRIPT_DIR/x86/make-iso-ci.sh" "$ISO_OUT"

if [[ ! -s "$ISO_OUT" ]]; then
    echo "FAIL: ISO not produced at $ISO_OUT" >&2
    exit 1
fi

echo "X86_C2C_WORKLOAD=$WORKLOAD"
echo "X86_C2C_LIFECYCLE_KERNEL=$KERNEL"
echo "X86_C2C_LIFECYCLE_EMBEDDED=$TH_DIR"
echo "X86_C2C_LIFECYCLE_ISO=$ISO_OUT"
echo "==> Done"
