# build-aarch64-cells.ps1 — Build aarch64 cells and create kernel_fs.img for aarch64
#
# Parallel to build-x86_64-cells.ps1. Builds the bootstrap cells for
# aarch64-unknown-none-softfloat and packages them into
# kernel/src/embedded-aarch64/kernel_fs.img (the VirtIO-virt RAM ramdisk the
# aarch64 kernel loads cells from). Also refreshes the separately-embedded init.
#
# service-vfs is built with --no-default-features: the `littlefs` /data backend
# needs a bare-metal cross-C toolchain (only riscv has one wired), so aarch64
# omits it — the persistent /data volume is simply absent, not needed for boot.
#
# Run from the Cellos root directory.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$target   = "aarch64-unknown-none-softfloat"
$buildDir = "target\$target\release"
# pic: kernel/cell self-relocation. +bti,+paca,+pacg: BTI landing pads + PAC
# return-address signing (must match the kernel's aarch64 codegen features).
$rustflags = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"

Write-Host "=== Building aarch64 cells (release) ==="
$env:RUSTFLAGS = $rustflags

Write-Host "Building app-shell..."
cargo build --release -p app-shell --target $target 2>&1 | Select-Object -Last 8
if ($LASTEXITCODE -ne 0) { Write-Warning "app-shell build failed (exit $LASTEXITCODE)" }

Write-Host "Building service-vfs (--no-default-features: no littlefs/C)..."
cargo build --release -p service-vfs --target $target --no-default-features 2>&1 | Select-Object -Last 8
if ($LASTEXITCODE -ne 0) { Write-Warning "service-vfs build failed (exit $LASTEXITCODE)" }

Write-Host "Building service-config..."
cargo build --release -p service-config --target $target 2>&1 | Select-Object -Last 8
if ($LASTEXITCODE -ne 0) { Write-Warning "service-config build failed (exit $LASTEXITCODE)" }

Write-Host "Building app-sys-tools (ls/cat/echo/ps/kill)..."
cargo build --release -p app-sys-tools --target $target 2>&1 | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "app-sys-tools build failed" }

Write-Host "Building service-input (UART EV_ASCII relay consumer)..."
cargo build --release -p service-input --target $target 2>&1 | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "service-input build failed" }

Write-Host "Building input-test (aarch64_uart_input_delivery gate)..."
cargo build --release -p input-test --target $target 2>&1 | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "input-test build failed" }

Write-Host "Building periph-demo (aarch64_periph_demo_gpio gate)..."
cargo build --release -p periph-demo --target $target 2>&1 | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "periph-demo build failed" }

Write-Host "Building app-init..."
cargo build --release -p app-init --target $target 2>&1 | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "app-init build failed" }
$env:RUSTFLAGS = ""

# Refresh the separately-embedded init ELF (kernel spawns it from embedded bytes).
$initSrc = "$buildDir\app-init"
if (Test-Path $initSrc) {
    Copy-Item $initSrc "kernel\src\embedded-aarch64\init" -Force
    Write-Host "  Refreshed kernel\src\embedded-aarch64\init"
}

$cells = @(
    @{ Bin = "app-shell";      Dst = "/bin/shell"       },
    @{ Bin = "service-vfs";    Dst = "/bin/vfs"         },
    @{ Bin = "service-config"; Dst = "/bin/config"      },
    @{ Bin = "service-input";  Dst = "/bin/input"       },
    @{ Bin = "input-test";     Dst = "/bin/input-test"  },
    @{ Bin = "periph-demo";    Dst = "/bin/periph-demo" },
    @{ Bin = "ls";             Dst = "/bin/ls"          },
    @{ Bin = "cat";            Dst = "/bin/cat"         },
    @{ Bin = "echo";           Dst = "/bin/echo"        },
    @{ Bin = "ps";             Dst = "/bin/ps"          },
    @{ Bin = "kill";           Dst = "/bin/kill"        }
)

$imgArgs = @("kernel\src\embedded-aarch64\kernel_fs.img")
$found   = @()
foreach ($c in $cells) {
    $src = "$buildDir\$($c.Bin)"
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

if ($found.Count -eq 0) {
    Write-Error "No aarch64 cell binaries built — kernel_fs.img not updated."
    exit 1
}

Write-Host ""
Write-Host "=== Creating aarch64 kernel_fs.img ==="
python tools\mkfat32.py @imgArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "mkfat32.py failed (exit $LASTEXITCODE)"
    exit 1
}
$kb = [Math]::Round((Get-Item "kernel\src\embedded-aarch64\kernel_fs.img").Length / 1KB, 0)
Write-Host "  kernel_fs.img created: ${kb} KB"

Write-Host ""
Write-Host "Done. Rebuild the aarch64 kernel to embed the new cells:"
Write-Host "  `$env:RUSTFLAGS = '-C relocation-model=pic -C target-feature=+bti,+paca,+pacg'"
Write-Host "  cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat"
Write-Host "  `$env:RUSTFLAGS = `$null"
Write-Host "  .\run-arm-virt.ps1   (or the aarch64-boot integration suite)"
