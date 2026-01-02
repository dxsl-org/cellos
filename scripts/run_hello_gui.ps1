#!/usr/bin/env pwsh
# Script chạy ViOS Hello App với giao diện GUI

$QEMU = "C:\Program Files\qemu\qemu-system-riscv64.exe"
$HELLO = "target/riscv64gc-unknown-none-elf/debug/hello"

Write-Host "🚀 Đang khởi động ViOS GUI Mode..." -ForegroundColor Cyan

# Kiểm tra file kernel
if (-not (Test-Path $HELLO)) {
    Write-Host "❌ Kernel không tồn tại: $HELLO" -ForegroundColor Red
    Write-Host "   Run: cargo build --target riscv64gc-unknown-none-elf -p vios-hello" -ForegroundColor Yellow
    exit 1
}

# Chạy QEMU:
# - Bỏ -nographic để hiện cửa sổ
# - -serial mon:stdio để log ra terminal và dùng monitor
# - -device virtio-gpu-device để bật đồ họa
& $QEMU `
    -machine virt `
    -cpu rv64 `
    -m 512M `
    -serial mon:stdio `
    -device virtio-gpu-device `
    -bios none `
    -kernel $HELLO `
    -d guest_errors

Write-Host "`n✅ QEMU session ended" -ForegroundColor Green
