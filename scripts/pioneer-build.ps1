#!/usr/bin/env pwsh
# Build Cellos for Milk-V Pioneer (SG2042) on Windows.
# Produces target/.../vicell-kernel and delegates image creation to WSL2.
#
# Usage:
#   .\scripts\pioneer-build.ps1             # build kernel + create pioneer-boot.img via WSL2
#   .\scripts\pioneer-build.ps1 -NoBuild    # skip build (reuse last kernel), just recreate image
#
# Writing to a USB/NVMe device still requires a Linux host (or WSL2 with losetup available):
#   sudo dd if=pioneer-boot.img of=/dev/sdX bs=4M status=progress conv=fsync && sync
param(
    [switch]$NoBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $NoBuild) {
    Write-Host "[pioneer-build] Building Cellos kernel for Pioneer SG2042..."
    $env:RUSTFLAGS = "-C relocation-model=pic"
    cargo build --release -p vicell-kernel `
        --target riscv64gc-unknown-none-elf `
        --features board-pioneer
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

Write-Host "[pioneer-build] Delegating image creation to WSL2..."
wsl bash scripts/pioneer-flash.sh
if ($LASTEXITCODE -ne 0) { throw "pioneer-flash.sh failed in WSL2" }

Write-Host ""
Write-Host "[pioneer-build] Image: pioneer-boot.img"
Write-Host "            To flash (Linux/WSL2 with USB/NVMe connected):"
Write-Host "              sudo dd if=pioneer-boot.img of=/dev/sdX bs=4M status=progress conv=fsync && sync"
