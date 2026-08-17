# run-hypervisor-arm.ps1 — Boot ViCell on QEMU ARM virt (aarch64) with EL2 hypervisor.
#
# Boots the ViCell kernel at EL2, which then runs the hypervisor cell to launch
# an Alpine Linux guest via Stage-2 MMU (Tier 3b — ARM64 EL2 VMM).
#
# Prerequisites:
#   1. qemu-system-aarch64 >= 8.0 (for cortex-a72 + virtualization=on support).
#      Install via winget: winget install QEMU.QEMU
#      Or QEMU installer: https://www.qemu.org/download/
#   2. Build the aarch64 kernel WITH the Alpine guest image embedded (see below):
#        bash scripts/make-hypervisor-fs.sh --gpu-test
#        $env:RUSTFLAGS = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
#        $env:EMBEDDED_OVERRIDE = "kernel\src\embedded-hv"
#        cargo build --release -p cellos-kernel --features qemu-virt-1g `
#          --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
#        $env:RUSTFLAGS  = $null
#        $env:EMBEDDED_OVERRIDE = $null
#   3. Build the hypervisor disk image:
#        bash .\scripts\format-disk-hv-arm.sh
#      GUI variant:
#        bash .\scripts\format-disk-hv-arm.sh --gui disk_hv_arm_gui.img
#
# KVM acceleration (real ARM64 host only — e.g. RK3588, Raspberry Pi 5):
#   Add -enable-kvm to the qemu args below for near-native guest performance.
#   CI runners are x86 and use TCG (software emulation); KVM is not available there.
#
# Guest networking:
#   Alpine gets 10.0.2.15 via SLIRP DHCP through the Net Cell.
#   The guest can reach the internet via SLIRP user-mode networking.
#   Port 2222 on the host is forwarded to port 22 in the guest for SSH.

param(
    [switch]$Gui
)

$qemu = "qemu-system-aarch64"
if (-not (Get-Command $qemu -ErrorAction SilentlyContinue)) {
    if (Test-Path "C:\Program Files\qemu\qemu-system-aarch64.exe") {
        $qemu = "C:\Program Files\qemu\qemu-system-aarch64.exe"
    } else {
        Write-Host "qemu-system-aarch64 not found. Install QEMU and add it to PATH."
        exit 1
    }
}

$target  = "aarch64-unknown-none-softfloat"
$kernel  = "target/$target/release/cellos-kernel"
$disk    = if ($Gui) { "disk_hv_arm_gui.img" } else { "disk_hv_arm.img" }

if (-not (Test-Path $kernel)) {
    Write-Host "Hypervisor kernel not found: $kernel"
    Write-Host "Build it with:"
    Write-Host "  bash scripts/make-hypervisor-fs.sh --gpu-test"
    Write-Host "  `$env:RUSTFLAGS = '-C relocation-model=pic -C target-feature=+bti,+paca,+pacg'"
    Write-Host "  `$env:EMBEDDED_OVERRIDE = 'kernel\src\embedded-hv'"
    Write-Host "  cargo build --release -p cellos-kernel --features qemu-virt-1g --target $target -Z build-std=core,alloc"
    Write-Host "  `$env:RUSTFLAGS = `$null; `$env:EMBEDDED_OVERRIDE = `$null"
    exit 1
}

if (-not (Test-Path $disk)) {
    Write-Host "Hypervisor disk image not found: $disk"
    if ($Gui) {
        Write-Host "Build it with: bash .\scripts\format-disk-hv-arm.sh --gui disk_hv_arm_gui.img"
    } else {
        Write-Host "Build it with: bash .\scripts\format-disk-hv-arm.sh"
    }
    exit 1
}

Write-Host ""
Write-Host "Starting ViCell hypervisor on QEMU ARM virt (aarch64 EL2)..."
Write-Host "  Machine:  virt,virtualization=on,gic-version=2"
Write-Host "  CPU:      cortex-a72 (ARMv8.0, EL2 capable)"
Write-Host "  RAM:      1 GiB total, including 128 MiB contiguous guest RAM"
Write-Host "  Guest:    Alpine Linux via Stage-2 MMU (Tier 3b VMM)"
Write-Host ""
Write-Host "Wait for 'ViCell >' shell, then Alpine boots automatically via /bin/hypervisor."
Write-Host "Inside Alpine guest, you should see '/ #' prompt after DHCP."
Write-Host "Press Ctrl-a x to quit QEMU."
if ($Gui) {
    Write-Host "GUI mode: host compositor output goes to a QEMU GTK window via virtio-gpu."
    Write-Host "Shell and kernel logs stay in this terminal via -serial stdio."
}
Write-Host ""

$qemuArgs = @(
    "-machine", "virt,virtualization=on,gic-version=2",
    "-cpu", "cortex-a72",
    "-m", "1G",
    "-kernel", $kernel,
    "-drive", "if=none,file=$disk,format=raw,id=hd0",
    "-device", "virtio-blk-device,drive=hd0",
    "-netdev", "user,id=net0,hostfwd=tcp::2222-:22",
    "-device", "virtio-net-device,netdev=net0",
    "-no-reboot"
)

if ($Gui) {
    $qemuArgs += @(
        "-device", "virtio-gpu-device",
        "-device", "virtio-keyboard-device",
        "-device", "virtio-mouse-device",
        "-display", "gtk",
        "-serial", "stdio"
    )
} else {
    $qemuArgs += "-nographic"
}

& $qemu @qemuArgs
