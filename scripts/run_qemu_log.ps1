#!/usr/bin/env pwsh
$QEMU = "C:\Program Files\qemu\qemu-system-riscv64.exe"
if (-not (Test-Path $QEMU)) {
    $QEMU = "qemu-system-riscv64"
}
$KERNEL = "target/riscv64gc-unknown-none-elf/debug/kernel"

Write-Host "Launching QEMU (logging to qemu.log)..."
& $QEMU `
    -machine virt `
    -cpu rv64 `
    -m 512M `
    -nographic `
    -serial file:qemu.log `
    -bios none `
    -device virtio-gpu-device `
    -kernel $KERNEL

