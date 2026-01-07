# Run ViOS in QEMU with Graphics
$qemu = "qemu-system-riscv64"
if (Get-Command $qemu -ErrorAction SilentlyContinue) {
    # QEMU in PATH
} elseif (Test-Path "C:\Program Files\qemu\qemu-system-riscv64.exe") {
    $qemu = "C:\Program Files\qemu\qemu-system-riscv64.exe"
} else {
    Write-Host "QEMU not found. Please install QEMU or add it to PATH."
    exit 1
}

$kernel = "target/riscv64gc-unknown-none-elf/debug/vios-kernel"
$disk = "disk.img"

# Build kernel
cargo build -p vios-kernel

Write-Host "Starting ViOS in Graphical Mode..."
& $qemu -machine virt -cpu rv64 -smp 1 -m 256M -serial stdio -bios default -kernel $kernel `
    -drive "file=$disk,format=raw,id=hd0,if=none" `
    -device virtio-blk-device,drive=hd0 `
    -device virtio-gpu-device `
    -device virtio-keyboard-device `
    -device virtio-mouse-device
