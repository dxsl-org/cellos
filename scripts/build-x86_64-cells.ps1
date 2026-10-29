# build-x86_64-cells.ps1 — Build x86_64 cells and create kernel_fs.img for x86_64
#
# Thin caller of scripts/assemble-x86-image.py --profile headless, the single
# source of the x86 image recipe: it builds every cell from source, signs them,
# checks the inputs and the read-back image, and publishes kernel_fs.img, init,
# boot-config/ and manifest.json atomically into -OutDir (default
# kernel/src/embedded-x86_64). Any build failure or missing artifact fails the
# run and leaves -OutDir as it was. This script only supplies the toolchain
# environment.
#
# Run from the ViCell root directory.

param(
    [string]$OutDir = "kernel/src/embedded-x86_64",
    # Optional 32-byte hex private signing seed, passed through (default: dev key).
    [string]$SeedFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# littlefs /data backend (service-vfs default feature): littlefs C core is
# cross-compiled with plain clang; third_party/freestanding-include supplies
# the libc declarations (implementations: compiler_builtins + POSIX shim).
# -mno-red-zone/-mno-sse/-mno-mmx match the Rust x86_64-unknown-none codegen.
$repoRoot = (Get-Location).Path
$includeDir = Join-Path $repoRoot 'third_party/freestanding-include'
if (-not $env:CC_x86_64_unknown_none) {
    $env:CC_x86_64_unknown_none = if ($IsWindows) {
        "C:\Program Files\LLVM\bin\clang.exe"
    } else {
        "clang"
    }
}
if (-not $env:CFLAGS_x86_64_unknown_none) {
    $env:CFLAGS_x86_64_unknown_none =
        "--target=x86_64-unknown-none-elf -ffreestanding -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$includeDir"
}
if (-not $env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none) {
    $hostInclude = if ($IsWindows) { "" } else { " -I/usr/include/x86_64-linux-gnu" }
    $env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none =
        "--target=x86_64-unknown-none-elf -I$includeDir$hostInclude"
}
if (-not $env:LIBCLANG_PATH) {
    $vsLlvm = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin"
    if (Test-Path "$vsLlvm/libclang.dll") { $env:LIBCLANG_PATH = $vsLlvm }
}

Write-Host "=== Building x86_64 cells (release) ==="

$python = if (Get-Command python -ErrorAction SilentlyContinue) {
    "python"
} elseif (Get-Command python3 -ErrorAction SilentlyContinue) {
    "python3"
} else {
    throw "Python 3 is required to package the x86_64 VIFS1 image"
}

$objcopyBin = if (Get-Command llvm-objcopy -ErrorAction SilentlyContinue) { "llvm-objcopy" } else { "objcopy" }
$assembleArgs = @('--profile', 'headless', '--out-dir', $OutDir, '--objcopy', $objcopyBin)
if ($SeedFile) { $assembleArgs += @('--seed-file', $SeedFile) }
& $python (Join-Path 'scripts' 'assemble-x86-image.py') @assembleArgs
if ($LASTEXITCODE -ne 0) {
    throw "assemble-x86-image.py failed (exit $LASTEXITCODE); $OutDir is unchanged"
}

# The canonical kernel profile comes from the assembler's manifest (CANONICAL_KERNEL_FEATURES).
$kernelFeatures = ((Get-Content -Raw (Join-Path $OutDir 'manifest.json') | ConvertFrom-Json).kernel_features) -join ','
Write-Host ""
Write-Host "Done. Rebuild kernel to embed the new x86_64 cells:"
Write-Host "  cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc --features $kernelFeatures"
