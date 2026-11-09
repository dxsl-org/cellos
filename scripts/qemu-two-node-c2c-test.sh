#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Two-node x86_64 C2C cluster test under QEMU KVM/TCG.
# Boots Node A and Node B interconnected by a direct Layer-2 virtual Ethernet socket.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ISO="${1:-build/cellos-x86_64.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-35}"
X86_ACCEL="${X86_ACCEL:-kvm}"
X86_CPU_MODEL="${X86_CPU_MODEL:-host}"
SMP="${SMP:-1}"
SOCKET_PORT="${C2C_SOCKET_PORT:-23456}"
C2C_TRANSPORT="${C2C_TRANSPORT:-mcast}"
MCAST_GROUP="${C2C_MCAST_GROUP:-239.0.0.1}"

if [[ "$C2C_TRANSPORT" == "mcast" ]]; then
    NETDEV_A="socket,id=c2clan,mcast=$MCAST_GROUP:$SOCKET_PORT"
    NETDEV_B="socket,id=c2clan,mcast=$MCAST_GROUP:$SOCKET_PORT"
    DESC_A="Multicast Peer on $MCAST_GROUP:$SOCKET_PORT"
    DESC_B="Multicast Peer on $MCAST_GROUP:$SOCKET_PORT"
    LAUNCH_DELAY=0
else
    NETDEV_A="socket,id=c2clan,listen=127.0.0.1:$SOCKET_PORT"
    NETDEV_B="socket,id=c2clan,connect=127.0.0.1:$SOCKET_PORT"
    DESC_A="L2 Listener on :$SOCKET_PORT"
    DESC_B="L2 Connector to :$SOCKET_PORT"
    LAUNCH_DELAY=1.5
fi
WORK_DIR="${C2C_WORK_DIR:-build/two-node-c2c}"

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: ISO not found: $ISO" >&2
    echo "Build with: bash scripts/build-cloud-images.sh" >&2
    exit 1
fi

mkdir -p "$WORK_DIR"
NODE_A_LOG="$WORK_DIR/node-a.log"
NODE_B_LOG="$WORK_DIR/node-b.log"
rm -f "$NODE_A_LOG" "$NODE_B_LOG"

ACCEL_ARGS=()
if [[ -n "$X86_ACCEL" ]]; then
    ACCEL_ARGS=(-accel "$X86_ACCEL")
fi

echo "=========================================================================="
echo "  Cellos Two-Node Virtual Cluster C2C Gate"
echo "  ISO:         $ISO"
echo "  Accel:       $X86_ACCEL (CPU: $X86_CPU_MODEL, SMP: ${SMP}c/node)"
echo "  Transport:   $C2C_TRANSPORT ($DESC_A)"
echo "  Window:      ${BOOT_WINDOW}s"
echo "=========================================================================="

# Cleanup handler on exit/interrupt
NODE_A_PID=""
NODE_B_PID=""
FIFO_A="$WORK_DIR/node-a.in"
FIFO_B="$WORK_DIR/node-b.in"
rm -f "$FIFO_A" "$FIFO_B"
mkfifo "$FIFO_A" "$FIFO_B"
exec 3<>"$FIFO_A"
exec 4<>"$FIFO_B"

cleanup() {
    if [[ -n "$NODE_A_PID" ]] && kill -0 "$NODE_A_PID" 2>/dev/null; then
        kill "$NODE_A_PID" 2>/dev/null || true
    fi
    if [[ -n "$NODE_B_PID" ]] && kill -0 "$NODE_B_PID" 2>/dev/null; then
        kill "$NODE_B_PID" 2>/dev/null || true
    fi
    exec 3>&- || true
    exec 4>&- || true
    rm -f "$FIFO_A" "$FIFO_B"
}
trap cleanup EXIT INT TERM
# ── 1. Launch Node A (Server / Socket Listener) ──────────────────────────────
echo "[c2c-cluster] Launching Node A ($DESC_A, MAC 52:54:00:c2:c0:01)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -smp "$SMP" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "$NETDEV_A" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:01" \
    < "$FIFO_A" > "$NODE_A_LOG" 2>&1 &
NODE_A_PID=$!

if [[ "$LAUNCH_DELAY" != "0" ]]; then
    sleep "$LAUNCH_DELAY"
fi

# ── 2. Launch Node B (Client / Socket Connect) ───────────────────────────────
echo "[c2c-cluster] Launching Node B ($DESC_B, MAC 52:54:00:c2:c0:02)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -smp "$SMP" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "$NETDEV_B" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:02" \
    < "$FIFO_B" > "$NODE_B_LOG" 2>&1 &
NODE_B_PID=$!

# Background feeder: when both nodes reach prompt, run ifconfig and clean shutdown
(
    for _ in {1..40}; do
        if grep -qa "Cellos >" "$NODE_A_LOG" 2>/dev/null && grep -qa "Cellos >" "$NODE_B_LOG" 2>/dev/null; then
            break
        fi
        sleep 0.5
    done
    sleep 6
    echo "ifconfig" > "$FIFO_A"
    echo "ifconfig" > "$FIFO_B"
    sleep 2
    echo "/bin/shutdown" > "$FIFO_A"
    echo "/bin/shutdown" > "$FIFO_B"
) &
FEEDER_PID=$!
# ── 3. Await Boot Completion ────────────────────────────────────────────────
echo "[c2c-cluster] Waiting for both nodes to reach shell prompt..."
wait "$NODE_A_PID" || true
wait "$NODE_B_PID" || true
NODE_A_PID=""
NODE_B_PID=""

# ── 4. Verify Oracles & Evidence ─────────────────────────────────────────────
FAIL=0

if ! grep -qa "Cellos >" "$NODE_A_LOG"; then
    echo "FAIL: Node A did not reach shell prompt" >&2
    FAIL=1
fi
if ! grep -qa "Cellos >" "$NODE_B_LOG"; then
    echo "FAIL: Node B did not reach shell prompt" >&2
    FAIL=1
fi

if ! grep -qa "mac=52:54:00:c2:c0:01" "$NODE_A_LOG"; then
    echo "FAIL: Node A did not bind MAC 52:54:00:c2:c0:01" >&2
    FAIL=1
fi
if ! grep -qa "mac=52:54:00:c2:c0:02" "$NODE_B_LOG"; then
    echo "FAIL: Node B did not bind MAC 52:54:00:c2:c0:02" >&2
    FAIL=1
fi
if ! grep -qa "169.254.192.1" "$NODE_A_LOG"; then
    echo "FAIL: Node A did not assign IPv4 Link-Local 169.254.192.1" >&2
    FAIL=1
fi
if ! grep -qa "169.254.192.2" "$NODE_B_LOG"; then
    echo "FAIL: Node B did not assign IPv4 Link-Local 169.254.192.2" >&2
    FAIL=1
fi
if ! grep -qa "eth0: inet 169.254.192.1  netmask 255.255.0.0" "$NODE_A_LOG"; then
    echo "FAIL: Node A user-space ifconfig did not report Link-Local address" >&2
    FAIL=1
fi
if ! grep -qa "eth0: inet 169.254.192.2  netmask 255.255.0.0" "$NODE_B_LOG"; then
    echo "FAIL: Node B user-space ifconfig did not report Link-Local address" >&2
    FAIL=1
fi
if ! grep -qa "first e1000 RX" "$NODE_A_LOG"; then
    echo "FAIL: Node A did not receive L2 Ethernet frames from cluster" >&2
    FAIL=1
fi
if ! grep -qa "first e1000 RX" "$NODE_B_LOG"; then
    echo "FAIL: Node B did not receive L2 Ethernet frames from cluster" >&2
    FAIL=1
fi
if (( SMP > 1 )); then
    if ! grep -qa "topology online=$SMP" "$NODE_A_LOG"; then
        echo "FAIL: Node A did not bring $SMP CPUs online" >&2
        FAIL=1
    fi
    if ! grep -qa "topology online=$SMP" "$NODE_B_LOG"; then
        echo "FAIL: Node B did not bring $SMP CPUs online" >&2
        FAIL=1
    fi
fi
if ! grep -qa "power: requesting poweroff" "$NODE_A_LOG"; then
    echo "FAIL: Node A did not execute clean ACPI poweroff" >&2
    FAIL=1
fi
if ! grep -qa "power: requesting poweroff" "$NODE_B_LOG"; then
    echo "FAIL: Node B did not execute clean ACPI poweroff" >&2
    FAIL=1
fi

if grep -qia "KERNEL PANIC\|\[fault\] Cell" "$NODE_A_LOG"; then
    echo "FAIL: Node A reported kernel panic or cell fault" >&2
    FAIL=1
fi
if grep -qia "KERNEL PANIC\|\[fault\] Cell" "$NODE_B_LOG"; then
    echo "FAIL: Node B reported kernel panic or cell fault" >&2
    FAIL=1
fi

if (( FAIL == 0 )); then
    echo "=========================================================================="
    echo "  PASS: Two-Node C2C Cluster Boot & Layer-2 Link Verified"
    echo "  Node A: $(grep -ai "mac=52:54:00:c2:c0:01" "$NODE_A_LOG" | head -1)"
    echo "          $(grep -ai "169.254.192.1" "$NODE_A_LOG" | head -1)"
    echo "  Node B: $(grep -ai "mac=52:54:00:c2:c0:02" "$NODE_B_LOG" | head -1)"
    echo "          $(grep -ai "169.254.192.2" "$NODE_B_LOG" | head -1)"
    echo "  Logs:   $NODE_A_LOG and $NODE_B_LOG"
    echo "=========================================================================="
    exit 0
else
    echo "=========================================================================="
    echo "  FAIL: Two-node C2C cluster verification failed"
    echo "=========================================================================="
    exit 1
fi
