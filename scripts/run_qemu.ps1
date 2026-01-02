#!/usr/bin/env pwsh
# QEMU Launch Script for ViOS RISC-V Kernel

$QEMU = "C:\Program Files\qemu\qemu-system-riscv64.exe"
if (-not (Test-Path $QEMU)) {
    $QEMU = "qemu-system-riscv64"
}
$KERNEL = "target/riscv64gc-unknown-none-elf/debug/kernel"

Write-Host "🚀 Launching ViOS in QEMU (RISC-V)..." -ForegroundColor Cyan

# Check if kernel binary exists
if (-not (Test-Path $KERNEL)) {
    Write-Host "❌ Kernel binary not found at: $KERNEL" -ForegroundColor Red
    Write-Host "   Run: cargo build --target riscv64gc-unknown-none-elf -p vios-hello" -ForegroundColor Yellow
    exit 1
}

# Launch QEMU
& $QEMU `
    -machine virt `
    -cpu rv64 `
    -m 512M `
    -serial stdio `
    -bios default `
    -device virtio-gpu-device `
    -drive file=fat:rw:vios_data,format=raw,id=hd0,if=none `
    -device virtio-blk-device,drive=hd0 `
    -kernel $KERNEL

Write-Host "`n✅ QEMU session ended" -ForegroundColor Green
