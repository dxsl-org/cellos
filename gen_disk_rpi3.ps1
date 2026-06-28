# gen_disk_rpi3.ps1 — Build bootable Cellos SD card image for Raspberry Pi 3 (BCM2837).
#
# Requirements:
#   - WSL2 installed (Ubuntu or Debian) with parted, mkfs.fat, mount
#   - cargo build --release --features board-rpi3 already run
#   - tools/rpi3-firmware/ containing bootcode.bin, start.elf, fixup.dat, config.txt
#     (download: see tools/rpi3-firmware/README.txt)
#
# Partition layout (512 MiB total):
#   P1: FAT32 256 MiB  — boot (VideoCore firmware + kernel8.img)
#   P2: FAT32 ~256 MiB — Cellos cell binaries (init, vfs, net, shell, ...)
#
# Usage: .\gen_disk_rpi3.ps1 [-Output disk_rpi3.img]

param(
    [string]$Output = "disk_rpi3.img"
)

$ErrorActionPreference = "Stop"

$target      = "aarch64-unknown-none-softfloat"
$kernel_path = "target\$target\release\vicell-kernel"
$firmware_dir = "tools\rpi3-firmware"
$img_size_mb  = 512

# Validate prerequisites
if (-not (Test-Path $kernel_path)) {
    Write-Error "Kernel not found: $kernel_path`nRun: cargo build --release --features board-rpi3 -p vicell-kernel --target $target"
}
foreach ($fw in @("bootcode.bin", "start.elf", "fixup.dat", "config.txt")) {
    if (-not (Test-Path "$firmware_dir\$fw")) {
        Write-Error "Missing firmware file: $firmware_dir\$fw`nSee tools/rpi3-firmware/README.txt for download instructions."
    }
}

# Convert Windows paths to WSL paths
$pwd_wsl     = (wsl wslpath -u ((Get-Location).Path))
$kernel_wsl  = (wsl wslpath -u ((Resolve-Path $kernel_path).Path))
$fw_wsl      = (wsl wslpath -u ((Resolve-Path $firmware_dir).Path))
$output_wsl  = "$pwd_wsl/$Output"

Write-Host "[rpi3] Building SD image: $Output ($img_size_mb MiB)"
Write-Host "[rpi3] Kernel: $kernel_path"
Write-Host "[rpi3] Firmware: $firmware_dir"

wsl bash -c @"
set -euo pipefail

IMG="$output_wsl"
KERNEL="$kernel_wsl"
FIRMWARE="$fw_wsl"
IMG_SIZE_MB=$img_size_mb
BOOT_SIZE_MB=256

# Create blank image
rm -f "\$IMG"
dd if=/dev/zero of="\$IMG" bs=1M count=\$IMG_SIZE_MB status=none

# Partition table: MBR, FAT32 boot (type 0x0C required by VideoCore) + FAT32 data
parted -s "\$IMG" mklabel msdos
parted -s "\$IMG" mkpart primary fat32 1MiB \${BOOT_SIZE_MB}MiB
parted -s "\$IMG" mkpart primary fat32 \${BOOT_SIZE_MB}MiB 100%
parted -s "\$IMG" set 1 boot on

# Attach loop device with partition scan
LOOP=\$(losetup --find --show --partscan "\$IMG")
cleanup() {
    umount "\$BOOT" "\$DATA" 2>/dev/null || true
    rmdir  "\$BOOT" "\$DATA" 2>/dev/null || true
    losetup -d "\$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

# Format (VideoCore requires FAT32 with type 0x0C — set by parted above)
mkfs.fat -F32 -n CELLOS-BOOT "\${LOOP}p1" >/dev/null
mkfs.fat -F32 -n CELLOS-CELL "\${LOOP}p2" >/dev/null

BOOT=\$(mktemp -d)
DATA=\$(mktemp -d)
mount "\${LOOP}p1" "\$BOOT"
mount "\${LOOP}p2" "\$DATA"

# Boot partition: VideoCore firmware + Cellos kernel (must be named kernel8.img for ARM64)
cp "\$FIRMWARE/bootcode.bin" "\$BOOT/"
cp "\$FIRMWARE/start.elf"   "\$BOOT/"
cp "\$FIRMWARE/fixup.dat"   "\$BOOT/"
cp "\$FIRMWARE/config.txt"  "\$BOOT/"
cp "\$KERNEL"               "\$BOOT/kernel8.img"
echo "[rpi3]   boot: bootcode.bin start.elf fixup.dat config.txt kernel8.img"

# Cell partition: copy cell binaries built for aarch64
CELL_DIR="target/aarch64-unknown-none-softfloat/release"
for cell in app-init service-vfs service-net app-shell service-compositor service-input driver-gpio-bcm service-power service-config supervisor; do
    if [ -f "\$CELL_DIR/\$cell" ]; then
        cp "\$CELL_DIR/\$cell" "\$DATA/"
        echo "[rpi3]   cell: \$cell"
    fi
done

echo "[rpi3] Done: \$IMG (\$IMG_SIZE_MB MiB)"
"@

Write-Host ""
Write-Host "[rpi3] Image ready: $Output"
Write-Host "[rpi3] QEMU test:  .\run-rpi3.ps1 -SdImage"
Write-Host "[rpi3] Flash (Linux/WSL2): sudo dd if=$Output of=/dev/sdX bs=4M status=progress conv=fsync"
Write-Host "[rpi3] Flash (Windows):    Use Raspberry Pi Imager → 'Use custom' → $Output"
