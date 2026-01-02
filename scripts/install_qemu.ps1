#!/usr/bin/env pwsh
# QEMU Installation Script for Windows

Write-Host "🔧 Installing QEMU for Windows..." -ForegroundColor Cyan

# QEMU download URL (official Windows build from Stefan Weil)
$qemuVersion = "20251224"
$qemuUrl = "https://qemu.weilnetz.de/w64/2024/qemu-w64-setup-$qemuVersion.exe"
$installerPath = "$env:TEMP\qemu-installer.exe"

Write-Host "📥 Downloading QEMU $qemuVersion..." -ForegroundColor Yellow
try {
    Invoke-WebRequest -Uri $qemuUrl -OutFile $installerPath -UseBasicParsing
    Write-Host "✅ Download complete!" -ForegroundColor Green
} catch {
    Write-Host "❌ Download failed: $_" -ForegroundColor Red
    Write-Host "" 
    Write-Host "📝 Manual Installation Instructions:" -ForegroundColor Yellow
    Write-Host "1. Visit: https://qemu.weilnetz.de/w64/" -ForegroundColor White
    Write-Host "2. Download the latest QEMU installer" -ForegroundColor White
    Write-Host "3. Run the installer with default settings" -ForegroundColor White
    Write-Host "4. Add QEMU to PATH: C:\Program Files\qemu" -ForegroundColor White
    exit 1
}

Write-Host ""
Write-Host "🚀 Running QEMU installer..." -ForegroundColor Yellow
Write-Host "   Please follow the installation wizard." -ForegroundColor Gray
Write-Host "   Default installation path: C:\Program Files\qemu" -ForegroundColor Gray

# Run installer
Start-Process -FilePath $installerPath -Wait

# Check if QEMU was installed
$qemuPath = "C:\Program Files\qemu"
if (Test-Path $qemuPath) {
    Write-Host "✅ QEMU installed successfully!" -ForegroundColor Green
    
    # Add to PATH if not already there
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$qemuPath*") {
        Write-Host "📝 Adding QEMU to PATH..." -ForegroundColor Yellow
        [Environment]::SetEnvironmentVariable(
            "Path",
            "$currentPath;$qemuPath",
            "User"
        )
        Write-Host "✅ PATH updated! Please restart your terminal." -ForegroundColor Green
    }
} else {
    Write-Host "⚠️  QEMU installation path not found." -ForegroundColor Yellow
    Write-Host "   Please verify installation manually." -ForegroundColor Gray
}

# Cleanup
Remove-Item $installerPath -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "🎉 Installation complete!" -ForegroundColor Cyan
Write-Host "   Verify with: qemu-system-riscv64 --version" -ForegroundColor Gray
