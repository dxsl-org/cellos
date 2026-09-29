#!/usr/bin/env bash
# Build an RPi3 Tier-3 image with the required Linux guest files in VIFS1.
# --volatile-disk selects the QEMU raspi3b (no SD backend) profile. The default
# packages /mnt/sd/guest_disk.img on a bootable Pi SD image for physical boards.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
SKIP_FETCH=0
VOLATILE=0
for arg in "$@"; do
    case "$arg" in
        --skip-fetch) SKIP_FETCH=1 ;;
        --volatile-disk) VOLATILE=1 ;;
        *) echo "ERROR: unknown option: $arg" >&2; exit 2 ;;
    esac
done

TARGET=aarch64-unknown-none-softfloat
BIN_DIR="target/$TARGET/release"
CACHE=.alpine-cache
EMBEDDED=target/rpi3-hv-embedded
SD_IMAGE=disk_rpi3_hv.img
FIRMWARE=tools/rpi3-firmware
PYTHON_BIN="${PYTHON_BIN:-python3}"
if ! command -v aarch64-linux-gnu-objcopy >/dev/null 2>&1; then
    echo 'ERROR: aarch64-linux-gnu-objcopy required to produce Pi firmware/QEMU raw kernel8.img' >&2
    exit 1
fi

if (( ! SKIP_FETCH )); then
    bash scripts/fetch-alpine-artifacts.sh "$CACHE"
fi
for file in "$CACHE/vmlinuz-virt" "$CACHE/initramfs-virt"; do
    if [[ ! -s "$file" ]]; then
        echo "ERROR: required Linux guest file missing or empty: $file" >&2
        exit 1
    fi
done

# Alpine vmlinuz-virt is an EFI zboot envelope; the VMM needs the raw
# ARM64 Image. Validate its header and ensure both assets fit in 128 MiB.
"$PYTHON_BIN" - "$CACHE/vmlinuz-virt" "$CACHE/Image-rpi3-hv" "$CACHE/initramfs-virt" <<'PY'
import gzip
import os
import struct
import sys

source, output, initrd = sys.argv[1:]
with open(source, 'rb') as f:
    header = f.read(64)
    if header[4:8] == b'zimg':
        offset, size = struct.unpack_from('<II', header, 8)
        if offset < 64 or offset + size > os.path.getsize(source):
            sys.exit('ERROR: invalid EFI zboot payload bounds')
        f.seek(offset)
        payload = f.read(size)
        try:
            image = gzip.decompress(payload)
        except (OSError, EOFError) as exc:
            sys.exit(f'ERROR: invalid EFI zboot gzip payload: {exc}')
    else:
        f.seek(0)
        image = f.read()
if len(image) < 64 or image[56:60] != b'ARMd':
    sys.exit('ERROR: guest Linux kernel is not a raw ARM64 Image')
text_offset, image_size = struct.unpack_from('<QQ', image, 8)
align2m = lambda x: (x + 0x1fffff) & ~0x1fffff
entry = align2m(text_offset)
initrd_start = entry + align2m(max(image_size, len(image)))
end = initrd_start + align2m(os.path.getsize(initrd)) + 0x200000
if end > 128 * 1024 * 1024:
    sys.exit(f'ERROR: guest kernel/initrd/DTB require {end} bytes, limit is 128 MiB')
with open(output, 'wb') as f:
    f.write(image)
print(f'[rpi3-hv] raw ARM64 Image: {output} ({len(image)} bytes)')
PY

# Build only the boot cells required by the minimal hypervisor profile.
# Keep the same PIC/BTI/PAC and bindgen target defaults as make-hypervisor-fs.sh.
export RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -I$ROOT/third_party/freestanding-include}"

HV_FEATURES=service-hypervisor/board-rpi3
if (( VOLATILE )); then
    HV_FEATURES+=,service-hypervisor/volatile-disk
fi
cargo build --release --target "$TARGET" --no-default-features \
    -p app-init -p service-vfs -p service-hypervisor \
    --features "app-init/board-rpi3,app-init/hypervisor-min,$HV_FEATURES"
cargo build --release --target "$TARGET" -p service-net

# Fail closed on every boot-critical cell: omitting a path creates a successful
# but unbootable VIFS1 image, and /vmlinuz plus /initrd.gz must always be there.
# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
sign_cells "$BIN_DIR/app-init" "$BIN_DIR/service-vfs" "$BIN_DIR/service-net" "$BIN_DIR/hypervisor"

mkdir -p "$EMBEDDED"
POLICY_TMP="$(mktemp -d)"
trap 'rm -rf "$POLICY_TMP"' EXIT
# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$POLICY_TMP/POLICY.BIN"
"$PYTHON_BIN" tools/mkfat32.py "$EMBEDDED/kernel_fs.img" \
    "$BIN_DIR/app-init" bin/init \
    "$BIN_DIR/service-vfs" bin/vfs \
    "$BIN_DIR/service-net" bin/net \
    "$BIN_DIR/hypervisor" bin/hypervisor \
    "$CACHE/Image-rpi3-hv" vmlinuz \
    "$CACHE/initramfs-virt" initrd.gz \
    "$POLICY_TMP/POLICY.BIN" POLICY.BIN
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$POLICY_TMP/layout.txt"
assert_policy_in_image "$POLICY_TMP/layout.txt"
if ! grep -qiF -- '--- /bin ---' "$POLICY_TMP/layout.txt"; then
    echo 'ERROR: required /bin directory absent from VIFS1' >&2
    exit 1
fi
for path in vmlinuz initrd.gz init vfs net hypervisor; do
    if ! grep -qiF "LFN '$path'" "$POLICY_TMP/layout.txt"; then
        echo "ERROR: required guest image entry absent from VIFS1: $path" >&2
        exit 1
    fi
done
cp "$BIN_DIR/app-init" "$EMBEDDED/init"

EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release --target "$TARGET" \
    -p cellos-kernel --features board-rpi3
KERNEL="$BIN_DIR/cellos-kernel"
if [[ ! -s "$KERNEL" ]]; then
    echo "ERROR: RPi3 kernel ELF missing: $KERNEL" >&2
    exit 1
fi
# QEMU raspi3b -kernel must load a raw kernel8.img just like VideoCore firmware:
# loading the ELF directly can enter the wrong exception level for Pi startup.
RAW_KERNEL="$EMBEDDED/kernel8.img"
aarch64-linux-gnu-objcopy -O binary "$KERNEL" "$RAW_KERNEL"
if [[ ! -s "$RAW_KERNEL" ]]; then
    echo "ERROR: Pi raw kernel missing: $RAW_KERNEL" >&2
    exit 1
fi

if (( ! VOLATILE )); then
    for program in mformat mcopy mkfs.ext4; do
        if ! command -v "$program" >/dev/null 2>&1; then
            echo "ERROR: required Pi SD image tool not installed: $program" >&2
            exit 1
        fi
    done
    for file in bootcode.bin start.elf fixup.dat config.txt; do
        if [[ ! -s "$FIRMWARE/$file" ]]; then
            echo "ERROR: required Pi firmware missing: $FIRMWARE/$file" >&2
            exit 1
        fi
    done
    BOOT_IMG="$POLICY_TMP/boot.img"
    DATA_IMG="$POLICY_TMP/cell.img"
    GUEST_DISK="$POLICY_TMP/guest_disk.img"
    truncate -s 256M "$BOOT_IMG"
    truncate -s 256M "$DATA_IMG"
    truncate -s 64M "$GUEST_DISK"
    mkfs.ext4 -q -F "$GUEST_DISK"
    mformat -i "$BOOT_IMG" -F -v CELLOS-BOOT ::
    mformat -i "$DATA_IMG" -F -v CELLOS-CELL ::
    for file in bootcode.bin start.elf fixup.dat config.txt; do
        mcopy -i "$BOOT_IMG" "$FIRMWARE/$file" "::$file"
    done
    if [[ -s "$FIRMWARE/bcm2710-rpi-3-b.dtb" ]]; then
        mcopy -i "$BOOT_IMG" "$FIRMWARE/bcm2710-rpi-3-b.dtb" ::
    fi
    mcopy -i "$BOOT_IMG" "$RAW_KERNEL" ::kernel8.img
    mcopy -i "$BOOT_IMG" "$GUEST_DISK" ::guest_disk.img
    # raspi3b models an SD card only when its byte length is a power of two.
    # The two 256 MiB partitions plus the 1 MiB MBR gap need >512 MiB.
    truncate -s 1G "$SD_IMAGE"
    "$PYTHON_BIN" tools/write-rpi3-mbr.py "$SD_IMAGE"
    dd if="$BOOT_IMG" of="$SD_IMAGE" bs=512 seek=2048 conv=notrunc status=none
    dd if="$DATA_IMG" of="$SD_IMAGE" bs=512 seek=526336 conv=notrunc status=none
    echo "[rpi3-hv] physical SD image: $SD_IMAGE (/mnt/sd/guest_disk.img on P1)"
else
    echo '[rpi3-hv] volatile disk requested; no SD image created (guest disk is nonpersistent)'
fi

echo "[rpi3-hv] Pi guest VIFS1: $EMBEDDED/kernel_fs.img"
echo "[rpi3-hv] QEMU raspi3b -kernel raw Pi image: $RAW_KERNEL (EL2 entry)"
echo '[rpi3-hv] QEMU mini UART is serial1; use -serial null -serial stdio'
