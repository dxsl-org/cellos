#!/usr/bin/env bash
# Build the current Phase 04 boot set, VIFS1 ramdisk, kernel, and scratch disk.

set -euo pipefail

TARGET="${1:?usage: build-phase04-qemu-image.sh <target> [disk.img]}"
DISK_OUT="${2:-phase04-${TARGET}.img}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

case "$TARGET" in
  riscv64gc-unknown-none-elf)
    EMBEDDED="kernel/src/embedded"
    export CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="-C relocation-model=pic"
    export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
    export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
    ;;
  aarch64-unknown-none-softfloat)
    EMBEDDED="kernel/src/embedded-aarch64"
    export CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
    export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
    export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
    export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat:---target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu}"
    ;;
  *)
    echo "FAIL: unsupported Phase 04 target: $TARGET" >&2
    exit 2
    ;;
esac

PACKAGES=(
  app-init app-shell service-platform service-vfs service-config
  driver-virtio-blk driver-virtio-net driver-virtio-gpu
  service-input service-net service-compositor service-net-broker supervisor
)

PACKAGE_ARGS=()
for package in "${PACKAGES[@]}"; do
  PACKAGE_ARGS+=( -p "$package" )
done

echo "[phase04-image] building ${#PACKAGES[@]} cells for $TARGET"
cargo build --release --target "$TARGET" "${PACKAGE_ARGS[@]}"

REL="target/$TARGET/release"
PAIRS=(
  app-shell /bin/shell
  platform /bin/platform
  service-vfs /bin/vfs
  service-config /bin/config
  driver-virtio-blk /bin/block
  driver-virtio-net /bin/virtio-net
  driver-virtio-gpu /bin/virtio-gpu
  service-input /bin/input
  service-net /bin/net
  service-compositor /bin/compositor
  service-net-broker /bin/net-broker
  supervisor /bin/supervisor
)

for ((index = 0; index < ${#PAIRS[@]}; index += 2)); do
  binary="${PAIRS[index]}"
  [[ -f "$REL/$binary" ]] || {
    echo "FAIL: required cell missing: $REL/$binary" >&2
    exit 1
  }
done
[[ -f "$REL/app-init" ]] || { echo "FAIL: app-init missing" >&2; exit 1; }

mkdir -p "$EMBEDDED"
cp "$REL/app-init" "$EMBEDDED/init"

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TEMP_DIR"' EXIT
python3 scripts/sign-policy.py --out "$TEMP_DIR/POLICY.BIN" >/dev/null
printf 'Cellos-Phase04\n' > "$TEMP_DIR/hostname"

IMAGE_ARGS=( "$EMBEDDED/kernel_fs.img" )
for ((index = 0; index < ${#PAIRS[@]}; index += 2)); do
  IMAGE_ARGS+=( "$REL/${PAIRS[index]}" "${PAIRS[index + 1]}" )
done
IMAGE_ARGS+=( "$TEMP_DIR/POLICY.BIN" /POLICY.BIN )
python3 tools/mkfat32.py "${IMAGE_ARGS[@]}"

LAYOUT="$TEMP_DIR/layout.txt"
python3 tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$LAYOUT"
for required in "LFN 'block'" "LFN 'input'" "LFN 'virtio-gpu'" "SFN POLICY.BIN"; do
  grep -q -- "$required" "$LAYOUT" || {
    echo "FAIL: VIFS1 missing $required" >&2
    exit 1
  }
done

python3 tools/mkfat32.py "$DISK_OUT" "$TEMP_DIR/hostname" /etc/hostname
echo "[phase04-image] building kernel for $TARGET"
cargo build --release --target "$TARGET" -p cellos-kernel

echo "PHASE04_KERNEL=$REL/cellos-kernel"
echo "PHASE04_DISK=$DISK_OUT"
