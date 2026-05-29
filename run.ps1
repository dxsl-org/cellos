# Run ViOS in QEMU
$qemu = "qemu-system-riscv64"
if (Get-Command $qemu -ErrorAction SilentlyContinue) {
    # QEMU in PATH
} elseif (Test-Path "C:\Program Files\qemu\qemu-system-riscv64.exe") {
    $qemu = "C:\Program Files\qemu\qemu-system-riscv64.exe"
} else {
    Write-Host "QEMU not found. Please install QEMU or add it to PATH."
    exit 1
}

# Use release kernel for production boot (debug is too large for 128MB).
# Release kernel requires 512MB RAM to fit: ~52MB kernel + 64MB heap + cells + stacks.
$kernel = "target/riscv64gc-unknown-none-elf/release/vios-kernel"
$disk   = "disk_v3.img"

# Build release kernel if not present
if (-not (Test-Path $kernel)) {
    Write-Host "Release kernel not found — building..."
    cargo build --release -p vios-kernel
}

Write-Host "Starting ViOS in QEMU (Nographic Mode)..."
Write-Host "Tip: Press 'Ctrl-a' then 'x' to exit QEMU."
Write-Host "Boot sequence: OpenSBI → kernel → init → VFS → config → shell (ViOS>)"
Write-Host ""

# 512M: kernel(52MB) + heap(64MB) + cells + stacks fit comfortably.
# VirtIO block passes disk_v3.img which contains the cell bootstrap table.
& $qemu -machine virt -m 512M -nographic -bios default -kernel $kernel `
        -drive file=$disk,format=raw,id=hd0,if=none `
        -device virtio-blk-device,drive=hd0
