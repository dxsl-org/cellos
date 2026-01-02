# Debug build script
Write-Host "Attempting to build kernel..." -ForegroundColor Cyan

# Run build and capture output
try {
    cargo build --no-default-features --target riscv64gc-unknown-none-elf -p kernel --bin kernel *>&1 | Tee-Object -FilePath "debug_build.txt"
    $exitCode = $LASTEXITCODE
} catch {
    Write-Host "Error running cargo: $_" -ForegroundColor Red
    exit 1
}

Write-Host "`n=== LAST 50 LINES OF OUTPUT ===" -ForegroundColor Yellow  
Get-Content "debug_build.txt" -Tail 50

if ($exitCode -ne 0) {
    Write-Host "`nBuild FAILED with exit code: $exitCode" -ForegroundColor Red
    exit $exitCode
} else {
    Write-Host "`nBuild SUCCEEDED!" -ForegroundColor Green
}
