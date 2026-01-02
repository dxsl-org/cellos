#!/usr/bin/env pwsh
# Build script for ViOS kernel

Write-Host "Building ViOS kernel..." -ForegroundColor Cyan

# Build the kernel
try {
    cargo build --no-default-features --target riscv64gc-unknown-none-elf -p kernel 2&gt;&1 | Tee-Object -FilePath "build.log"
    $buildSuccess = $LASTEXITCODE -eq 0
} catch {
    $buildSuccess = $false
}

# Display last 50 lines
Write-Host "`nLast 50 lines of build output:" -ForegroundColor Yellow
Get-Content "build.log" | Select-Object -Last 50

# Check if build succeeded
if ($buildSuccess) {
    Write-Host "`n✅ Build succeeded!" -ForegroundColor Green
    
    # Show binary info
    $kernel = "target/riscv64gc-unknown-none-elf/debug/kernel"
    if (Test-Path $kernel) {
        $size = (Get-Item $kernel).Length
        Write-Host "Kernel binary: $kernel ($size bytes)" -ForegroundColor Cyan
    }
} else {
    Write-Host "`n❌ Build failed! Check build.log for details" -ForegroundColor Red
    exit 1
}
