[CmdletBinding()]
param(
    [string]$InterfaceAlias = 'Ethernet',
    [int]$ExpectedInterfaceIndex = 14,
    [string]$ServerAddress = '192.168.42.1',
    [string]$ClientAddress = '192.168.42.2',
    [string]$Root = (Join-Path $PSScriptRoot 'root'),
    [switch]$ApplyNetworkConfig,
    [switch]$ApplyFirewall,
    [switch]$RestoreNetwork,
    [switch]$PreflightOnly
)

$ErrorActionPreference = 'Stop'
$prefixLength = 24
$stateDir = Join-Path $PSScriptRoot 'state'
$logsDir = Join-Path $PSScriptRoot 'logs'
$statePath = Join-Path $stateDir 'network-before.json'
$firewallName = 'Cellos-RPi3-Netboot-Ethernet'
$required = @('cellos.uimg')
$adapter = Get-NetAdapter -Name $InterfaceAlias
if ($adapter.ifIndex -ne $ExpectedInterfaceIndex) {
    throw "Interface index changed: $($adapter.ifIndex), expected $ExpectedInterfaceIndex"
}
if ($adapter.InterfaceDescription -notmatch 'Ethernet') {
    throw "Refusing non-Ethernet adapter: $($adapter.InterfaceDescription)"
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if ($RestoreNetwork) {
    if (-not (Test-Administrator)) { throw 'RestoreNetwork requires Administrator PowerShell' }
    if (-not (Test-Path -LiteralPath $statePath)) { throw "State missing: $statePath" }
    $state = Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json
    Get-NetFirewallRule -Name $firewallName -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object IPAddress -eq $ServerAddress | Remove-NetIPAddress -Confirm:$false
    Set-NetIPInterface -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -Dhcp $state.dhcp
    foreach ($address in $state.manualAddresses) {
        New-NetIPAddress -InterfaceIndex $adapter.ifIndex -IPAddress $address.ip -PrefixLength $address.prefix | Out-Null
    }
    Write-Host "Restored network state for $InterfaceAlias"
    return
}

foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $Root $name))) {
        throw "TFTP root file missing: $name"
    }
}
if ($ApplyNetworkConfig) {
    if (-not (Test-Administrator)) { throw 'ApplyNetworkConfig requires Administrator PowerShell' }
    New-Item -ItemType Directory -Path $stateDir -Force | Out-Null
    if (-not (Test-Path -LiteralPath $statePath)) {
        $ipInterface = Get-NetIPInterface -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4
        $manual = Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 |
            Where-Object PrefixOrigin -eq 'Manual' |
            ForEach-Object { [pscustomobject]@{ ip = $_.IPAddress; prefix = $_.PrefixLength } }
        [ordered]@{ dhcp = $ipInterface.Dhcp.ToString(); manualAddresses = @($manual) } |
            ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $statePath -Encoding utf8
    }
    Set-NetIPInterface -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -Dhcp Disabled
    Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object PrefixOrigin -eq 'Manual' | Remove-NetIPAddress -Confirm:$false
    New-NetIPAddress -InterfaceIndex $adapter.ifIndex -IPAddress $ServerAddress `
        -PrefixLength $prefixLength -PolicyStore ActiveStore | Out-Null
}
if ($ApplyFirewall) {
    if (-not (Test-Administrator)) { throw 'ApplyFirewall requires Administrator PowerShell' }
    Get-NetFirewallRule -Name $firewallName -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -Name $firewallName -DisplayName $firewallName -Direction Inbound `
        -Action Allow -Protocol UDP -LocalPort 69 -LocalAddress $ServerAddress `
        -InterfaceAlias $InterfaceAlias -Profile Any | Out-Null
}

$boundAddress = Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue |
    Where-Object IPAddress -eq $ServerAddress
if (-not $boundAddress) {
    throw "Assign $ServerAddress first: rerun as Admin with -ApplyNetworkConfig -ApplyFirewall"
}
$conflicts = Get-NetUDPEndpoint -LocalPort 69 -ErrorAction SilentlyContinue
if ($conflicts) { throw "UDP port already in use on target address: $($conflicts.LocalPort -join ', ')" }
Write-Host "Preflight PASS: $InterfaceAlias ifIndex=$($adapter.ifIndex) $ServerAddress/$prefixLength status=$($adapter.Status)"
if ($PreflightOnly) { return }

New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
$log = Join-Path $logsDir ("server-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
Write-Host "Server log: $log"
& py -3.12 (Join-Path $PSScriptRoot 'rpi3-dhcp-tftp.py') --bind 0.0.0.0 `
    --client $ClientAddress --root $Root --log $log --bind-wait 3600
