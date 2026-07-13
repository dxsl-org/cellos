# build-x86_64-cells.ps1 — Build x86_64 cells and create kernel_fs.img for x86_64
#
# Builds app-shell and service-vfs (+ service-config) for x86_64-unknown-none,
# then packages them into kernel/src/embedded-x86_64/kernel_fs.img.
#
# Run from the ViCell root directory.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$target    = "x86_64-unknown-none"
$buildDir  = "target\$target\release"
$embedded  = "kernel\src\embedded-x86_64"
$buildStd  = "-Z build-std=core,alloc"
# Cells link at CELL_VA_START (0x1_0000_0000 = 4 GiB) since 2026-06-19; a small
# code-model with relocation-model=static cannot materialise that address
# (rust-lld: R_X86_64_32S against local symbol). Cells are PIC/PIE — matches the
# .cargo/config.toml note that cells need relocation-model=pie (kernel uses pic).
$rustflags = "-C relocation-model=pic"

# littlefs /data backend (service-vfs default feature): littlefs C core is
# cross-compiled with plain clang; third_party/freestanding-include supplies
# the libc declarations (implementations: compiler_builtins + POSIX shim).
# -mno-red-zone/-mno-sse/-mno-mmx match the Rust x86_64-unknown-none codegen.
$repoRoot = (Get-Location).Path
if (-not $env:CC_x86_64_unknown_none) {
    $env:CC_x86_64_unknown_none = "C:\Program Files\LLVM\bin\clang.exe"
}
if (-not $env:CFLAGS_x86_64_unknown_none) {
    $env:CFLAGS_x86_64_unknown_none =
        "--target=x86_64-unknown-none-elf -ffreestanding -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$repoRoot\third_party\freestanding-include"
}
if (-not $env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none) {
    $env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none =
        "--target=x86_64-unknown-none-elf -I$repoRoot\third_party\freestanding-include"
}
if (-not $env:LIBCLANG_PATH) {
    $vsLlvm = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin"
    if (Test-Path "$vsLlvm/libclang.dll") { $env:LIBCLANG_PATH = $vsLlvm }
}

Write-Host "=== Building x86_64 cells (release) ==="

$env:RUSTFLAGS = $rustflags

# Build shell
Write-Host "Building app-shell..."
$cmd = "cargo build --release -p app-shell --target $target $buildStd 2>&1"
Invoke-Expression $cmd | Select-Object -Last 10
if ($LASTEXITCODE -ne 0) { Write-Warning "app-shell build failed (exit $LASTEXITCODE)" }

# Build vfs
Write-Host "Building service-vfs..."
$cmd = "cargo build --release -p service-vfs --target $target $buildStd 2>&1"
Invoke-Expression $cmd | Select-Object -Last 10
if ($LASTEXITCODE -ne 0) { Write-Warning "service-vfs build failed (exit $LASTEXITCODE)" }

# Build config
Write-Host "Building service-config..."
$cmd = "cargo build --release -p service-config --target $target $buildStd 2>&1"
Invoke-Expression $cmd | Select-Object -Last 10
if ($LASTEXITCODE -ne 0) { Write-Warning "service-config build failed (exit $LASTEXITCODE)" }

# Build the PCIe cell stack (Kernel Boundary Law: drivers live in cells).
# platform = ECAM scanner (kernel spawns /bin/platform before init);
# nvme/e1000 = PCIe Driver Cells (init spawns them; PcieDriverCap is
# path-granted by the kernel loader). Each exits cleanly when its device
# is absent, so diskless/NIC-less boots are unaffected.
foreach ($pkg in "service-platform", "driver-nvme", "driver-e1000") {
    Write-Host "Building $pkg..."
    $cmd = "cargo build --release -p $pkg --target $target $buildStd 2>&1"
    Invoke-Expression $cmd | Select-Object -Last 10
    if ($LASTEXITCODE -ne 0) { Write-Warning "$pkg build failed (exit $LASTEXITCODE)" }
}

$env:RUSTFLAGS = ""

# Build sys-tools (ls/cat/echo/ps/kill — M3.2)
Write-Host "Building app-sys-tools (ls/cat/echo/ps/kill)..."
$env:RUSTFLAGS = $rustflags
$cmd = "cargo build --release -p app-sys-tools --target $target $buildStd 2>&1"
Invoke-Expression $cmd | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "app-sys-tools build failed" }
$env:RUSTFLAGS = ""

# Build init and refresh the separately-embedded init ELF (kernel spawns it from
# embedded bytes at boot). Rebuilding here keeps init in lockstep with the other
# cells' ostd (G2 spawn_from_path routing) instead of a stale committed binary.
Write-Host "Building app-init..."
$env:RUSTFLAGS = $rustflags
$cmd = "cargo build --release -p app-init --target $target $buildStd 2>&1"
Invoke-Expression $cmd | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Write-Warning "app-init build failed" }
$env:RUSTFLAGS = ""
$initSrc = "$buildDir\app-init"
if (Test-Path $initSrc) {
    Copy-Item $initSrc "kernel\src\embedded-x86_64\init" -Force
    Write-Host "  Refreshed kernel\src\embedded-x86_64\init"
}

# Collect available binaries
$cells = @(
    @{ Bin = "app-shell";      Dst = "/bin/shell"  },
    @{ Bin = "service-vfs";    Dst = "/bin/vfs"    },
    @{ Bin = "service-config"; Dst = "/bin/config" },
    @{ Bin = "platform";       Dst = "/bin/platform" },
    @{ Bin = "driver-nvme";    Dst = "/bin/nvme"   },
    @{ Bin = "driver-e1000";   Dst = "/bin/e1000"  },
    @{ Bin = "ls";             Dst = "/bin/ls"     },
    @{ Bin = "cat";            Dst = "/bin/cat"    },
    @{ Bin = "echo";           Dst = "/bin/echo"   },
    @{ Bin = "ps";             Dst = "/bin/ps"     },
    @{ Bin = "kill";           Dst = "/bin/kill"   }
)

$imgArgs = @("kernel\src\embedded-x86_64\kernel_fs.img")
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
    Write-Error "No x86_64 cell binaries built — kernel_fs.img not updated."
    exit 1
}

Write-Host ""
Write-Host "=== Creating x86_64 kernel_fs.img ==="
python tools\mkfat32.py @imgArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "mkfat32.py failed (exit $LASTEXITCODE)"
    exit 1
}
$kb = [Math]::Round((Get-Item "kernel\src\embedded-x86_64\kernel_fs.img").Length / 1KB, 0)
Write-Host "  kernel_fs.img created: ${kb} KB"

Write-Host ""
Write-Host "Done. Rebuild kernel to embed the new x86_64 cells:"
Write-Host "  cargo build --release -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc"
