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
    # The shell resolves a bare name under /bin itself; an explicit /bin/<name> arrives at the loader
    # as /bin//bin/<name> and is refused ("DENY launch edge ... spawn_cap=false"), which reads as a
    # missing command.
    [string]$ShellCommand = 'ai-test',
    [string]$InterfaceAlias = 'Ethernet',
    [int]$ExpectedInterfaceIndex = 26,
    [switch]$ApplyNetworkConfig,
    [switch]$ApplyFirewall,
    # Use a TFTP server that is already serving this lane (it re-reads cellos.uimg per request, so a
    # running server picks up a swapped payload). Skips starting a second one -- only one process can
    # hold UDP 69.
    [switch]$SkipServer,
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
    $repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
    $EvidenceDir = Join-Path $repoRoot '.agents\260914-ai-oracle-arm64\evidence'
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

# 3. NIC + firewall preflight. With -SkipServer the lane's own script cannot be asked to preflight:
# it treats a busy UDP 69 as a failure precisely because it wants to bind that port, and here the
# busy port *is* the server we are borrowing. So check the two facts we actually need instead.
if ($SkipServer) {
    $bound = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.IPAddress -eq '192.168.42.1' -and $_.InterfaceIndex -eq $ExpectedInterfaceIndex }
    if (-not $bound) { throw 'netboot address 192.168.42.1 is not assigned to the expected interface' }
    $holder = Get-NetUDPEndpoint -LocalPort 69 -ErrorAction SilentlyContinue
    if (-not $holder) { throw 'SkipServer was requested but nothing is serving UDP 69' }
    Write-Host ("[ai-oracle] preflight: 192.168.42.1 on ifIndex {0}, UDP 69 held by pid {1}" -f `
        $ExpectedInterfaceIndex, (($holder | ForEach-Object OwningProcess) -join ', '))
} else {
    & pwsh -NoProfile -File $serveScript -InterfaceAlias $InterfaceAlias `
        -ExpectedInterfaceIndex $ExpectedInterfaceIndex -ApplyNetworkConfig:$ApplyNetworkConfig `
        -ApplyFirewall:$ApplyFirewall -PreflightOnly
    if ($LASTEXITCODE -ne 0) { throw 'network preflight failed' }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$logPath = Join-Path $EvidenceDir "rpi3-$Variant-$stamp.log"
$server = $null
if ($SkipServer) {
    Write-Host ("[ai-oracle] using the running TFTP server; log {0}" -f $logPath)
} else {
    $server = Start-Process -FilePath 'pwsh' -PassThru -WindowStyle Hidden -ArgumentList @(
        '-NoProfile', '-File', $serveScript,
        '-InterfaceAlias', $InterfaceAlias, '-ExpectedInterfaceIndex', "$ExpectedInterfaceIndex"
    )
    Write-Host ("[ai-oracle] TFTP server pid {0}; log {1}" -f $server.Id, $logPath)
}

$header = @(
    "# Cellos AI inference oracle over RPi3 netboot",
    "# variant: $Variant",
    "# payload: $staged ($((Get-Item $staged).Length) bytes, sha256 $stagedHash)",
    "# serial: $ComPort at $Baud baud, driven=$DriveShell, command=$ShellCommand",
    "# lines below are streamed as they arrive; the verdict is appended at the end",
    ''
)
Set-Content -LiteralPath $logPath -Value ($header -join "`n") -Encoding utf8

$serial = New-Object System.IO.Ports.SerialPort $ComPort, $Baud, 'None', 8, 'One'
$serial.NewLine = "`n"
$transcript = New-Object System.Collections.Generic.List[string]
$sentOracle = $false
$verdict = 'TIMEOUT'
$started = Get-Date
$promptSeenAt = $null
$bytesSeen = 0
$lastWakeAt = $null
$bootSent = $false

function Write-Transcript([string]$text) {
    if (-not $text) { return }
    foreach ($line in ($text -split "`r?`n")) {
        if ($line.Trim().Length -eq 0) { continue }
        $stamped = '{0:HH:mm:ss.fff} {1}' -f (Get-Date), $line
        $transcript.Add($stamped)
        Add-Content -LiteralPath $script:logPath -Value $stamped -ErrorAction SilentlyContinue
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
        if ($chunk) { $bytesSeen += $chunk.Length }
        # A board that is already sitting at a bootloader prompt prints nothing until it is spoken to,
        # and one that is mid-countdown stops autobooting on the first byte it receives. Both are
        # recovered by the same move: nudge the line (a bare CR), then, if that lands on a U-Boot
        # prompt, ask it to boot. Nothing is sent while the console is quiet and the Pi is off.
        if ($bytesSeen -eq 0 -and -not $lastWakeAt -and
            ((Get-Date) - $started).TotalSeconds -ge 45) {
            $serial.Write("`r")
            $lastWakeAt = Get-Date
            Write-Transcript '[ai-oracle] no console output yet; sent a CR to wake the line'
        }
        if ($chunk) {
            Write-Transcript $chunk
            $all = ($transcript -join "`n")
            # Wait for the service to have a model before typing the command: `describe` reports
            # zero vocabulary until the load finishes, and the oracle would fail on a boot race
            # rather than on anything it is meant to measure. If the readiness line never arrives
            # (a board that already booted before the capture opened the port prints nothing until
            # it is spoken to), fall back to the prompt alone after 20 s: the oracle reports what
            # the service actually has, so a stale-but-real console still produces real evidence.
            if ($all -match 'Cellos\s*>' -and -not $promptSeenAt) { $promptSeenAt = Get-Date }
            $ready = ($all -match '\[ai\] model ready') -or
                     ($promptSeenAt -and ((Get-Date) - $promptSeenAt).TotalSeconds -ge 20)
            if ($DriveShell -and -not $sentOracle -and $all -match 'Cellos\s*>' -and $ready) {
                Start-Sleep -Milliseconds 700
                $serial.Write("$ShellCommand`r")
                Write-Transcript "[ai-oracle] sent: $ShellCommand"
                $sentOracle = $true
            }
            if ($all -match '\[ai\] model ready') {
                if (-not $script:modelReadyLogged) {
                    $script:modelReadyLogged = $true
                    Write-Host '[ai-oracle] service reported its model resident'
                }
            }
            if ($all -match '(?m)(=>|U-Boot>)\s*$' -and -not $bootSent) {
                Start-Sleep -Milliseconds 400
                $serial.Write("boot`r")
                Write-Transcript '[ai-oracle] U-Boot prompt detected; sent: boot'
                $bootSent = $true
                $promptSeenAt = $null
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
    # A server we did not start keeps running: it is the operator's, not ours to kill.
}

$trailer = @(
    '',
    ("# verdict: {0}" -f $verdict),
    ("# elapsed: {0} s" -f [int]((Get-Date) - $started).TotalSeconds)
)
Add-Content -LiteralPath $logPath -Value ($trailer -join "`n") -Encoding utf8
Write-Host ''
Write-Host ("[ai-oracle] verdict: {0}  (log: {1})" -f $verdict, $logPath)
if ($verdict -ne 'PASS') { exit 1 }
