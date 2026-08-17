# gen_disk_rpi3.ps1 — Build bootable Cellos SD card image for Raspberry Pi 3 (BCM2837).
#
# Requirements:
#   - WSL2 installed (Ubuntu or Debian) with parted, dosfstools, util-linux, mount
#   - cargo build --release --features board-rpi3 already run
#   - aarch64-linux-gnu-objcopy available inside WSL
#   - tools/rpi3-firmware/ containing bootcode.bin, start.elf, fixup.dat, config.txt
#     (download: see tools/rpi3-firmware/README.txt)
#
# Partition layout (512 MiB total):
#   P1: FAT32 256 MiB  — boot (VideoCore firmware + kernel8.img)
#   P2: FAT32 ~256 MiB — Cellos cell binaries (init, vfs, net, shell, ...)
#
# Usage: .\gen_disk_rpi3.ps1 [-Output disk_rpi3.img] [-FirmwareDirectory tools\rpi3-firmware]

param(
    [string]$Output = "disk_rpi3.img",
    [string]$FirmwareDirectory = "tools\rpi3-firmware"
)

$ErrorActionPreference = "Stop"

function Convert-ToWslPath([string]$Path) {
    $converted = & wsl.exe --exec wslpath -u $Path
    if ($LASTEXITCODE -ne 0) {
        throw "Could not convert path for WSL: $Path"
    }
    return $converted.Trim()
}

$target      = "aarch64-unknown-none-softfloat"
$kernel_path = "target\$target\release\cellos-kernel"
$firmware_dir = $FirmwareDirectory
$img_size_mb  = 512

# Validate prerequisites
if (-not (Test-Path $kernel_path)) {
    Write-Error "Kernel not found: $kernel_path`nRun: cargo build --release --features board-rpi3 -p cellos-kernel --target $target"
}
foreach ($fw in @("bootcode.bin", "start.elf", "fixup.dat", "config.txt")) {
    if (-not (Test-Path "$firmware_dir\$fw")) {
        Write-Error "Missing firmware file: $firmware_dir\$fw`nSee tools/rpi3-firmware/README.txt for download instructions."
    }
}

# Convert Windows paths to WSL paths
$working_directory = (Get-Location).ProviderPath
$output_path = if ([IO.Path]::IsPathRooted($Output)) {
    [IO.Path]::GetFullPath($Output)
} else {
    [IO.Path]::GetFullPath((Join-Path $working_directory $Output))
}
if ([IO.Path]::GetExtension($output_path) -ne ".img") {
    throw "Output must be an .img file: $output_path"
}
if (Test-Path -LiteralPath $output_path -PathType Container) {
    throw "Output points to a directory: $output_path"
}
$output_parent = Split-Path -Parent $output_path
if (-not (Test-Path -LiteralPath $output_parent -PathType Container)) {
    throw "Output directory does not exist: $output_parent"
}

$pwd_wsl     = Convert-ToWslPath $working_directory
$kernel_wsl  = Convert-ToWslPath (Resolve-Path $kernel_path).ProviderPath
$fw_wsl      = Convert-ToWslPath (Resolve-Path $firmware_dir).ProviderPath
$output_wsl  = Convert-ToWslPath $output_path
$wsl_uid     = (& wsl.exe --exec id -u).Trim()
$wsl_gid     = (& wsl.exe --exec id -g).Trim()

Write-Host "[rpi3] Building SD image: $Output ($img_size_mb MiB)"
Write-Host "[rpi3] Kernel: $kernel_path"
Write-Host "[rpi3] Firmware: $firmware_dir"

$bash_script = @'
set -euo pipefail

ROOT="$1"
IMG="$2"
KERNEL="$3"
FIRMWARE="$4"
IMG_SIZE_MB="$5"
OWNER_UID="$6"
OWNER_GID="$7"
BOOT_SIZE_MB=256

cd "$ROOT"

for tool in aarch64-linux-gnu-objcopy parted mkfs.fat losetup mount umount dd; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "[rpi3] ERROR: $tool is required inside WSL" >&2
        exit 1
    }
done

# VideoCore loads a flat AArch64 image, not an ELF executable.
RAW_KERNEL=$(mktemp)
LOOP=
BOOT=
DATA=
cleanup() {
    if [ -n "$BOOT" ]; then
        umount "$BOOT" 2>/dev/null || true
        rmdir "$BOOT" 2>/dev/null || true
    fi
    if [ -n "$DATA" ]; then
        umount "$DATA" 2>/dev/null || true
        rmdir "$DATA" 2>/dev/null || true
    fi
    if [ -n "$LOOP" ]; then
        losetup -d "$LOOP" 2>/dev/null || true
    fi
    rm -f "$RAW_KERNEL"
}
trap cleanup EXIT

aarch64-linux-gnu-objcopy -O binary "$KERNEL" "$RAW_KERNEL"
RAW_MAGIC=$(od -An -tx1 -N4 "$RAW_KERNEL" | tr -d ' \n')
if [ "$RAW_MAGIC" = "7f454c46" ]; then
    echo "[rpi3] ERROR: generated kernel8.img still has ELF magic" >&2
    exit 1
fi
RAW_SIZE=$(stat -c '%s' "$RAW_KERNEL")
if [ "$RAW_SIZE" -eq 0 ]; then
    echo "[rpi3] ERROR: generated kernel8.img is empty" >&2
    exit 1
fi
RAW_SHA256=$(sha256sum "$RAW_KERNEL" | awk '{print $1}')
echo "[rpi3] Raw kernel: $RAW_SIZE bytes SHA-256=$RAW_SHA256"

# Create blank image
rm -f "$IMG"
dd if=/dev/zero of="$IMG" bs=1M count="$IMG_SIZE_MB" status=none

# Partition table: MBR, FAT32 boot (type 0x0C required by VideoCore) + FAT32 data
parted -s "$IMG" mklabel msdos
parted -s "$IMG" mkpart primary fat32 1MiB ${BOOT_SIZE_MB}MiB
parted -s "$IMG" mkpart primary fat32 ${BOOT_SIZE_MB}MiB 100%
parted -s "$IMG" set 1 boot on

# Attach loop device with partition scan
LOOP=$(losetup --find --show --partscan "$IMG")
BOOT=$(mktemp -d)
DATA=$(mktemp -d)

# Format (VideoCore requires FAT32 with type 0x0C — set by parted above)
mkfs.fat -F32 -n CELLOS-BOOT "${LOOP}p1" >/dev/null
mkfs.fat -F32 -n CELLOS-CELL "${LOOP}p2" >/dev/null

mount "${LOOP}p1" "$BOOT"
mount "${LOOP}p2" "$DATA"

# Boot partition: VideoCore firmware + Cellos kernel (must be named kernel8.img for ARM64)
cp "$FIRMWARE/bootcode.bin" "$BOOT/"
cp "$FIRMWARE/start.elf"   "$BOOT/"
cp "$FIRMWARE/fixup.dat"   "$BOOT/"
cp "$FIRMWARE/config.txt"  "$BOOT/"
cp "$RAW_KERNEL"           "$BOOT/kernel8.img"
echo "[rpi3]   boot: bootcode.bin start.elf fixup.dat config.txt kernel8.img"

# Cell partition: copy cell binaries built for aarch64
CELL_DIR="target/aarch64-unknown-none-softfloat/release"
for cell in app-init service-vfs service-net app-shell service-compositor service-input driver-gpio-bcm service-power service-config supervisor; do
    if [ -f "$CELL_DIR/$cell" ]; then
        cp "$CELL_DIR/$cell" "$DATA/"
        echo "[rpi3]   cell: $cell"
    fi
done

echo "[rpi3] Done: $IMG ($IMG_SIZE_MB MiB)"
chown "$OWNER_UID:$OWNER_GID" "$IMG"
'@

wsl.exe -u root --exec bash -c $bash_script bash $pwd_wsl $output_wsl $kernel_wsl $fw_wsl $img_size_mb $wsl_uid $wsl_gid
if ($LASTEXITCODE -ne 0) {
    throw "WSL image generation failed with exit code $LASTEXITCODE"
}

Write-Host ""
Write-Host "[rpi3] Image ready: $Output"
Write-Host "[rpi3] QEMU test:  .\run-rpi3.ps1 -SdImage"
Write-Host "[rpi3] Flash (Linux/WSL2): sudo dd if=$Output of=/dev/sdX bs=4M status=progress conv=fsync"
Write-Host "[rpi3] Flash (Windows):    Use Raspberry Pi Imager → 'Use custom' → $Output"
