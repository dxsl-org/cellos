[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z]$')][string]$SdDriveLetter,
    [Parameter(Mandatory)][string]$UbootImage,
    [Parameter(Mandatory)][string]$BootScript,
    [Parameter(Mandatory)][string]$DeviceTree,
    [int]$ExpectedDiskNumber = -1,
    [string]$Backups = (Join-Path $PSScriptRoot 'backups')
)

$ErrorActionPreference = 'Stop'
$sdRoot = "${SdDriveLetter}:\"
$partition = Get-Partition -DriveLetter $SdDriveLetter
$disk = $partition | Get-Disk
if ($ExpectedDiskNumber -ge 0 -and $disk.Number -ne $ExpectedDiskNumber) {
    throw "Refusing disk $($disk.Number); expected $ExpectedDiskNumber"
}
if ($disk.Size -gt 64GB) {
    throw "Refusing unexpectedly large target disk: $([math]::Round($disk.Size / 1GB, 2)) GiB"
}

$uboot = Get-Item -LiteralPath $UbootImage
$script = Get-Item -LiteralPath $BootScript
$dtb = Get-Item -LiteralPath $DeviceTree
if ($uboot.Length -lt 256KB -or $uboot.Length -gt 4MB) {
    throw 'U-Boot image size is outside the expected range'
}
if ($script.Length -lt 64 -or $script.Length -gt 64KB) {
    throw 'U-Boot script size is outside the expected range'
}
$scriptHeader = [IO.File]::ReadAllBytes($script.FullName)[0..63]
if ([BitConverter]::ToString($scriptHeader[0..3]) -ne '27-05-19-56' -or
    $scriptHeader[30] -ne 6) {
    throw 'BootScript must be a valid legacy U-Boot script image'
}
if ($dtb.Length -lt 4KB -or $dtb.Length -gt 1MB -or
    [BitConverter]::ToString([IO.File]::ReadAllBytes($dtb.FullName)[0..3]) -ne 'D0-0D-FE-ED') {
    throw 'DeviceTree must be a valid flattened device tree'
}

$sourceRoots = @($sdRoot)
$onCardBackup = Get-ChildItem -LiteralPath $sdRoot -Directory -Filter 'local-boot-backup-*' |
    Sort-Object Name -Descending | Select-Object -First 1
if ($onCardBackup) { $sourceRoots += $onCardBackup.FullName }

function Resolve-BootFile([string]$Name) {
    foreach ($sourceRoot in $sourceRoots) {
        $candidate = Join-Path $sourceRoot $Name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    throw "Boot file missing from SD and rollback directory: $Name"
}

$firmware = @{}
foreach ($name in @('bootcode.bin', 'start.elf', 'fixup.dat', 'config.txt')) {
    $firmware[$name] = Resolve-BootFile $name
}
$configLines = @(Get-Content -LiteralPath $firmware['config.txt'])
$configLines = @($configLines | Where-Object {
    $_ -notmatch '^\s*(arm_64bit|kernel|device_tree|enable_uart|uart_2ndstage|core_freq)\s*=' -and
    $_ -notmatch '^\s*dtoverlay\s*=\s*disable-bt\s*$' -and
    $_ -ne '# Static Cellos TFTP bootstrap via U-Boot'
})
$configLines += @(
    '',
    '# Static Cellos TFTP bootstrap via U-Boot',
    'arm_64bit=1',
    'kernel=u-boot.bin',
    'device_tree=bcm2710-rpi-3-b.dtb',
    'enable_uart=1',
    'core_freq=250'
)

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backupDirectory = Join-Path $Backups $stamp
$backupRoot = Join-Path $backupDirectory 'sd-root'
if ($WhatIfPreference) {
    Write-Host "Would back up $sdRoot to $backupRoot"
    Write-Host "Would install U-Boot static TFTP bootstrap on disk $($disk.Number)"
    return
}

New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
Get-ChildItem -LiteralPath $sdRoot -Force |
    Where-Object Name -ne 'System Volume Information' |
    Copy-Item -Destination $backupRoot -Recurse -Force
if (-not (Get-ChildItem -LiteralPath $backupRoot -Force)) {
    throw 'SD backup is empty'
}
$backupFiles = Get-ChildItem -LiteralPath $backupRoot -File -Recurse
$backupEntries = foreach ($file in $backupFiles) {
    [pscustomobject]@{
        path = [IO.Path]::GetRelativePath($backupRoot, $file.FullName)
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash
        bytes = $file.Length
    }
}
[ordered]@{
    created = (Get-Date).ToString('o')
    diskNumber = $disk.Number
    diskModel = $disk.FriendlyName
    backupFiles = @($backupEntries)
} | ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath (Join-Path $backupDirectory 'manifest.json') -Encoding utf8

if ($PSCmdlet.ShouldProcess($sdRoot, 'Install U-Boot static TFTP bootstrap')) {
    foreach ($name in @('bootcode.bin', 'start.elf', 'fixup.dat')) {
        $destination = Join-Path $sdRoot $name
        if ([IO.Path]::GetFullPath($firmware[$name]) -ne [IO.Path]::GetFullPath($destination)) {
            Copy-Item -LiteralPath $firmware[$name] -Destination $destination -Force
        }
    }
    Copy-Item -LiteralPath $uboot.FullName -Destination (Join-Path $sdRoot 'u-boot.bin') -Force
    Copy-Item -LiteralPath $script.FullName -Destination (Join-Path $sdRoot 'boot.scr') -Force
    Copy-Item -LiteralPath $dtb.FullName -Destination (Join-Path $sdRoot 'bcm2710-rpi-3-b.dtb') -Force
    $oldOverlay = Join-Path $sdRoot 'overlays\disable-bt.dtbo'
    if (Test-Path -LiteralPath $oldOverlay) { Remove-Item -LiteralPath $oldOverlay -Force }
    $oldDtb = Join-Path $sdRoot 'bcm2837-rpi-3-b.dtb'
    if (Test-Path -LiteralPath $oldDtb) { Remove-Item -LiteralPath $oldDtb -Force }
    Set-Content -LiteralPath (Join-Path $sdRoot 'config.txt') -Value $configLines -Encoding ascii
}

$installed = foreach ($name in @(
    'bootcode.bin', 'start.elf', 'fixup.dat', 'config.txt',
    'u-boot.bin', 'boot.scr', 'bcm2710-rpi-3-b.dtb'
)) {
    $file = Get-Item -LiteralPath (Join-Path $sdRoot $name)
    [pscustomobject]@{
        Name = $name
        Bytes = $file.Length
        SHA256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash
    }
}
Write-Host "Host backup: $backupRoot"
$installed | Format-Table -AutoSize
