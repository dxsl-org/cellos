# serve-ai-oracle.ps1 — netboot the Spec 24 AI image over the static TFTP lane and capture the
# oracle's own output from the board's UART.
#
# This is a driver for tools/rpi3-netboot/serve-rpi3-netboot.ps1, not a replacement: it selects the
# payload, applies the same NIC/firewall preflight, starts that server, and reads the console.
#
# Run from an Administrator PowerShell on the Windows host (the lane binds the physical NIC; WSL2
# cannot receive those packets). Two variants are prepared in root/:
#
#   fixture  cellos-ai-fixture.uimg  10 027 008 B  deterministic fixture: golden ids + embedding
#   15m      cellos-ai-15m.uimg      43 581 440 B  real 25.6 MiB checkpoint: the memory-budget number
#
# Suggested order: fixture first (proves the whole chain on the board in a minute, and that payload
# size is the one this lane has already served), then 15m for the CP-3 measurement.
#
# Examples:
#   pwsh -File .\tools\rpi3-netboot\serve-ai-oracle.ps1 -Variant fixture -ApplyNetworkConfig -ApplyFirewall
#   pwsh -File .\tools\rpi3-netboot\serve-ai-oracle.ps1 -Variant 15m -ComPort COM3
#
# The UART adapter must be able to *write* to the Pi (adapter TX -> Pi RXD0, pin 10) for -DriveShell
# to type the oracle command. With only Pi TXD0 -> adapter RX (the unattended-autoboot wiring), pass
# -DriveShell:$false and type /bin/ai-test yourself when the prompt appears.

[CmdletBinding()]
param(
    [ValidateSet('fixture', '15m')]
    [string]$Variant = 'fixture',
    [string]$ComPort = '',
    [int]$Baud = 115200,
    [switch]$DriveShell = $true,
    [int]$TimeoutSec = 420,
    [string]$InterfaceAlias = 'Ethernet',
    [int]$ExpectedInterfaceIndex = 26,
    [switch]$ApplyNetworkConfig,
    [switch]$ApplyFirewall,
    [string]$EvidenceDir = ''
)

$ErrorActionPreference = 'Stop'
$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$root = Join-Path $scriptDir 'root'
$serveScript = Join-Path $scriptDir 'serve-rpi3-netboot.ps1'

$payload = switch ($Variant) {
    'fixture' { Join-Path $root 'cellos-ai-fixture.uimg' }
    '15m' { Join-Path $root 'cellos-ai-15m.uimg' }
}
if (-not (Test-Path -LiteralPath $payload)) {
    throw "payload missing: $payload (build it with scripts/build-aarch64-cells.ps1 -BoardRpi3 -AiModel <gguf>, then rpi3-uimage.py)"
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
if (($ApplyNetworkConfig -or $ApplyFirewall) -and -not (Test-Administrator)) {
    throw 'ApplyNetworkConfig/-ApplyFirewall require Administrator PowerShell'
}
if (-not $EvidenceDir) {
    $EvidenceDir = Join-Path (Split-Path -Parent $scriptDir) '.agents\260914-ai-oracle-arm64\evidence'
}
New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null

# 1. Point the lane at the chosen payload. The server serves exactly `cellos.uimg`, so the payload
# already staged there is kept aside first — the lane's other users should not lose their image to an
# AI run.
$staged = Join-Path $root 'cellos.uimg'
if (Test-Path -LiteralPath $staged) {
    $keep = Join-Path $root ("cellos.uimg.before-ai-{0}" -f (Get-Date -Format 'yyyyMMddHHmmss'))
    Copy-Item -LiteralPath $staged -Destination $keep -Force
    Write-Host "[ai-oracle] previous payload kept as $([IO.Path]::GetFileName($keep))"
}
Copy-Item -LiteralPath $payload -Destination $staged -Force
$stagedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $staged).Hash
Write-Host ("[ai-oracle] payload {0}: {1} bytes sha256={2}" -f $Variant, (Get-Item $staged).Length, $stagedHash)

# 2. Serial port. List what the host has so a wrong -ComPort is obvious rather than confusing.
$ports = [System.IO.Ports.SerialPort]::GetPortNames()
Write-Host ("[ai-oracle] serial ports: {0}" -f (($ports | Sort-Object) -join ', '))
if (-not $ComPort) {
    if ($ports.Count -eq 1) {
        $ComPort = $ports[0]
        Write-Host "[ai-oracle] using the only port: $ComPort"
    } else {
        throw "pass -ComPort (available: $($ports -join ', '))"
    }
}

# 3. NIC + firewall preflight through the lane's own script, so the rules stay in one place.
& pwsh -NoProfile -File $serveScript -InterfaceAlias $InterfaceAlias `
    -ExpectedInterfaceIndex $ExpectedInterfaceIndex -ApplyNetworkConfig:$ApplyNetworkConfig `
    -ApplyFirewall:$ApplyFirewall -PreflightOnly
if ($LASTEXITCODE -ne 0) { throw 'network preflight failed' }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$logPath = Join-Path $EvidenceDir "rpi3-$Variant-$stamp.log"
$server = Start-Process -FilePath 'pwsh' -PassThru -WindowStyle Hidden -ArgumentList @(
    '-NoProfile', '-File', $serveScript,
    '-InterfaceAlias', $InterfaceAlias, '-ExpectedInterfaceIndex', "$ExpectedInterfaceIndex"
)
Write-Host ("[ai-oracle] TFTP server pid {0}; log {1}" -f $server.Id, $logPath)

$serial = New-Object System.IO.Ports.SerialPort $ComPort, $Baud, 'None', 8, 'One'
$serial.NewLine = "`n"
$transcript = New-Object System.Collections.Generic.List[string]
$sentOracle = $false
$verdict = 'TIMEOUT'
$started = Get-Date

function Write-Transcript([string]$text) {
    if (-not $text) { return }
    foreach ($line in ($text -split "`r?`n")) {
        if ($line.Trim().Length -eq 0) { continue }
        $stamped = '{0:HH:mm:ss.fff} {1}' -f (Get-Date), $line
        $transcript.Add($stamped)
        Write-Host $stamped
    }
}

try {
    $serial.Open()
    Write-Host ''
    Write-Host '============================================================'
    Write-Host '  Power on the Raspberry Pi 3 now (server is already serving).'
    Write-Host "  Watching for the oracle verdict for $TimeoutSec s."
    Write-Host '============================================================'
    while (((Get-Date) - $started).TotalSeconds -lt $TimeoutSec) {
        $chunk = $serial.ReadExisting()
        if ($chunk) {
            Write-Transcript $chunk
            $all = ($transcript -join "`n")
            # Wait for the service to have a model before typing the command: `describe` reports
            # zero vocabulary until the load finishes, and the oracle would fail on a boot race
            # rather than on anything it is meant to measure.
            if ($DriveShell -and -not $sentOracle -and $all -match 'Cellos\s*>\s*$' -and
                $all -match '\[ai\] model ready') {
                Start-Sleep -Milliseconds 700
                $serial.Write("/bin/ai-test`r")
                Write-Transcript '[ai-oracle] sent: /bin/ai-test'
                $sentOracle = $true
            }
            if ($all -match '\[ai\] model ready') {
                if (-not $script:modelReadyLogged) {
                    $script:modelReadyLogged = $true
                    Write-Host '[ai-oracle] service reported its model resident'
                }
            }
            if ($all -match '\[ai-test\] PASS') { $verdict = 'PASS'; break }
            if ($all -match '\[ai-test\] FAIL|KERNEL PANIC') { $verdict = 'FAIL'; break }
        }
        Start-Sleep -Milliseconds 200
    }
} finally {
    if ($serial.IsOpen) { $serial.Close() }
    $serial.Dispose()
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
}

$header = @(
    "# Cellos AI inference oracle over RPi3 netboot",
    "# variant: $Variant",
    "# payload: $staged ($((Get-Item $staged).Length) bytes, sha256 $stagedHash)",
    "# serial: $ComPort at $Baud baud, driven=$DriveShell",
    "# verdict: $verdict",
    "# elapsed: $([int]((Get-Date) - $started).TotalSeconds) s",
    ''
)
Set-Content -LiteralPath $logPath -Value (($header + $transcript) -join "`n") -Encoding utf8
Write-Host ''
Write-Host ("[ai-oracle] verdict: {0}  (log: {1})" -f $verdict, $logPath)
if ($verdict -ne 'PASS') { exit 1 }
