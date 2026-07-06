# build-zig-cells.ps1 — Build all Zig cells for Cellos.
#
# Discovers Zig cells in cells/tests/ and builds each one. Outputs
# "cell:<name>=<elf-path>" lines on stdout for gen_disk.ps1 to consume.
#
# Usage (called from gen_disk.ps1):
#   $zig_output = & pwsh scripts/build-zig-cells.ps1
#
# Direct use:
#   pwsh scripts/build-zig-cells.ps1 [-Arch riscv64|aarch64] [-Optimize ReleaseSmall]

param(
    [string]$Arch     = "riscv64",
    [string]$Optimize = "ReleaseSmall"
)

$RepoRoot = Split-Path -Parent $PSScriptRoot

# Guard: skip if zig is not installed.
if (-not (Get-Command zig -ErrorAction SilentlyContinue)) {
    Write-Host "Skipping Zig cells (zig not in PATH — install Zig 0.13+ to enable Tier 1b Zig)."
    exit 0
}

$zigVersion = (zig version 2>&1)
Write-Host "Zig: $zigVersion"

# Canonical list of Zig cells (add new cells here).
$cells = @(
    "cells\tests\zig-hello",
    "cells\tests\zig-mlibc-smoke"
)

$target = "${Arch}-freestanding-none"

foreach ($cell in $cells) {
    $cellPath = Join-Path $RepoRoot $cell
    if (-not (Test-Path "$cellPath\build.zig")) { continue }

    $name = Split-Path $cellPath -Leaf
    Write-Host "Building Zig cell: $name (target=$target, optimize=$Optimize)..."

    Push-Location $cellPath
    zig build "-Dtarget=$target" "-Doptimize=$Optimize" 2>&1 | Select-Object -Last 5
    $buildOk = ($LASTEXITCODE -eq 0)
    Pop-Location

    if ($buildOk) {
        $elf = "$cellPath\zig-out\bin\$name"
        if (Test-Path $elf) {
            Write-Output "cell:$name=$elf"
        } else {
            Write-Warning "Zig cell ${name}: build succeeded but ELF not found at $elf"
        }
    } else {
        Write-Warning "Zig cell ${name}: build failed (see output above)."
    }
}
