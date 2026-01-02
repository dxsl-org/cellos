#!/usr/bin/env pwsh
# Build script with full error output

Write-Host "Building ViOS kernel..." -ForegroundColor Cyan

$ErrorActionPreference = "Continue"

cargo build --no-default-features --target riscv64gc-unknown-none-elf -p kernel 2&gt;&amp;1 | Tee-Object -FilePath "build_full_output.txt"

$exitCode = $LASTEXITCODE

if ($exitCode -eq 0) {
    Write-Host "`n✅ Build successful!" -ForegroundColor Green
} else {
    Write-Host "`n❌ Build failed with exit code: $exitCode" -ForegroundColor Red
    Write-Host "Full output saved to: build_full_output.txt" -ForegroundColor Yellow
    
    # Show last 50 lines
    Write-Host "`nLast 50 lines of output:" -ForegroundColor Yellow
    Get-Content "build_full_output.txt" -Tail 50
}

exit $exitCode
