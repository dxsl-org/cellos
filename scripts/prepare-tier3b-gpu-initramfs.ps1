param(
    [string]$CacheDir = ".alpine-cache",
    [string]$Output = ".alpine-cache/initramfs-tier3b-gpu"
)

$ErrorActionPreference = "Stop"
$target = "aarch64-unknown-none-softfloat"
$manifest = "tests/guests/tier3b-gpu-probe/Cargo.toml"
$probe = "tests/guests/tier3b-gpu-probe/target/$target/release/tier3b-gpu-probe"
$baseInitramfs = Join-Path $CacheDir "initramfs-virt"
$modloop = Join-Path $CacheDir "modloop-virt"

foreach ($required in @($baseInitramfs, $modloop)) {
    if (-not (Test-Path $required)) {
        throw "Missing $required. Run scripts/fetch-alpine-artifacts.sh first."
    }
}

cargo build --manifest-path $manifest --target $target --release
if ($LASTEXITCODE -ne 0) {
    throw "Tier3b guest probe build failed."
}

python tools/repack-initramfs.py $baseInitramfs $Output `
    --add bin/sh tests/guests/tier3b-gpu-probe/guest-init.sh 100755 `
    --add tier3b-gpu-probe $probe 100755 `
    --add modloop.squashfs $modloop 100644
if ($LASTEXITCODE -ne 0) {
    throw "Tier3b initramfs repack failed."
}

Write-Host "Tier3b test initramfs: $Output"
