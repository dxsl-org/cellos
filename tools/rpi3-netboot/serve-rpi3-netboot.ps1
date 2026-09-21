[CmdletBinding()]
param(
    [string]$InterfaceAlias = 'Ethernet',
    # Stable physical identity of the netboot NIC. An interface *index* is not
    # an identity: Windows reassigns it across reboots, driver updates, and
    # re-plugs, which is exactly what turned a correct safety guard into a
    # spurious failure. The MAC is fixed to the adapter.
    [string]$ExpectedMacAddress = '',
    [string]$ServerAddress = '192.168.42.1',
    [string]$ClientAddress = '192.168.42.2',
    [string]$Root = '',
    [switch]$ApplyNetworkConfig,
    [switch]$ApplyFirewall,
    [switch]$RestoreNetwork,
    [switch]$PreflightOnly
)

$ErrorActionPreference = 'Stop'
$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $Root) { $Root = Join-Path $scriptDir 'root' }
$prefixLength = 24
$stateDir = Join-Path $scriptDir 'state'
$logsDir = Join-Path $scriptDir 'logs'
$statePath = Join-Path $stateDir 'network-before.json'
$firewallName = 'Cellos-RPi3-Netboot-Ethernet'
$required = @('cellos.uimg')
$adapter = Get-NetAdapter -Name $InterfaceAlias
if ($adapter.InterfaceDescription -notmatch 'Ethernet') {
    throw "Refusing non-Ethernet adapter: $($adapter.InterfaceDescription)"
}

# Normalising both sides keeps the pin independent of the adapter's MAC
# formatting (dashes, colons, or none).
function Get-NormalizedMac([string]$Mac) {
    return ($Mac -replace '[^0-9A-Fa-f]', '').ToUpperInvariant()
}
$adapterMac = Get-NormalizedMac $adapter.MacAddress
$macPinned = -not [string]::IsNullOrWhiteSpace($ExpectedMacAddress)
Write-Host "[netboot] adapter: alias=$($adapter.Alias) mac=$($adapter.MacAddress) ifIndex=$($adapter.ifIndex) status=$($adapter.Status)"

# Every action below that changes host network state must be aimed at an adapter
# the operator has identified by its physical address. Read-only runs print the
# identity instead, so a first run tells you the MAC to pin.
$mutatesNetwork = $ApplyNetworkConfig -or $ApplyFirewall -or $RestoreNetwork
if ($macPinned -and (Get-NormalizedMac $ExpectedMacAddress) -ne $adapterMac) {
    throw "Adapter MAC mismatch: $($adapter.MacAddress) is not $ExpectedMacAddress"
}
if ($mutatesNetwork -and -not $macPinned) {
    throw ("Refusing to reconfigure an unidentified adapter.`n" +
           "  Re-run with: -ExpectedMacAddress $($adapter.MacAddress)")
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
        [ordered]@{
            dhcp = $ipInterface.Dhcp.ToString()
            manualAddresses = @($manual)
            macAddress = $adapter.MacAddress
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $statePath -Encoding utf8
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
    throw ("Assign $ServerAddress first: rerun as Admin with " +
           "-ExpectedMacAddress $($adapter.MacAddress) -ApplyNetworkConfig -ApplyFirewall")
}
$conflicts = Get-NetUDPEndpoint -LocalPort 69 -ErrorAction SilentlyContinue
if ($conflicts) { throw "UDP port already in use on target address: $($conflicts.LocalPort -join ', ')" }
Write-Host "Preflight PASS: $InterfaceAlias mac=$($adapter.MacAddress) ifIndex=$($adapter.ifIndex) $ServerAddress/$prefixLength status=$($adapter.Status)"
if ($PreflightOnly) { return }

New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
$log = Join-Path $logsDir ("server-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
Write-Host "Server log: $log"
& py -3.12 (Join-Path $PSScriptRoot 'rpi3-dhcp-tftp.py') --bind 0.0.0.0 `
    --client $ClientAddress --root $Root --log $log --bind-wait 3600
