# Run Cellos on QEMU raspi3b (BCM2837) — board-rpi3 kernel test.
#
# Prerequisites:
#   1. Install qemu-system-aarch64 >= 7.0 (raspi3b support).
#   2. Build the board-rpi3 kernel:
#        $env:RUSTFLAGS = "-C relocation-model=pic"
#        cargo build --release --features board-rpi3 -p vicell-kernel `
#            --target aarch64-unknown-none-softfloat
#        $env:RUSTFLAGS = $null
#
# Two boot modes:
#   kernel-direct  — loads kernel via -kernel flag (no SD image needed, fast)
#   sd-image       — boots from disk_rpi3.img (full flow, requires gen_disk_rpi3.ps1)
#
# QEMU raspi3b provides:
#   BCM2837 mini UART at 0x3F215000  — console on stdio
#   BCM2837 GPIO       at 0x3F200000  — gpio-bcm driver cell
#   BCM2836 local ctrl at 0x40000000  — timer + interrupt controller
#   ARM Generic Timer  — CNTFRQ_EL0 as reported by QEMU (may differ from 19.2 MHz)
#   No VirtIO (real board; cells must load from SD/EMMC only)
#
# Press Ctrl-a x to quit QEMU.

param(
    [switch]$SdImage,          # boot from disk_rpi3.img instead of -kernel
    [switch]$Gdb,              # halt at boot + open GDB port :1234
    [string]$Disk = "disk_rpi3.img"
)

$ErrorActionPreference = "Stop"

$qemu = "qemu-system-aarch64"
if (-not (Get-Command $qemu -ErrorAction SilentlyContinue)) {
    $candidates = @(
        "C:\Program Files\qemu\qemu-system-aarch64.exe",
        "C:\Program Files (x86)\qemu\qemu-system-aarch64.exe"
    )
    $found = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($found) { $qemu = $found }
    else {
        Write-Host "qemu-system-aarch64 not found. Install QEMU >= 7.0 and add it to PATH."
        exit 1
    }
}

$target = "aarch64-unknown-none-softfloat"
$kernel  = "target/$target/release/vicell-kernel"

Write-Host "[rpi3] Building board-rpi3 kernel..."
# relocation-model=pic: kernel self-relocates at _start via GOT-indirect (ldr pseudo).
# board-rpi3 feature: BCM2837 UART/GPIO/IRQ paths, linker-rpi3.ld (0x80000 load addr).
$env:RUSTFLAGS = "-C relocation-model=pic"
cargo build --release --features board-rpi3 -p vicell-kernel --target $target
$env:RUSTFLAGS = $null

if (-not (Test-Path $kernel)) {
    Write-Host "board-rpi3 kernel build failed."
    Write-Host "  rustup target add aarch64-unknown-none-softfloat"
    exit 1
}

Write-Host "[rpi3] Starting QEMU raspi3b..."
Write-Host "[rpi3] Console: BCM mini UART (stdio)"
Write-Host "[rpi3] Press Ctrl-a x to quit."
Write-Host ""

$qemu_args = @(
    "-machine", "raspi3b",
    "-cpu", "cortex-a53",
    "-m", "1G",
    # QEMU raspi3b serial wiring:
    #   serial_hd(0) = PL011 UART  at 0x3F201000 → used by Bluetooth on real Pi
    #   serial_hd(1) = BCM AUX UART at 0x3F215040 → our BCM mini UART driver
    # Route serial_hd(0) to null and serial_hd(1) to stdio so our driver's output
    # appears on the terminal.  -display none avoids -nographic's monitor-on-stdio
    # conflict that causes "could not connect" on Windows QEMU 10.x.
    "-display", "none",
    "-serial", "null",   # serial_hd(0): PL011 — discard
    "-serial", "stdio"   # serial_hd(1): BCM AUX mini UART — our console
)

if ($Gdb) {
    $qemu_args += @("-s", "-S")
    Write-Host "[rpi3] GDB server active on :1234 — kernel halted at boot."
    Write-Host "[rpi3]   Connect: gdb -ex 'target remote :1234'"
}

if ($SdImage) {
    if (-not (Test-Path $Disk)) {
        Write-Host "SD image not found: $Disk"
        Write-Host "Build it with: .\gen_disk_rpi3.ps1"
        exit 1
    }
    Write-Host "[rpi3] Booting from SD image: $Disk"
    $qemu_args += @("-drive", "if=sd,file=$Disk,format=raw")
} else {
    Write-Host "[rpi3] Booting kernel-direct: $kernel"
    $qemu_args += @("-kernel", $kernel)
}

& $qemu @qemu_args
