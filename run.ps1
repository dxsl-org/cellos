# Run ViCell in QEMU
#
# Usage:
#   ./run.ps1            # auto-detects stale cells → regenerates disk, then boots
#   ./run.ps1 -SkipDisk  # skip the staleness check (kernel-only fast path)
param([switch]$SkipDisk)

$qemu = "qemu-system-riscv64"
if (Get-Command $qemu -ErrorAction SilentlyContinue) {
    # QEMU in PATH
} elseif (Test-Path "C:\Program Files\qemu\qemu-system-riscv64.exe") {
    $qemu = "C:\Program Files\qemu\qemu-system-riscv64.exe"
} else {
    Write-Host "QEMU not found. Please install QEMU or add it to PATH."
    exit 1
}

# Release kernel now only 4.4 MB (kernel_fs.img embedded separately).
# 256 MB is sufficient: kernel(4.4MB) + heap(64MB) + cells + stacks.
$kernel = "target/riscv64gc-unknown-none-elf/release/vicell-kernel"
$disk   = "disk_v3.img"

# ── Build-skew guard ──────────────────────────────────────────────────────────
# This script rebuilds ONLY the kernel. Cell binaries live in disk_v3.img and
# in kernel_fs.img (embedded into the kernel), both produced by gen_disk.ps1.
# Editing a cell/lib and re-running run.ps1 therefore used to boot the OLD cell
# silently — the #1 "I changed the code but QEMU runs the old thing" footgun,
# and the source of build-skew phantom panics (new kernel + stale cells).
# Detect it: if any source under cells/ or libs/ is newer than disk_v3.img,
# run gen_disk.ps1 before booting. -SkipDisk bypasses when you know the change
# is kernel-only.
if (-not $SkipDisk) {
    $needDisk = -not (Test-Path $disk)
    if (-not $needDisk) {
        $diskTime = (Get-Item $disk).LastWriteTime
        $newer = Get-ChildItem cells, libs -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object {
                $_.FullName -notmatch '\\(target|zig-out|\.zig-cache)\\' -and
                $_.LastWriteTime -gt $diskTime
            } | Select-Object -First 1
        if ($newer) {
            Write-Host "Cell/lib sources changed since disk_v3.img was built (e.g. $($newer.FullName))." -ForegroundColor Yellow
            $needDisk = $true
        }
    } else {
        Write-Host "disk_v3.img missing." -ForegroundColor Yellow
    }
    if ($needDisk) {
        Write-Host "Running gen_disk.ps1 so QEMU boots the code you just edited (skip with -SkipDisk)..." -ForegroundColor Yellow
        & pwsh -File "$PSScriptRoot/gen_disk.ps1"
        if ($LASTEXITCODE -ne 0) { Write-Host "gen_disk.ps1 failed — not booting a stale image." -ForegroundColor Red; exit 1 }
    }
}

# Always rebuild the kernel (a stale binary silently masks build/boot breakage).
# PIC is scoped to THIS kernel build only via RUSTFLAGS — never via .cargo/config,
# which would also make Cells PIC and break their .data access (see .cargo/config.toml).
# build.rs supplies -pie + --no-dynamic-linker; relocation-model=pic makes rustc emit
# the matching R_RISCV_RELATIVE relocations the kernel self-applies at _start.
Write-Host "Building release kernel (RUSTFLAGS=relocation-model=pic)..."
$env:RUSTFLAGS = "-C relocation-model=pic"
cargo build --release -p vicell-kernel
$env:RUSTFLAGS = $null
if (-not (Test-Path $kernel)) { Write-Host "Kernel build failed."; exit 1 }

Write-Host "Starting ViCell in QEMU (Nographic Mode)..."
Write-Host "Tip: Press 'Ctrl-a' then 'x' to exit QEMU."
Write-Host "Boot: OpenSBI → kernel (4.4MB) → init → VFS → config → input → shell (ViCell>)"
Write-Host ""

# Full VirtIO hardware configuration:
#   virt-blk: disk_v3.img (bootstrap table with cell ELFs)
#   virt-net:  user-mode network, DHCP assigns 10.0.2.15 to ViCell
#   virt-gpu:  GPU framebuffer (no graphical display in -nographic mode, but compositor can use it)
#   virt-input: VirtIO keyboard for the input service (separate from UART)
#
# Note: -nographic sends serial/UART to stdin/stdout; VirtIO keyboard is for graphical mode.
# Full VirtIO hardware: block, NIC (DHCP → 10.0.2.15), keyboard, and GPU
# (framebuffer setup needs the 32 MB heap; it allocates a ~4 MB framebuffer).
# 256 MB RAM: the C runtimes (Lua/MicroPython) carry multi-MB BSS arenas;
# with 128 MB cumulative frame allocation reached the RAM ceiling and faulted.
& $qemu -machine virt -m 256M -nographic -bios default -kernel $kernel `
        -drive "file=$disk,format=raw,id=hd0,if=none" `
        -device virtio-blk-device,drive=hd0 `
        -netdev user,id=net0 `
        -device virtio-net-device,netdev=net0 `
        -object rng-builtin,id=rng0 `
        -device virtio-rng-device,rng=rng0 `
        -device virtio-keyboard-device `
        -device virtio-gpu-device
