[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$KernelImage,
    [string]$Root = (Join-Path $PSScriptRoot 'root')
)

$ErrorActionPreference = 'Stop'
$source = Get-Item -LiteralPath $KernelImage
if ($source.Length -lt 1MB) { throw 'Kernel image is unexpectedly small' }
$header = [IO.File]::ReadAllBytes($source.FullName)[0..3]
if ([Text.Encoding]::ASCII.GetString($header) -eq "`u{7f}ELF") {
    throw 'KernelImage must be a raw image, not ELF'
}
if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
    throw "TFTP root not found: $Root"
}
$target = Join-Path $Root 'cellos.uimg'
$next = Join-Path $Root 'cellos.uimg.next'
& py -3 (Join-Path $PSScriptRoot 'rpi3-uimage.py') `
    --input $source.FullName --output $next
if ($LASTEXITCODE -ne 0) { throw 'Failed to create Cellos uImage' }
$sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $source.FullName).Hash
& py -3 (Join-Path $PSScriptRoot 'rpi3-uimage.py') --verify $next
if ($LASTEXITCODE -ne 0) { throw 'TFTP uImage verification failed' }
Move-Item -LiteralPath $next -Destination $target -Force
$finalHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash
[pscustomobject]@{
    Path = $target
    PayloadBytes = $source.Length
    PayloadSHA256 = $sourceHash
    ImageSHA256 = $finalHash
} | Format-List
