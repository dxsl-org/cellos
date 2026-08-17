$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'serve-rpi3-netboot.ps1'
$buildScript = Join-Path $PSScriptRoot 'build-rpi3-uboot-static.sh'
$cellBuildScript = Join-Path $PSScriptRoot '..\..\scripts\build-aarch64-cells.ps1'
$inputManifest = Join-Path $PSScriptRoot '..\..\cells\services\input\Cargo.toml'
$inputSource = Join-Path $PSScriptRoot '..\..\cells\services\input\src\virtio_device.rs'
$consoleSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\console_drv.rs'
$trapSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\trap.rs'
$miniUartSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\uart_bcm_mini.rs'
$legacyIrqSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\bcm2835_legacy_irq.rs'
$syscallSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\syscall.rs'
$taskSource = Join-Path $PSScriptRoot '..\..\kernel\src\task.rs'
$mmcCoreSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\core.rs'
$sdhciSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\sdhci.rs'
$mmcPinmuxSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\pinmux_rpi3.rs'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    $script,
    [ref]$tokens,
    [ref]$errors
)
if ($errors) {
    throw "PowerShell parse failed: $($errors[0].Message)"
}
$firewall = $ast.FindAll({
    param($node)
    $node -is [Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -eq 'New-NetFirewallRule'
}, $true)
if ($firewall.Count -ne 1) {
    throw "Expected one firewall rule declaration, found $($firewall.Count)"
}
$elements = $firewall[0].CommandElements.Extent.Text
if ('-InterfaceAlias' -notin $elements) {
    throw 'Netboot firewall must remain scoped to the Ethernet interface'
}
if ('-LocalAddress' -notin $elements) {
    throw 'Static TFTP firewall must remain scoped to the server address'
}
if ('67' -in $elements) {
    throw 'Static U-Boot must not reopen the DHCP port'
}
if ('69' -notin $elements) {
    throw 'Static U-Boot requires UDP port 69'
}
$serverSource = Get-Content -Raw -LiteralPath $script
if ($serverSource -notmatch '(?m)^& py -3\.12 .* --bind 0\.0\.0\.0') {
    throw 'Netboot server must use the verified Python 3.12 wildcard listener'
}
if ($serverSource -notmatch '--client \$ClientAddress') {
    throw 'Netboot server must reject clients other than the static Pi address'
}
if ($serverSource -notmatch '(?m)^\$conflicts = Get-NetUDPEndpoint -LocalPort 69 -ErrorAction SilentlyContinue\s*$') {
    throw 'Wildcard listener preflight must reject every existing UDP 69 listener'
}
$buildSource = Get-Content -Raw -LiteralPath $buildScript
if ($buildSource -notmatch '--disable BOOTSTD_DEFAULTS') {
    throw 'U-Boot defaults must not force-enable the Linux Image-header path'
}
if ($buildSource -notmatch '--disable CMD_BOOTI') {
    throw 'Raw Cellos boot must disable the Linux Image-header path'
}
if ($buildSource -notmatch "grep -F '# CONFIG_CMD_BOOTI is not set'") {
    throw 'U-Boot build must verify that CMD_BOOTI stayed disabled'
}
if ($buildSource -notmatch "grep -Eq '\^VERSION = 2026\$'") {
    throw 'U-Boot build must reject an unexpected source version'
}
if ($buildSource -notmatch "grep -Eq '\^PATCHLEVEL = 07\$'") {
    throw 'U-Boot build must reject an unexpected source patch level'
}
$cellBuildSource = Get-Content -Raw -LiteralPath $cellBuildScript
if ($cellBuildSource -notmatch '(?s)if \(\$BoardRpi3\).*?--no-default-features.*?--target-dir \$rpi3TargetDir') {
    throw 'RPi3 input cell must disable VirtIO in a separate Cargo target directory'
}
if ($cellBuildSource -notmatch 'refusing to package a stale artifact') {
    throw 'RPi3 input build must fail closed instead of packaging stale output'
}
if ($cellBuildSource -notmatch "Assert-CellBuild 'service-vfs'" -or
    $cellBuildSource -notmatch 'refusing to package stale artifacts') {
    throw 'Every RPi3 embedded cell build must fail closed'
}
if ($cellBuildSource -notmatch 'Resolve-Path \(Join-Path \$PSScriptRoot ''\.\.''\)\)\.ProviderPath') {
    throw 'AArch64 cell build must resolve a native UNC path for clang includes'
}
if ($cellBuildSource -notmatch 'target\\rpi3-embedded') {
    throw 'RPi3 cells must use a separate embedded-artifact directory'
}
$manifestSource = Get-Content -Raw -LiteralPath $inputManifest
if ($manifestSource -notmatch '(?m)^default = \["virtio-mmio"\]$') {
    throw 'Ordinary input builds must retain VirtIO MMIO by default'
}
$inputRustSource = Get-Content -Raw -LiteralPath $inputSource
if ($inputRustSource -notmatch 'cfg\(all\(target_arch = "aarch64", feature = "virtio-mmio"\)\)') {
    throw 'AArch64 QEMU slot iteration must be feature-gated'
}
$consoleRustSource = Get-Content -Raw -LiteralPath $consoleSource
if ($consoleRustSource -notmatch '(?s)cfg\(all\(target_arch = "aarch64", feature = "board-rpi3"\)\).*?uart_bcm_mini::poll_rx') {
    throw 'RPi3 console input must poll the BCM mini UART'
}
if ($consoleRustSource -notmatch '(?s)cfg\(all\(target_arch = "aarch64", not\(feature = "board-rpi3"\)\)\).*?uart_pl011::poll_rx') {
    throw 'Generic AArch64 console input must retain the QEMU PL011 path'
}
$miniUartRustSource = Get-Content -Raw -LiteralPath $miniUartSource
$legacyIrqRustSource = Get-Content -Raw -LiteralPath $legacyIrqSource
if ($miniUartRustSource -notmatch 'enable_rx_interrupt' -or
    $miniUartRustSource -notmatch 'wr\(AUX_MU_IER, 1\)') {
    throw 'RPi3 mini UART must enable interrupt-backed RX'
}
if ($legacyIrqRustSource -notmatch 'AUX_IRQ: u32 = 29' -or
    $legacyIrqRustSource -notmatch 'is_aux_irq_pending') {
    throw 'RPi3 legacy controller must route AUX IRQ 29'
}
$trapRustSource = Get-Content -Raw -LiteralPath $trapSource
if ($trapRustSource -notmatch '(?s)is_aux_irq_pending\(\).*?vi_handle_uart_irq\(\)') {
    throw 'RPi3 trap path must drain the mini UART RX interrupt'
}
if ($trapRustSource -match 'probe_put\(b''T''\)' -or
    $trapRustSource -match 'probe_put\(b''M''\)') {
    throw 'RPi3 trap hot paths must not emit raw per-event UART markers'
}
if ($trapRustSource -notmatch 'probe_uncategorized_el2_fault' -or
    $trapRustSource -notmatch 'FS0') {
    throw 'RPi3 fault-only diagnostics must remain available'
}
$taskRustSource = Get-Content -Raw -LiteralPath $taskSource
if ($taskRustSource -match 'probe_put\(b''A''\)' -or
    $taskRustSource -match 'probe_put\(b''N''\)') {
    throw 'RPi3 scheduler hot paths must not emit raw per-event UART markers'
}
$syscallRustSource = Get-Content -Raw -LiteralPath $syscallSource
if ($syscallRustSource -match '\[rpi3\] Log syscall') {
    throw 'RPi3 console must not synchronously warn for every user log syscall'
}
$mmcCoreRustSource = Get-Content -Raw -LiteralPath $mmcCoreSource
if ($mmcCoreRustSource -notmatch 'const IDENT_CLOCK_HZ: u32 = 400_000;') {
    throw 'RPi3 SD identification must retain the standard 400 kHz clock'
}
if ($mmcCoreRustSource -notmatch '(?s)let sectors = self\.sd_read_csd\(rca\)\?;\s*self\.cmd7_select\(rca\)\?;') {
    throw 'SD CMD9 must read CSD in Standby before CMD7 selects the card'
}
$sdhciRustSource = Get-Content -Raw -LiteralPath $sdhciSource
if ($sdhciRustSource -notmatch 'transfer_mode_shadow' -or
    $sdhciRustSource -notmatch 'space_bcm2835_write' -or
    $sdhciRustSource -notmatch 'off == SDHCI_BUFFER') {
    throw 'RPi3 Arasan accesses must retain 32-bit command shadowing and write spacing'
}
if ($sdhciRustSource -notmatch '(?s)fn setup_data_transfer\(.*?write8\(SDHCI_TIMEOUT_CONTROL, TIMEOUT_MAX\);.*?write16\(SDHCI_BLOCK_SIZE, block_size\);.*?write16\(SDHCI_BLOCK_COUNT, block_count\);.*?write16\(SDHCI_TRANSFER_MODE, transfer_mode\);') {
    throw 'RPi3 data commands must program the SDHCI timeout before transfer registers'
}
$mmcPinmuxRustSource = Get-Content -Raw -LiteralPath $mmcPinmuxSource
if ($mmcPinmuxRustSource -notmatch 'for pin in 34\.\.=39' -or
    $mmcPinmuxRustSource -notmatch 'for pin in 48\.\.=53' -or
    $mmcPinmuxRustSource -notmatch 'GPIO_ALT3: u32 = 7') {
    throw 'RPi3 must disconnect Wi-Fi SDIO pins and route the external slot to Arasan'
}
if ($mmcPinmuxRustSource -match 'GPPUD') {
    throw 'RPi3 Arasan pinmux must preserve firmware-configured SD pull resistors'
}
Write-Host 'PASS: static TFTP, raw Cellos boot, and RPi3 input guards are present'
