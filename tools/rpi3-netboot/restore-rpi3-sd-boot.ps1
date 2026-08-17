[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z]$')][string]$SdDriveLetter,
    [Parameter(Mandatory)][string]$BackupDirectory,
    [int]$ExpectedDiskNumber = -1
)

$ErrorActionPreference = 'Stop'
$sdRoot = "${SdDriveLetter}:\"
$partition = Get-Partition -DriveLetter $SdDriveLetter
$disk = $partition | Get-Disk
if ($ExpectedDiskNumber -ge 0 -and $disk.Number -ne $ExpectedDiskNumber) {
    throw "Refusing disk $($disk.Number); expected $ExpectedDiskNumber"
}
$source = Join-Path $BackupDirectory 'sd-root'
if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "Backup sd-root missing: $source"
}
$manifestPath = Join-Path $BackupDirectory 'manifest.json'
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
foreach ($entry in $manifest.backupFiles) {
    $file = Join-Path $source $entry.path
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash -ne $entry.sha256) {
        throw "Backup hash mismatch: $($entry.path)"
    }
}
if ($WhatIfPreference) {
    Write-Host "Would restore $source to $sdRoot"
    return
}
if ($PSCmdlet.ShouldProcess($sdRoot, "Restore local boot from $source")) {
    foreach ($name in @(
        'u-boot.bin', 'boot.scr', 'bcm2837-rpi-3-b.dtb',
        'bcm2710-rpi-3-b.dtb', 'overlays\disable-bt.dtbo'
    )) {
        $stale = Join-Path $sdRoot $name
        if (Test-Path -LiteralPath $stale) { Remove-Item -LiteralPath $stale -Force }
    }
    foreach ($item in Get-ChildItem -LiteralPath $source -Force) {
        Copy-Item -LiteralPath $item.FullName -Destination $sdRoot -Recurse -Force
    }
}
Write-Host "Restored local SD boot files to $sdRoot"
