# setup-mlibc.ps1 — Build mlibc libc.a on Windows (no WSL2 required).
#
# What this script does:
#   1. Clones managarm/mlibc into third_party/mlibc-src/ (skips if already cloned)
#   2. Copies ViCell's sysdeps/vicell into the clone
#   3. Patches mlibc's meson.build to add the 'vicell' host_machine.system() branch
#   4. Runs meson setup + ninja to build libc.a for riscv64
#   5. Copies the resulting libc.a to third_party/mlibc/build/libc.a
#
# Requirements (all checked below):
#   - git
#   - meson  (pip install meson)
#   - ninja  (ships with WinLibs / install via winget install Ninja-build.Ninja)
#   - riscv-none-elf-gcc  (xpack at C:\RISCV\xpack-riscv-none-elf-gcc-15.2.0-1)
#
# aarch64 still requires WSL2 (no Windows aarch64-none-elf toolchain included).
# Run `bash scripts/build-mlibc.sh` in WSL2 to also produce build-aarch64/libc.a.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot   = Split-Path $PSScriptRoot -Parent
$MlibcSrc   = Join-Path $RepoRoot 'third_party\mlibc-src'
$OurSysdeps = Join-Path $RepoRoot 'third_party\mlibc\sysdeps\vicell'
$BuildOut   = Join-Path $RepoRoot 'third_party\mlibc\build'
$CrossFile  = Join-Path $RepoRoot 'scripts\mlibc-riscv64-windows.cross'
$MlibcRepo  = 'https://github.com/managarm/mlibc.git'

function Check-Command($cmd) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Error "Required tool not found: $cmd`nInstall it and re-run this script."
        exit 1
    }
    Write-Host "  ✓ $cmd found" -ForegroundColor Green
}

Write-Host "`n=== Checking prerequisites ===" -ForegroundColor Cyan
Check-Command git
Check-Command meson
Check-Command ninja
Check-Command riscv-none-elf-gcc

# ─── Step 1: mlibc source at the pin ─────────────────────────────────────────
# Same pin as scripts/build-mlibc.sh (see its header for why this commit).
$MlibcPin = 'ae1092eb211e42e80d2d1c73881c8c25fde80fe6'
Write-Host "`n=== Step 1: mlibc source @ $MlibcPin ===" -ForegroundColor Cyan
if (-not (Test-Path (Join-Path $MlibcSrc '.git'))) {
    git init -q $MlibcSrc
    git -C $MlibcSrc remote add origin $MlibcRepo
}
git -C $MlibcSrc cat-file -e "$MlibcPin^{commit}" 2>$null
if ($LASTEXITCODE -ne 0) {
    git -C $MlibcSrc fetch --depth 1 origin $MlibcPin
    if ($LASTEXITCODE -ne 0) { Write-Error "fetch of $MlibcPin failed"; exit 1 }
}
git -C $MlibcSrc checkout -q -f $MlibcPin
# Drops the previous patch, sysdeps copy, builds and wrap checkouts.
git -C $MlibcSrc clean -q -ffdx -e subprojects/packagecache

$mlibcSha = git -C $MlibcSrc rev-parse HEAD
Write-Host "  mlibc commit: $mlibcSha" -ForegroundColor DarkGray

# ─── Step 2: Install ViCell sysdeps ───────────────────────────────────────────
Write-Host "`n=== Step 2: Installing ViCell sysdeps ===" -ForegroundColor Cyan
$destSysdeps = Join-Path $MlibcSrc 'sysdeps\vicell'
if (-not (Test-Path $destSysdeps)) {
    New-Item -ItemType Directory -Force $destSysdeps | Out-Null
}
Copy-Item -Recurse -Force "$OurSysdeps\*" $destSysdeps
Write-Host "  Copied sysdeps/vicell → $destSysdeps" -ForegroundColor Green
# Cellos patches to mlibc itself (same set as scripts/build-mlibc.sh).
Get-ChildItem (Join-Path $RepoRoot 'third_party\mlibc\patches\*.patch') | Sort-Object Name | ForEach-Object {
    git -C $MlibcSrc apply $_.FullName
    if ($LASTEXITCODE -ne 0) { Write-Error "git apply failed: $($_.Name)"; exit 1 }
    Write-Host "  applied $($_.Name)" -ForegroundColor Green
}

# ─── Step 3: Patch mlibc's meson.build ────────────────────────────────────────
Write-Host "`n=== Step 3: Patching meson.build ===" -ForegroundColor Cyan
$mesonBuild = Join-Path $MlibcSrc 'meson.build'
$content    = Get-Content $mesonBuild -Raw

$patch = @"

elif host_machine.system() == 'vicell'
    subdir('sysdeps/vicell')
"@

# Insert the vicell branch before the dispatch chain's final `else error(...)`.
$content = $content -replace "(\r?\n\s*else\s*\r?\n\s*error\('No sysdeps defined)", "$patch`$1"
if ($content -notmatch "host_machine\.system\(\) == 'vicell'") {
    Write-Error "no error('No sysdeps defined ...') block; insert the vicell branch by hand"
    exit 1
}
Set-Content -Path $mesonBuild -Value $content -NoNewline
Write-Host "  meson.build patched" -ForegroundColor Green

# ─── Step 4: meson setup ──────────────────────────────────────────────────────
Write-Host "`n=== Step 4: meson setup (riscv64) ===" -ForegroundColor Cyan
$buildDir = Join-Path $MlibcSrc 'build-win'

$mesonArgs = @(
    'setup', $buildDir, $MlibcSrc,
    "--cross-file=$CrossFile",
    '-Ddefault_library=static',
    '-Dposix_option=enabled',
    '-Dlinux_option=disabled',
    '-Dheaders_only=false',
    '--wipe'
)

Write-Host "  meson $($mesonArgs -join ' ')"
& meson @mesonArgs
if ($LASTEXITCODE -ne 0) { Write-Error "meson setup failed"; exit 1 }

# ─── Step 5: ninja build ──────────────────────────────────────────────────────
Write-Host "`n=== Step 5: ninja build ===" -ForegroundColor Cyan
& ninja -C $buildDir
if ($LASTEXITCODE -ne 0) { Write-Error "ninja build failed"; exit 1 }

# ─── Step 6: copy libc.a to expected location ────────────────────────────────
Write-Host "`n=== Step 6: Installing libc.a ===" -ForegroundColor Cyan
New-Item -ItemType Directory -Force $BuildOut | Out-Null
$srcLib  = Join-Path $buildDir 'libc.a'
$destLib = Join-Path $BuildOut 'libc.a'
Copy-Item -Force $srcLib $destLib
# libgcc helpers of the compiler that built libc.a; libs/mlibc-shim links them
# before Rust's compiler_builtins (see scripts/build-mlibc.sh).
$libgcc = & riscv-none-elf-gcc -march=rv64gc -mabi=lp64d -print-libgcc-file-name
Copy-Item -Force $libgcc (Join-Path $BuildOut 'libgcc.a')

$size = (Get-Item $destLib).Length / 1KB
Write-Host "  third_party/mlibc/build/libc.a  ($([int]$size) KB)" -ForegroundColor Green

Write-Host "`n✅ mlibc riscv64 build complete!" -ForegroundColor Green
Write-Host "   Run 'cargo check' — mlibc-shim warning should be gone."
Write-Host "   For aarch64: run 'bash scripts/build-mlibc.sh' in WSL2."
