#!/usr/bin/env bash
# Build an RPi3 Tier-3 image with the required Linux guest files in VIFS1.
# --volatile-disk selects the QEMU raspi3b (no SD backend) profile. The default
# packages /mnt/sd/guest_disk.img on a bootable Pi SD image for physical boards.
#
# --autostart preloads the Tier-3 VM at boot (`app-init/hv-autostart`). Without
# it the image boots to the Cellos shell with the guest idle and the operator
# starts it with `hv`; the QEMU boot gate and any server that wants a Tier-3 app
# to start fast pass the flag.
#
# INITRD_OVERRIDE=<file> replaces the guest initramfs this build packages. It is
# the same hook the x86 lane has, and exists so a guest-image experiment (for
# example an initramfs whose init runs a command before the shell) can be gated
# without editing the builder.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
SKIP_FETCH=0
VOLATILE=0
AUTOSTART=0
TIER3=1
UI=0
AI=0
MINIMAL_DRIVERS=0
SUPERVISOR=0
GUEST=alpine
# `while` + `shift`, not `for arg in "$@"`: a for loop iterates the original
# list, so a `shift` inside it never skips the option's value and
# `--guest alpine-wide` arrives as an unknown option.
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-fetch) SKIP_FETCH=1; shift ;;
        --volatile-disk) VOLATILE=1; shift ;;
        --autostart) AUTOSTART=1; shift ;;
        # Strongest way to turn Tier 3 off: the guest-hosting cell and the guest
        # files are not packaged at all, so nothing in the image can start one.
        --no-tier3) TIER3=0; shift ;;
        --ui) UI=1; shift ;;
        # Guest profile (one per image): alpine (128 MiB, default), alpine-wide
        # (256 MiB, for Python/Node/a headless browser) or alpine-gui (512 MiB,
        # for a guest that draws -- the presentation path is not built yet).
        --guest) GUEST="${2:?--guest needs a profile}"; shift 2 ;;
        --ai) AI=1; shift ;;
        # The hotswap supervisor cell (it also carries the hostile recovery
        # handler when that feature is on).
        --supervisor) SUPERVISOR=1; shift ;;
        # Trim the board to the drivers a bring-up image needs (console,
        # interrupts, timer, pinmux). `has_driver` gates real kernel init, so
        # this is a smaller kernel, not just a smaller label.
        --minimal-drivers) MINIMAL_DRIVERS=1; shift ;;
        *) echo "ERROR: unknown option: $1" >&2; exit 2 ;;
    esac
done
if (( AUTOSTART && !TIER3 )); then
    echo "ERROR: --autostart contradicts --no-tier3" >&2
    exit 2
fi

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
if (( TIER3 )); then
    for file in "$CACHE/vmlinuz-virt" "$CACHE/initramfs-virt"; do
        if [[ ! -s "$file" ]]; then
            echo "ERROR: required Linux guest file missing or empty: $file" >&2
            exit 1
        fi
    done
fi
if (( ! VOLATILE )); then
    [[ -s "$CACHE/modloop-virt" ]] || {
        echo "ERROR: persistent Pi guest needs Alpine modloop-virt for ext4" >&2
        exit 1
    }
    command -v unsquashfs >/dev/null || {
        echo "ERROR: persistent Pi guest needs unsquashfs" >&2
        exit 1
    }
fi


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

# Build only the Cells required by the Pi guest and its physical USB keyboard.
# Keep the same PIC/BTI/PAC and bindgen target defaults as make-hypervisor-fs.sh.
export RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -I$ROOT/third_party/freestanding-include}"

HV_FEATURES=service-hypervisor/board-rpi3
case "$GUEST" in
    alpine) ;;
    alpine-wide) HV_FEATURES+=,service-hypervisor/alpine-wide-guest ;;
    alpine-gui) HV_FEATURES+=,service-hypervisor/alpine-gui-guest ;;
    *) echo "ERROR: --guest must be alpine, alpine-wide or alpine-gui (got '$GUEST')" >&2; exit 2 ;;
esac
if (( VOLATILE )); then
    HV_FEATURES+=,service-hypervisor/volatile-disk
fi
INIT_FEATURES=app-init/board-rpi3,app-init/input
if (( !MINIMAL_DRIVERS )); then
    INIT_FEATURES+=,app-init/usb-host
fi
BUILD_PACKAGES=(-p app-init -p app-shell -p service-vfs -p service-input)
if (( TIER3 )); then
    INIT_FEATURES+=,app-init/tier3
    BUILD_PACKAGES+=(-p service-hypervisor)
    if (( AUTOSTART )); then
        INIT_FEATURES+=,app-init/tier3-autostart
    fi
fi
if (( UI )); then
    INIT_FEATURES+=,app-init/ui
fi
if (( AI )); then
    INIT_FEATURES+=,app-init/ai
fi
EXTRA_FEATURES="service-input/board-rpi3"
if (( TIER3 )); then
    EXTRA_FEATURES+=",$HV_FEATURES"
fi
# Diagnostic image hooks. Two independent switches, because they answer different
# questions and only one of them can be left on while the operator works in the
# guest:
#
#   CELLOS_DEBUG_TRACE=1  the low-rate traces that place a fault from one board
#                         log: the hypervisor's L2 exchange outcomes
#                         (`[hv-l2] <op> … result=<reason>`), which name which
#                         half of the guest↔Net Cell bridge broke.
#   CELLOS_DEBUG_LOOP=1   the per-turn loops: the kernel's refused-TrySend
#                         reasons (`cellos-kernel/ipc-trace`), the Net Cell's
#                         `[net-loop]` heartbeat and the driver's `[dwc2-loop]`.
#                         They print about once per turn, so they belong to
#                         measuring turn cost — with them on, the guest console
#                         the operator is typing into is buried.
#
# Both unset in a normal build; all three features are diagnostic-only and change
# no behaviour.
DEBUG_TRACE=0
[[ "${CELLOS_DEBUG_TRACE:-0}" == 1 ]] && DEBUG_TRACE=1
DEBUG_LOOP=0
[[ "${CELLOS_DEBUG_LOOP:-0}" == 1 ]] && DEBUG_LOOP=1
if (( DEBUG_TRACE )); then
    EXTRA_FEATURES+=",service-hypervisor/l2-trace"
    echo '[rpi3-hv] CELLOS_DEBUG_TRACE=1: hypervisor l2-trace'
fi
if (( DEBUG_LOOP )); then
    echo '[rpi3-hv] CELLOS_DEBUG_LOOP=1: kernel ipc-trace + net/dwc2 loop-trace'
fi
cargo build --release --target "$TARGET" --no-default-features \
    "${BUILD_PACKAGES[@]}" \
    --features "$INIT_FEATURES,$EXTRA_FEATURES"
# Separate from the trace switches: the loopback self-test changes chip state at
# bring-up (PHY loopback, restored afterwards), so it is its own opt-in and never
# rides along with a plain diagnostic build.
if [[ "${CELLOS_DEBUG_LOOPBACK:-0}" == 1 ]]; then
    echo '[rpi3-hv] CELLOS_DEBUG_LOOPBACK=1: LAN9514 PHY loopback self-test at bring-up'
fi
DWC2_FEATURES=()
if (( DEBUG_LOOP )); then
    DWC2_FEATURES+=(driver-dwc2-usb/loop-trace)
fi
if [[ "${CELLOS_DEBUG_LOOPBACK:-0}" == 1 ]]; then
    DWC2_FEATURES+=(driver-dwc2-usb/loopback-diag)
fi
NET_FEATURES=()
if (( ${#DWC2_FEATURES[@]} )); then
    joined="$(IFS=,; echo "${DWC2_FEATURES[*]}")"
    if (( DEBUG_LOOP )); then
        NET_FEATURES+=(--features "service-net/loop-trace,$joined")
    else
        NET_FEATURES+=(--features "$joined")
    fi
fi
cargo build --release --target "$TARGET" -p service-net -p driver-dwc2-usb "${NET_FEATURES[@]}"

# The option cells build with their **default** features: the invocation above
# passes `--no-default-features`, and cells such as `driver-bcm-display` keep
# their runtime wiring (`ostd`, the SoC HAL) behind a default feature — building
# them there links a bare crate with no allocator and no panic handler.
OPTION_PACKAGES=()
if (( UI )); then
    OPTION_PACKAGES+=(-p service-compositor -p driver-bcm-display -p fb-console)
fi
if (( AI )); then
    OPTION_PACKAGES+=(-p service-config -p service-ai)
fi
if (( SUPERVISOR )); then
    OPTION_PACKAGES+=(-p supervisor)
fi
if (( ${#OPTION_PACKAGES[@]} > 0 )); then
    cargo build --release --target "$TARGET" "${OPTION_PACKAGES[@]}"
fi

# Fail closed on every boot-critical cell: omitting a path creates a successful
# but unbootable VIFS1 image, and /vmlinuz plus /initrd.gz must always be there.
# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
SIGN_TARGETS=("$BIN_DIR/app-init" "$BIN_DIR/app-shell" "$BIN_DIR/service-vfs" \
    "$BIN_DIR/service-input" "$BIN_DIR/service-net")
if (( !MINIMAL_DRIVERS )); then
    SIGN_TARGETS+=("$BIN_DIR/driver-dwc2-usb" "$BIN_DIR/driver-lan9514")
fi
if (( UI )); then
    SIGN_TARGETS+=("$BIN_DIR/service-compositor" "$BIN_DIR/driver-bcm-display" "$BIN_DIR/fb-console")
fi
if (( AI )); then
    SIGN_TARGETS+=("$BIN_DIR/service-config" "$BIN_DIR/service-ai")
fi
if (( SUPERVISOR )); then
    SIGN_TARGETS+=("$BIN_DIR/supervisor")
fi
if (( TIER3 )); then
    SIGN_TARGETS+=("$BIN_DIR/hypervisor")
fi
sign_cells "${SIGN_TARGETS[@]}"

mkdir -p "$EMBEDDED"
POLICY_TMP="$(mktemp -d)"
trap 'rm -rf "$POLICY_TMP"' EXIT
# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$POLICY_TMP/POLICY.BIN"
INITRD="$CACHE/initramfs-virt"
if [[ -n "${INITRD_OVERRIDE:-}" ]]; then
    [[ -s "$INITRD_OVERRIDE" ]] || { echo "ERROR: INITRD_OVERRIDE not readable: $INITRD_OVERRIDE" >&2; exit 2; }
    INITRD="$INITRD_OVERRIDE"
elif (( VOLATILE )); then
    # The stock initramfs carries busybox but no applet links, so the guest's
    # `rdinit=/bin/sh` shell cannot find `ls`, `cat` or `ps`. Link the applets
    # the volatile profile's shell is expected to have; see the tool's docstring.
    INITRD="$POLICY_TMP/initrd-pi-shell.gz"
    "$PYTHON_BIN" tools/prepare-rpi3-shell-initramfs.py "$CACHE/initramfs-virt" "$INITRD"
else
    INITRD="$POLICY_TMP/initrd-pi.gz"
    "$PYTHON_BIN" tools/prepare-rpi3-guest-initramfs.py \
        "$CACHE/initramfs-virt" "$CACHE/modloop-virt" \
        scripts/rpi3-guest-init "$INITRD"
fi
FAT_ARGS=("$BIN_DIR/app-init" bin/init \
    "$BIN_DIR/app-shell" bin/shell \
    "$BIN_DIR/service-vfs" bin/vfs \
    "$BIN_DIR/service-input" bin/input \
    "$BIN_DIR/service-net" bin/net)
if (( !MINIMAL_DRIVERS )); then
    FAT_ARGS+=("$BIN_DIR/driver-dwc2-usb" bin/dwc2-usb \
        "$BIN_DIR/driver-lan9514" bin/lan9514)
fi
if (( UI )); then
    FAT_ARGS+=("$BIN_DIR/service-compositor" bin/compositor \
        "$BIN_DIR/driver-bcm-display" bin/bcm-display \
        "$BIN_DIR/fb-console" bin/fb-console)
fi
if (( AI )); then
    FAT_ARGS+=("$BIN_DIR/service-config" bin/config \
        "$BIN_DIR/service-ai" bin/ai)
fi
if (( SUPERVISOR )); then
    FAT_ARGS+=("$BIN_DIR/supervisor" bin/supervisor)
fi
if (( TIER3 )); then
    FAT_ARGS+=("$BIN_DIR/hypervisor" bin/hypervisor \
        "$CACHE/Image-rpi3-hv" vmlinuz \
        "$INITRD" initrd.gz)
fi
FAT_ARGS+=("$POLICY_TMP/POLICY.BIN" POLICY.BIN)
CONFIG_FEATURES="$INIT_FEATURES"
if (( SUPERVISOR )); then CONFIG_FEATURES+=,app-init/supervisor; fi
"$PYTHON_BIN" tools/mkfat32.py \
    --config-features "$CONFIG_FEATURES" \
    --config-output-dir "$EMBEDDED/boot-config" \
    "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$POLICY_TMP/layout.txt"
assert_policy_in_image "$POLICY_TMP/layout.txt"
if ! grep -qiF -- '--- /bin ---' "$POLICY_TMP/layout.txt"; then
    echo 'ERROR: required /bin directory absent from VIFS1' >&2
    exit 1
fi
# The base entries every image must carry, plus the guest ones only when Tier 3
# is packaged. A `--no-tier3` image that still demanded vmlinuz would fail here
# after successfully building exactly the image that was asked for.
REQUIRED_ENTRIES=(init vfs input net)
if (( !MINIMAL_DRIVERS )); then
    REQUIRED_ENTRIES+=(dwc2-usb lan9514)
fi
if (( UI )); then
    REQUIRED_ENTRIES+=(compositor bcm-display fb-console)
fi
if (( AI )); then
    REQUIRED_ENTRIES+=(config ai)
fi
if (( SUPERVISOR )); then
    REQUIRED_ENTRIES+=(supervisor)
fi
if (( TIER3 )); then
    REQUIRED_ENTRIES+=(vmlinuz initrd.gz hypervisor)
fi
for path in "${REQUIRED_ENTRIES[@]}"; do
    if ! grep -qiF "LFN '$path'" "$POLICY_TMP/layout.txt"; then
        echo "ERROR: required guest image entry absent from VIFS1: $path" >&2
        exit 1
    fi
done
cp "$BIN_DIR/app-init" "$EMBEDDED/init"

KERNEL_FEATURES=board-rpi3
if (( MINIMAL_DRIVERS )); then
    KERNEL_FEATURES=board-rpi3-bring-up
fi
if (( DEBUG_LOOP )); then
    KERNEL_FEATURES+=",ipc-trace"
fi
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release --target "$TARGET" \
    -p cellos-kernel --features "$KERNEL_FEATURES"
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
    for program in mformat mcopy mmd mkfs.ext4; do
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
    GUEST_DISK="$POLICY_TMP/guest_disk.img"
    truncate -s 256M "$BOOT_IMG"
    truncate -s 64M "$GUEST_DISK"
    mkfs.ext4 -q -F "$GUEST_DISK"
    mformat -i "$BOOT_IMG" -F -v CELLOS-BOOT ::
    for file in bootcode.bin start.elf fixup.dat config.txt; do
        mcopy -i "$BOOT_IMG" "$FIRMWARE/$file" "::$file"
    done
    if [[ -s "$FIRMWARE/bcm2710-rpi-3-b.dtb" ]]; then
        mcopy -i "$BOOT_IMG" "$FIRMWARE/bcm2710-rpi-3-b.dtb" ::
    fi
    mcopy -i "$BOOT_IMG" "$RAW_KERNEL" ::kernel8.img
    mcopy -i "$BOOT_IMG" "$GUEST_DISK" ::guest_disk.img
    "$PYTHON_BIN" scripts/generate-boot-config.py --preserve-persistent "$SD_IMAGE" \
        --output-dir "$EMBEDDED/boot-config"
    mmd -i "$BOOT_IMG" ::/etc ::/etc/cellos
    for name in system services autoload; do
        mcopy -i "$BOOT_IMG" "$EMBEDDED/boot-config/$name.toml" "::/etc/cellos/$name.toml"
    done
    # raspi3b models an SD card only when its byte length is a power of two.
    # P2/P3/P4 follow the kernel's non-overlapping canonical partition map.
    truncate -s 1G "$SD_IMAGE"
    "$PYTHON_BIN" tools/write-rpi3-mbr.py "$SD_IMAGE" --tier3
    dd if="$BOOT_IMG" of="$SD_IMAGE" bs=512 seek=2048 conv=notrunc status=none
    # Boot-critical Cells are in signed VIFS1. P2 contains a valid empty
    # bootstrap table; P3/P4 are left for their native filesystem owners.
    "$PYTHON_BIN" - "$SD_IMAGE" <<'PY'
import struct
import sys
with open(sys.argv[1], "r+b") as disk:
    disk.seek(526336 * 512)
    disk.write(struct.pack("<QI", 0x56494F535F43454C, 0).ljust(512, b"\0"))
PY
    echo "[rpi3-hv] physical SD image: $SD_IMAGE (/mnt/sd/guest_disk.img on P1)"
else
    echo '[rpi3-hv] volatile disk requested; no SD image created (guest disk is nonpersistent)'
fi

echo "[rpi3-hv] Pi guest VIFS1: $EMBEDDED/kernel_fs.img"
echo "[rpi3-hv] QEMU raspi3b -kernel raw Pi image: $RAW_KERNEL (EL2 entry)"
echo '[rpi3-hv] QEMU mini UART is serial1; use -serial null -serial stdio'
