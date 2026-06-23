# run-cluster-arm64.ps1 — Boot a 2-node Cellos ARM64 cluster on localhost.
#
# Each node is a QEMU aarch64 virt machine; nodes are connected peer-to-peer
# via QEMU `-netdev socket` (loopback, no TAP required).
#
# Layout:
#   Node A (node-0):  serial → TCP :18180   socket listens on :19000
#   Node B (node-1):  serial → TCP :18181   socket connects to :19000
#
# The beacon group 239.0.0.1:9087 travels over the same VirtIO NIC, but
# QEMU SLIRP does NOT bridge multicast between two user-mode netdevs — you
# need the socket netdev for real UDP relay.  This script uses `-netdev socket`
# so beacon frames are visible on both nodes.
#
# Prerequisites:
#   1. qemu-system-aarch64 on PATH (or set $env:ViCell_QEMU_AARCH64).
#   2. Build the aarch64 kernel:
#        $env:RUSTFLAGS = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
#        cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat
#        $env:RUSTFLAGS = $null
#   3. Build and sign the net-broker cell:
#        cargo build --release -p service-net-broker --target aarch64-unknown-none-softfloat
#   4. Generate the disk image with net-broker included:
#        .\gen_disk.ps1
#   5. Optionally copy/rename the disk for node-1:
#        Copy-Item disk_arm_virt.img disk_arm_virt_n1.img
#
# Usage:
#   .\run-cluster-arm64.ps1                # default: disk_arm_virt.img for both nodes
#   .\run-cluster-arm64.ps1 -Node0Disk a.img -Node1Disk b.img

param(
    [string]$Node0Disk  = "disk_arm_virt.img",
    [string]$Node1Disk  = "disk_arm_virt.img",  # same disk is OK for boot tests
    [int]   $Node0Serial = 18180,
    [int]   $Node1Serial = 18181,
    [int]   $SocketPort  = 19000,    # node-0 listens, node-1 connects
    [switch]$NoWait                  # return immediately; caller watches serial ports
)

# ── Resolve QEMU binary ──────────────────────────────────────────────────────

$qemu = $env:ViCell_QEMU_AARCH64
if (-not $qemu) { $qemu = "qemu-system-aarch64" }
if (-not (Get-Command $qemu -ErrorAction SilentlyContinue)) {
    $fallback = "C:\Program Files\qemu\qemu-system-aarch64.exe"
    if (Test-Path $fallback) { $qemu = $fallback }
    else {
        Write-Error "qemu-system-aarch64 not found. Install QEMU or set $env:ViCell_QEMU_AARCH64."
        exit 1
    }
}

$target = "aarch64-unknown-none-softfloat"
$kernel = "target/$target/release/vicell-kernel"

if (-not (Test-Path $kernel)) {
    Write-Error "Kernel $kernel not found. Build with:"
    Write-Error '  $env:RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"'
    Write-Error "  cargo build --release -p vicell-kernel --target $target"
    exit 1
}
if (-not (Test-Path $Node0Disk)) {
    Write-Error "Disk image '$Node0Disk' not found. Run .\gen_disk.ps1 first."
    exit 1
}
if (-not (Test-Path $Node1Disk)) {
    Write-Error "Disk image '$Node1Disk' not found."
    exit 1
}

# ── Common QEMU arguments ────────────────────────────────────────────────────

function New-NodeArgs([string]$disk, [string]$serial_port, [string]$netdev) {
    return @(
        "-machine", "virt,gic-version=2",
        "-cpu",     "cortex-a57",
        "-m",       "256M",
        "-nographic",
        "-kernel",  $kernel,
        # Disk (shared read-only root; each node has its own FAT partition copy).
        "-drive",   "if=none,file=$disk,format=raw,id=hd0",
        "-device",  "virtio-blk-device,drive=hd0",
        # NIC: socket-mode peer-to-peer for real UDP multicast between nodes.
        "-netdev",  $netdev,
        "-device",  "virtio-net-device,netdev=net0",
        # RNG: required for broker Noise entropy gate.
        "-object",  "rng-builtin,id=rng0",
        "-device",  "virtio-rng-device,rng=rng0",
        # No GPU/keyboard — headless cluster nodes.
        "-monitor", "none",
        # Serial → TCP so both nodes can be watched simultaneously.
        "-serial",  "tcp:127.0.0.1:${serial_port},server,nowait",
        "-no-reboot"
    )
}

$node0netdev = "socket,id=net0,listen=:${SocketPort}"
$node1netdev = "socket,id=net0,connect=127.0.0.1:${SocketPort}"

# ── Launch nodes ─────────────────────────────────────────────────────────────

Write-Host "Cellos 2-node ARM64 cluster"
Write-Host "  Node-0 serial: telnet 127.0.0.1 $Node0Serial"
Write-Host "  Node-1 serial: telnet 127.0.0.1 $Node1Serial"
Write-Host "  Peer link:     QEMU socket 127.0.0.1:$SocketPort"
Write-Host ""
Write-Host "Starting node-0 (socket listener)..."

$args0 = New-NodeArgs $Node0Disk $Node0Serial $node0netdev
$proc0 = Start-Process -FilePath $qemu -ArgumentList $args0 -PassThru -WindowStyle Hidden

# Give node-0 a moment to bind the socket port before node-1 tries to connect.
Start-Sleep -Milliseconds 500

Write-Host "Starting node-1 (socket connector)..."
$args1 = New-NodeArgs $Node1Disk $Node1Serial $node1netdev
$proc1 = Start-Process -FilePath $qemu -ArgumentList $args1 -PassThru -WindowStyle Hidden

Write-Host ""
Write-Host "Both nodes running. Connect via:"
Write-Host "  Node-0: nc 127.0.0.1 $Node0Serial  (or telnet)"
Write-Host "  Node-1: nc 127.0.0.1 $Node1Serial"
Write-Host ""
Write-Host "Press Ctrl-C to kill both nodes."

if ($NoWait) {
    return @{ Node0Pid = $proc0.Id; Node1Pid = $proc1.Id }
}

try {
    while ($true) { Start-Sleep -Seconds 1 }
} finally {
    Write-Host "`nStopping both nodes..."
    $proc0 | Stop-Process -Force -ErrorAction SilentlyContinue
    $proc1 | Stop-Process -Force -ErrorAction SilentlyContinue
}
