#!/usr/bin/env pwsh
# Bounded QEMU/OVMF smoke for the repo-relative Cellos x86_64 ISO.

param(
    [string]$Iso,
    [string]$Ovmf,
    [int]$TimeoutSeconds = 40
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Iso) { $Iso = Join-Path $repoRoot 'build\vicell-x86.iso' }
if (-not [IO.Path]::IsPathRooted($Iso)) { $Iso = Join-Path $repoRoot $Iso }

if (-not $Ovmf -and $env:CELLOS_OVMF_CODE) { $Ovmf = $env:CELLOS_OVMF_CODE }
if (-not $Ovmf) {
    $Ovmf = @(
        (Join-Path $repoRoot 'build\ovmf-x86.fd'),
        'C:\Program Files\qemu\share\edk2-x86_64-code.fd',
        'C:\Program Files\qemu\share\edk2-x86_64-secure-code.fd'
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

$qemu = if ($env:CELLOS_QEMU_X86) {
    $env:CELLOS_QEMU_X86
} elseif (Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue) {
    (Get-Command qemu-system-x86_64).Source
} elseif (Test-Path -LiteralPath 'C:\Program Files\qemu\qemu-system-x86_64.exe') {
    'C:\Program Files\qemu\qemu-system-x86_64.exe'
}

if (-not $qemu) { throw 'qemu-system-x86_64 not found; set CELLOS_QEMU_X86.' }
if (-not (Test-Path -LiteralPath $Iso)) { throw "Cellos ISO not found: $Iso" }
if (-not $Ovmf -or -not (Test-Path -LiteralPath $Ovmf)) {
    throw 'OVMF code image not found; pass -Ovmf or set CELLOS_OVMF_CODE.'
}

$serial = Join-Path $repoRoot 'build\serial-x86-uefi.log'
if (Test-Path -LiteralPath $serial) { Clear-Content -LiteralPath $serial }

Write-Host "Booting Cellos x86_64 UEFI smoke; serial log: $serial"
$process = Start-Process -FilePath $qemu -ArgumentList @(
    '-machine', 'q35',
    '-cpu', 'qemu64,+pdpe1gb',
    '-m', '256M',
    '-drive', "if=pflash,format=raw,readonly=on,file=$Ovmf",
    '-cdrom', $Iso,
    '-boot', 'd',
    '-serial', "file:$serial",
    '-display', 'none',
    '-no-reboot', '-no-shutdown'
) -PassThru -WindowStyle Hidden

$success = $false
try {
    for ($elapsed = 0; $elapsed -lt $TimeoutSeconds; $elapsed++) {
        Start-Sleep -Seconds 1
        if (-not (Test-Path -LiteralPath $serial)) { continue }
        $content = Get-Content -LiteralPath $serial -Raw -ErrorAction SilentlyContinue
        if ($content -match 'Scheduler initialized') { $success = $true; break }
        if ($content -match 'PANIC|triple fault') { break }
    }
} finally {
    if (-not $process.HasExited) { $process.Kill() }
}

if (Test-Path -LiteralPath $serial) { Get-Content -LiteralPath $serial }
if (-not $success) { throw "UEFI smoke did not reach Scheduler initialized within ${TimeoutSeconds}s." }
Write-Host 'UEFI_QEMU_READY'
