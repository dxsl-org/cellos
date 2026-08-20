# build-aarch64-cells.ps1 — Build aarch64 cells and create kernel_fs.img for aarch64
#
# Parallel to build-x86_64-cells.ps1. Builds the bootstrap cells for
# aarch64-unknown-none-softfloat and packages them into
# kernel/src/embedded-aarch64/kernel_fs.img (the VirtIO-virt RAM ramdisk the
# aarch64 kernel loads cells from). Also refreshes the separately-embedded init.
#
# Run from the Cellos root directory.

param(
    [switch]$BoardRpi3
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$pythonArgs = @()
$python = if ($IsWindows -and (Get-Command py -ErrorAction SilentlyContinue)) {
    $pythonArgs = @('-3')
    'py'
} elseif (Get-Command python3 -ErrorAction SilentlyContinue) {
    'python3'
} elseif (Get-Command python -ErrorAction SilentlyContinue) {
    'python'
} else {
    throw 'Python 3 is required to build the embedded image'
}

$target   = "aarch64-unknown-none-softfloat"
$buildDir = Join-Path 'target' $target 'release'
$rpi3TargetDir = Join-Path 'target' 'rpi3-cells'
$rpi3BuildDir = Join-Path $rpi3TargetDir $target 'release'
$embeddedDir = if ($BoardRpi3) {
    Join-Path 'target' 'rpi3-embedded'
} else {
    Join-Path 'kernel' 'src' 'embedded-aarch64'
}

function Assert-CellBuild([string]$Name, [int]$ExitCode) {
    if ($ExitCode -eq 0) { return }
    if ($BoardRpi3) {
        throw "$Name build failed (exit $ExitCode); refusing to package stale artifacts"
    }
    Write-Warning "$Name build failed (exit $ExitCode)"
}
# pic: kernel/cell self-relocation. +bti,+paca,+pacg: BTI landing pads + PAC
# return-address signing (must match the kernel's aarch64 codegen features).
$rustflags = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"

# littlefs /data backend (service-vfs default feature): the littlefs C core is
# cross-compiled with plain clang — no bare-metal gcc needed. clang lacks libc
# headers for *-none targets, so third_party/freestanding-include supplies the
# declarations (implementations come from compiler_builtins + the POSIX shim).
# bindgen needs its OWN --target override: the Rust triple's "softfloat"
# component is not a valid clang triple.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).ProviderPath
if (-not $env:CC_aarch64_unknown_none_softfloat) {
    $env:CC_aarch64_unknown_none_softfloat = if ($IsLinux) {
        'clang'
    } else {
        'C:\Program Files\LLVM\bin\clang.exe'
    }
}
if (-not $env:CFLAGS_aarch64_unknown_none_softfloat) {
    $freestandingInclude = Join-Path $repoRoot 'third_party' 'freestanding-include'
    $env:CFLAGS_aarch64_unknown_none_softfloat =
        "--target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$freestandingInclude"
}
if (-not $env:BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat) {
    $env:BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat = if ($IsLinux) {
        '--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu'
    } else {
        "--target=aarch64-unknown-none-elf -I$repoRoot\third_party\freestanding-include"
    }
}
if (-not $env:LIBCLANG_PATH) {
    $vsLlvm = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin"
    if (Test-Path "$vsLlvm/libclang.dll") { $env:LIBCLANG_PATH = $vsLlvm }
}

Write-Host "=== Building aarch64 cells (release) ==="
$previousRustflags = $env:RUSTFLAGS
try {
$env:RUSTFLAGS = $rustflags

Write-Host "Building app-shell..."
cargo build --release -p app-shell --target $target 2>&1 | Select-Object -Last 8
Assert-CellBuild 'app-shell' $LASTEXITCODE

Write-Host "Building service-vfs (littlefs /data via clang cross-compile)..."
cargo build --release -p service-vfs --target $target 2>&1 | Select-Object -Last 8
Assert-CellBuild 'service-vfs' $LASTEXITCODE

Write-Host "Building service-config..."
cargo build --release -p service-config --target $target 2>&1 | Select-Object -Last 8
Assert-CellBuild 'service-config' $LASTEXITCODE

Write-Host "Building app-sys-tools (ls/cat/echo/ps/kill)..."
cargo build --release -p app-sys-tools --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'app-sys-tools' $LASTEXITCODE

Write-Host "Building service-input (UART EV_ASCII relay consumer)..."
if ($BoardRpi3) {
    $rpi3Input = Join-Path $rpi3BuildDir 'service-input'
    Remove-Item -LiteralPath $rpi3Input -Force -ErrorAction SilentlyContinue
    cargo build --release -p service-input --no-default-features --target $target `
        --target-dir $rpi3TargetDir 2>&1 | Select-Object -Last 5
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $rpi3Input)) {
        throw 'RPi3 service-input build failed; refusing to package a stale artifact'
    }
} else {
    cargo build --release -p service-input --target $target 2>&1 | Select-Object -Last 5
    Assert-CellBuild 'service-input' $LASTEXITCODE
}

Write-Host "Building input-test (aarch64_uart_input_delivery gate)..."
cargo build --release -p input-test --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'input-test' $LASTEXITCODE

Write-Host "Building periph-demo (aarch64_periph_demo_gpio gate)..."
cargo build --release -p periph-demo --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'periph-demo' $LASTEXITCODE

Write-Host "Building sensor-demo (RPi3 BCM BSC/GPIO gate)..."
cargo build --release -p sensor-demo --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'sensor-demo' $LASTEXITCODE

Write-Host "Building spi-demo (RPi3 BCM SPI0 gate)..."
cargo build --release -p spi-demo --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'spi-demo' $LASTEXITCODE

Write-Host "Building app-init..."
cargo build --release -p app-init --target $target 2>&1 | Select-Object -Last 5
Assert-CellBuild 'app-init' $LASTEXITCODE
} finally {
    $env:RUSTFLAGS = $previousRustflags
}

# Refresh the separately-embedded init ELF (kernel spawns it from embedded bytes).
$initSrc = Join-Path $buildDir 'app-init'
if (Test-Path $initSrc) {
    New-Item -ItemType Directory -Path $embeddedDir -Force | Out-Null
    Copy-Item $initSrc (Join-Path $embeddedDir 'init') -Force
    Write-Host "  Refreshed $embeddedDir\init"
}

$cells = @(
    @{ Bin = "app-shell";      Dst = "/bin/shell"       },
    @{ Bin = "service-vfs";    Dst = "/bin/vfs"         },
    @{ Bin = "service-config"; Dst = "/bin/config"      },
    @{ Bin = "service-input";  Dst = "/bin/input"       },
    @{ Bin = "input-test";     Dst = "/bin/input-test"  },
    @{ Bin = "periph-demo";    Dst = "/bin/periph-demo" },
    @{ Bin = "sensor-demo";    Dst = "/bin/sensor-demo" },
    @{ Bin = "spi-demo";       Dst = "/bin/spi-demo"    },
    @{ Bin = "ls";             Dst = "/bin/ls"          },
    @{ Bin = "cat";            Dst = "/bin/cat"         },
    @{ Bin = "echo";           Dst = "/bin/echo"        },
    @{ Bin = "ps";             Dst = "/bin/ps"          },
    @{ Bin = "kill";           Dst = "/bin/kill"        }
)

$imagePath = Join-Path $embeddedDir 'kernel_fs.img'
$imgArgs = @($imagePath)
$found   = @()
foreach ($c in $cells) {
    $src = if ($BoardRpi3 -and $c.Bin -eq 'service-input') {
        Join-Path $rpi3BuildDir $c.Bin
    } else {
        Join-Path $buildDir $c.Bin
    }
    if (Test-Path $src) {
        $kb = [Math]::Round((Get-Item $src).Length / 1KB, 0)
        Write-Host "  Found: $($c.Bin) (${kb} KB) -> $($c.Dst)"
        # mkfat32.py takes space-separated <src> <dst> pairs, NOT src:dst.
        $imgArgs += @($src, $c.Dst)
        $found += $c.Bin
    } else {
        Write-Warning "  Not found: $src (will be absent from kernel_fs.img)"
    }
}

foreach ($required in @('app-shell', 'service-vfs', 'service-config', 'service-input', 'periph-demo', 'sensor-demo', 'spi-demo')) {
    if ($required -notin $found) {
        throw "Required aarch64 cell missing from image inputs: $required"
    }
}

# Signed operator policy. Without /POLICY.BIN the kernel takes the `Absent` branch,
# which is dev-permissive: the whole policy layer runs and changes nothing, and no
# test can tell the difference. sign-policy.py round-trip-decodes the blob before
# writing, so an entry outside the kernel's domain masks fails here rather than
# becoming PolicyState::Invalid → DenyAll on a booted device.
#
# The blob is signed with the DEV fleet key and only verifies while the kernel carries
# the default `dev-policy-key` feature.
# Repo-relative, not $env:TEMP — that variable is unset on Linux runners, where the
# path collapses to the filesystem root and the write is denied. Forward slashes so the
# same string works under PowerShell on both platforms.
$policyTmp = "target/ViCell_aarch64_POLICY.BIN"
New-Item -ItemType Directory -Force (Split-Path $policyTmp) | Out-Null
& $python @pythonArgs (Join-Path 'scripts' 'sign-policy.py') --out $policyTmp
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $policyTmp)) {
    Write-Error "sign-policy.py failed — need 'pip install cryptography'."
    exit 1
}
# Root-level, 8.3-uppercase: kernel/src/policy.rs reads exactly /POLICY.BIN.
$imgArgs += @($policyTmp, "/POLICY.BIN")

Write-Host ""
Write-Host "=== Creating aarch64 kernel_fs.img ==="
& $python @pythonArgs (Join-Path 'tools' 'mkfat32.py') @imgArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "mkfat32.py failed (exit $LASTEXITCODE)"
    exit 1
}
Remove-Item $policyTmp -Force -ErrorAction SilentlyContinue
# Assert the layout instead of trusting the exit code: mkfat32 exits 0 for images
# whose destination paths were mangled, and a missing /POLICY.BIN degrades silently.
$layout = & $python @pythonArgs (Join-Path 'tools' 'inspect_fat.py') $imagePath 2>&1
foreach ($requiredMarker in @('SFN POLICY.BIN', "LFN 'vfs'", "LFN 'input'", "LFN 'periph-demo'", "LFN 'sensor-demo'", "LFN 'spi-demo'")) {
    if (($layout | Select-String -Quiet -SimpleMatch $requiredMarker) -eq $false) {
        Write-Error "aarch64 kernel_fs.img missing required entry: $requiredMarker"
        $layout | Write-Host
        exit 1
    }
}
$kb = [Math]::Round((Get-Item $imagePath).Length / 1KB, 0)
Write-Host "  kernel_fs.img created: ${kb} KB"

Write-Host ""
Write-Host "Done. Rebuild the aarch64 kernel to embed the new cells:"
Write-Host "  `$env:RUSTFLAGS = '-C relocation-model=pic -C target-feature=+bti,+paca,+pacg'"
if ($BoardRpi3) {
    Write-Host "  `$env:EMBEDDED_OVERRIDE = 'target/rpi3-embedded'"
    Write-Host "  cargo build --release -p cellos-kernel --features board-rpi3 --target aarch64-unknown-none-softfloat"
    Write-Host "  `$env:EMBEDDED_OVERRIDE = `$null"
} else {
    Write-Host "  cargo build --release -p cellos-kernel --target aarch64-unknown-none-softfloat"
}
Write-Host "  `$env:RUSTFLAGS = `$null"
Write-Host "  .\run-arm-virt.ps1   (or the aarch64-boot integration suite)"
