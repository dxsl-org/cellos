#!/usr/bin/env pwsh
# Build ViCell for VisionFive2 on Windows.
# Produces target/.../vicell-kernel and delegates image creation to WSL2.
#
# Usage:
#   .\scripts\vf2-build.ps1             # build kernel + create vf2-boot.img via WSL2
#   .\scripts\vf2-build.ps1 -NoBuild    # skip build (reuse last kernel), just recreate image
#
# Flashing still requires a Linux host (or WSL2 with losetup available):
#   sudo dd if=vf2-boot.img of=/dev/sdX bs=4M status=progress conv=fsync && sync
param(
    [switch]$NoBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

if (-not $NoBuild) {
    Write-Host "[vf2-build] Building ViCell kernel for VisionFive2..."
    $env:RUSTFLAGS = "-C relocation-model=pic"
    cargo build --release -p vicell-kernel `
        --target riscv64gc-unknown-none-elf `
        --features board-vf2
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

Write-Host "[vf2-build] Delegating image creation to WSL2..."
wsl bash scripts/vf2-flash.sh
if ($LASTEXITCODE -ne 0) { throw "vf2-flash.sh failed in WSL2" }

Write-Host ""
Write-Host "[vf2-build] Image: vf2-boot.img"
Write-Host "            To flash (Linux/WSL2 with SD card):"
Write-Host "              sudo dd if=vf2-boot.img of=/dev/sdX bs=4M status=progress conv=fsync && sync"
