#!/usr/bin/env bash
# Create the partitioned ARM hypervisor disk. P1 starts at the canonical LBA
# 2048 and carries guest_disk.img, so the VFS block-driver route mounts it at
# /mnt/sd. Alpine boot assets and bootstrap cells stay in the kernel's VIFS1
# image assembled by make-hypervisor-fs.sh.
#
# Usage: bash scripts/format-disk-hv-arm.sh [--gui] [output.img]

set -euo pipefail

GUI=0
OUT="disk_hv_arm.img"
OUT_EXPLICIT=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --gui)
            GUI=1
            shift
            ;;
        -h|--help)
            echo "Usage: bash scripts/format-disk-hv-arm.sh [--gui] [output.img]"
            exit 0
            ;;
        *)
            if [[ "$OUT" != "disk_hv_arm.img" ]]; then
                echo "ERROR: unexpected argument: $1" >&2
                echo "Usage: bash scripts/format-disk-hv-arm.sh [--gui] [output.img]" >&2
                exit 1
            fi
            OUT="$1"
            OUT_EXPLICIT=1
            shift
            ;;
    esac
done

if [[ "$GUI" -eq 1 && "$OUT_EXPLICIT" -eq 0 ]]; then
    OUT="disk_hv_arm_gui.img"
fi

TARGET="aarch64-unknown-none-softfloat"
BIN_DIR="target/$TARGET/release"

if [[ "$GUI" -eq 1 ]]; then
    echo "[format-disk-hv] Collecting aarch64 GUI cell binaries from $BIN_DIR..."
else
    echo "[format-disk-hv] Collecting aarch64 cell binaries from $BIN_DIR..."
fi

declare -A CELLS=(
    [app-init]=init
    [app-shell]=shell
    [service-vfs]=vfs
    [service-config]=config
    [service-net]=net
    [service-input]=input
    [service-compositor]=compositor
    [supervisor]=supervisor
    # The service-hypervisor package names its [[bin]] "hypervisor" — key on the
    # artifact name, not the package name, or the cell is silently skipped.
    [hypervisor]=hypervisor
)

if [[ "$GUI" -eq 1 ]]; then
    CELLS[driver-virtio-gpu]=virtio-gpu
fi

MKFAT_ARGS=()
for src_name in "${!CELLS[@]}"; do
    dst_name="${CELLS[$src_name]}"
    src="$BIN_DIR/$src_name"
    if [[ -f "$src" ]]; then
        echo "  /bin/$dst_name <- $src"
        MKFAT_ARGS+=("$src" "bin/$dst_name")
    else
        echo "  WARNING: $src not found, skipping /bin/$dst_name"
    fi
done

# The entire disk exists to carry the hypervisor cell — a silent skip here
# produces a "successful" build whose smoke test can never pass.
if [[ ! -f "$BIN_DIR/hypervisor" ]]; then
    echo "ERROR: $BIN_DIR/hypervisor not built — /bin/hypervisor would be missing" >&2
    exit 1
fi

if [[ "$GUI" -eq 1 && ! -f "$BIN_DIR/driver-virtio-gpu" ]]; then
    echo "ERROR: $BIN_DIR/driver-virtio-gpu not built — /bin/virtio-gpu would be missing" >&2
    exit 1
fi

TMPDIR_WORK=$(mktemp -d)
trap 'rm -rf "$TMPDIR_WORK" "${HOSTNAME_TMP:-}" "${GUEST_DISK_TMP:-}"' EXIT
HOSTNAME_TMP=$(mktemp)
if [[ "$GUI" -eq 1 ]]; then
    echo "ViCell-HV-GUI" > "$HOSTNAME_TMP"
else
    echo "ViCell-HV" > "$HOSTNAME_TMP"
fi
MKFAT_ARGS+=("$HOSTNAME_TMP" "etc/hostname")
GUEST_DISK_TMP=$(mktemp)
dd if=/dev/zero of="$GUEST_DISK_TMP" bs=1M count=8 status=none
MKFAT_ARGS+=("$GUEST_DISK_TMP" "guest_disk.img")


echo "[format-disk-hv] Creating partitioned $OUT..."
PYTHON_BIN="${PYTHON_BIN:-}"
if [[ -z "$PYTHON_BIN" ]]; then
    if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
        PYTHON_BIN=python3
    else
        PYTHON_BIN=python
    fi
fi
P1_IMG="$TMPDIR_WORK/p1.img"
"$PYTHON_BIN" tools/mkfat32.py "$P1_IMG" "${MKFAT_ARGS[@]}"
PART_FAT32_BASE_LBA=2048
PART_SRV_BASE_LBA=931072
PART_SRV_SECTORS=131072
FULL_SECTORS=$((PART_SRV_BASE_LBA + PART_SRV_SECTORS))
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
"$PYTHON_BIN" tools/write-mbr.py "$OUT" >/dev/null
dd if="$P1_IMG" of="$OUT" bs=512 seek="$PART_FAT32_BASE_LBA" conv=notrunc status=none
echo "[format-disk-hv] Done: $OUT (MBR + P1 FAT guest disk)"
