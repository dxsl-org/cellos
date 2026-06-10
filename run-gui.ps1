# Run ViCell in QEMU with graphical display (VirtIO GPU → compositor → ViUI).
$qemu = "qemu-system-riscv64"
if (Get-Command $qemu -ErrorAction SilentlyContinue) {
    # QEMU in PATH
} elseif (Test-Path "C:\Program Files\qemu\qemu-system-riscv64.exe") {
    $qemu = "C:\Program Files\qemu\qemu-system-riscv64.exe"
} else {
    Write-Host "QEMU not found. Please install QEMU or add it to PATH."
    exit 1
}

$kernel = "target/riscv64gc-unknown-none-elf/release/vicell-kernel"
$disk   = "disk_v3.img"

Write-Host "Building release kernel..."
$env:RUSTFLAGS = "-C relocation-model=pic"
cargo build --release -p vicell-kernel
$env:RUSTFLAGS = $null
if (-not (Test-Path $kernel)) { Write-Host "Kernel build failed."; exit 1 }

Write-Host "Starting ViCell in Graphical Mode (compositor → VirtIO GPU)..."
Write-Host "Serial output on this terminal; graphical window opens separately."
Write-Host "IMPORTANT: Move the mouse cursor INTO the QEMU window — keyboard is grabbed automatically."
Write-Host "           Press Ctrl+Alt+G to release grab and return focus to the terminal."
Write-Host ""

& $qemu -machine virt -m 256M -bios default -kernel $kernel `
    -drive "file=$disk,format=raw,id=hd0,if=none" `
    -device virtio-blk-device,drive=hd0 `
    -netdev user,id=net0 `
    -device virtio-net-device,netdev=net0 `
    -device virtio-gpu-device `
    -device virtio-keyboard-device `
    -device virtio-mouse-device `
    -display gtk,grab-on-hover=on `
    -serial stdio
