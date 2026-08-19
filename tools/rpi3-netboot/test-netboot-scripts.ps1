$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'serve-rpi3-netboot.ps1'
$buildScript = Join-Path $PSScriptRoot 'build-rpi3-uboot-static.sh'
$cellBuildScript = Join-Path $PSScriptRoot '..\..\scripts\build-aarch64-cells.ps1'
$inputManifest = Join-Path $PSScriptRoot '..\..\cells\services\input\Cargo.toml'
$inputSource = Join-Path $PSScriptRoot '..\..\cells\services\input\src\virtio_device.rs'
$inputDispatcherSource = Join-Path $PSScriptRoot '..\..\cells\services\input\src\dispatcher.rs'
$shellExecutorSource = Join-Path $PSScriptRoot '..\..\cells\tools\shell\src\executor.rs'
$consoleSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\console_drv.rs'
$uartSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\uart.rs'
$trapSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\trap.rs'
$miniUartSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\uart_bcm_mini.rs'
$legacyIrqSource = Join-Path $PSScriptRoot '..\..\hal\arch\arm\src\aarch64\bcm2835_legacy_irq.rs'
$bcm27xxProfileSource = Join-Path $PSScriptRoot '..\..\hal\soc\bcm27xx\src\profile.rs'
$syscallSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\syscall.rs'
$taskSource = Join-Path $PSScriptRoot '..\..\kernel\src\task.rs'
$tcbSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\tcb.rs'
$mmcCoreSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\core.rs'
$sdhciSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\sdhci.rs'
$mmcPinmuxSource = Join-Path $PSScriptRoot '..\..\kernel\src\task\drivers\mmc\pinmux_bcm.rs'
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
if ($consoleRustSource -notmatch '(?s)fn relay_ascii_to_input.*?ipc_post_nonblock\(' -or
    $consoleRustSource -match '(?s)fn relay_ascii_to_input.*?ipc_post_nonblock_bounded\(' -or
    $consoleRustSource -match '(?s)fn relay_ascii_to_input.*?copy_from_slice\(&0u32\.to_le_bytes\(\)\)') {
    throw 'RPi3 UART relay must preserve the 64-event first-hop bound without synthetic release events'
}
$miniUartRustSource = Get-Content -Raw -LiteralPath $miniUartSource
$legacyIrqRustSource = Get-Content -Raw -LiteralPath $legacyIrqSource
$bcm27xxProfileRustSource = Get-Content -Raw -LiteralPath $bcm27xxProfileSource
if ($miniUartRustSource -notmatch 'enable_rx_interrupt' -or
    $miniUartRustSource -notmatch 'wr\(AUX_MU_IER, 1\)' -or
    $miniUartRustSource -notmatch '(?s)pub fn try_putchar.*?return false.*?wr\(AUX_MU_IO') {
    throw 'RPi3 mini UART must enable interrupt-backed RX'
}
if ($legacyIrqRustSource -notmatch 'AUX_IRQ: u32 = hal_soc_bcm27xx::BCM2837\.irq\.aux' -or
    $bcm27xxProfileRustSource -notmatch 'aux:\s*29' -or
    $legacyIrqRustSource -notmatch 'is_aux_irq_pending') {
    throw 'RPi3 legacy controller must route AUX IRQ 29'
}
$trapRustSource = Get-Content -Raw -LiteralPath $trapSource
if ($trapRustSource -notmatch '(?s)is_aux_irq_pending\(\).*?vi_handle_uart_irq\(\)') {
    throw 'RPi3 trap path must drain the mini UART RX interrupt'
}
if ($trapRustSource -notmatch '(?s)timer::reset\(\);\s*if aux_pending \{.*?vi_handle_uart_irq\(\);.*?vi_timer_tick\(\);') {
    throw 'RPi3 timer IRQ must drain a co-pending mini UART before the scheduler tick'
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
$uartRustSource = Get-Content -Raw -LiteralPath $uartSource
if ($uartRustSource -notmatch '(?s)fn write_rpi3_console_byte.*?vi_handle_uart_irq\(\).*?try_putchar\(byte\)') {
    throw 'RPi3 synchronous console TX must drain RX while waiting for FIFO space'
}
$syscallRustSource = Get-Content -Raw -LiteralPath $syscallSource
if ($syscallRustSource -match '\[rpi3\] Log syscall') {
    throw 'RPi3 console must not synchronously warn for every user log syscall'
}
if ($syscallRustSource -notmatch '(?s)Syscall::RecvTimeout.*?let drained = .*?begin_receive_context\(mask\).*?pending_msgs' -or
    $syscallRustSource -notmatch '(?s)Syscall::TryRecv.*?let drained = .*?begin_receive_context\(mask\).*?pending_msgs') {
    throw 'Receive-context maintenance must reuse the pending-message scheduler lock'
}
$tcbRustSource = Get-Content -Raw -LiteralPath $tcbSource
if ($tcbRustSource -notmatch 'INPUT_EVENT_QUEUE_DEPTH: usize = 512') {
    throw 'RPi3 input backpressure must retain the bounded 512-event scheduling cushion'
}
$inputDispatcherRustSource = Get-Content -Raw -LiteralPath $inputDispatcherSource
if ($inputDispatcherRustSource -notmatch '(?s)fn send_keyboard_event.*?sys_send\(target, &buf\)' -or
    $inputDispatcherRustSource -notmatch '(?s)fn try_send_event.*?sys_try_send\(target, &buf\)' -or
    $inputDispatcherRustSource -notmatch 'SyscallResult::Ok\(0\) => Ok\(\(\)\)') {
    throw 'Input service must block keyboard delivery while keeping mouse dispatch nonblocking'
}
$shellExecutorRustSource = Get-Content -Raw -LiteralPath $shellExecutorSource
$cmdReadMatch = [regex]::Match($shellExecutorRustSource, '(?s)fn cmd_read.*?(?=\r?\n/// `source)')
if (-not $cmdReadMatch.Success -or
    $cmdReadMatch.Value -notmatch 'ostd::io::stdin\(\)\.read_line' -or
    $cmdReadMatch.Value -match 'sys_read\(0') {
    throw 'Shell read builtin must receive through the focus-aware input service path'
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
    $sdhciRustSource -notmatch 'space_controller_write' -or
    $sdhciRustSource -notmatch 'off != SDHCI_BUFFER' -or
    $sdhciRustSource -notmatch 'policy\.word_access_only' -or
    $sdhciRustSource -notmatch 'policy\.minimum_write_spacing_us' -or
    $bcm27xxProfileRustSource -notmatch 'word_access_only:\s*true' -or
    $bcm27xxProfileRustSource -notmatch 'minimum_write_spacing_us:\s*6') {
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
