#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Two-node x86_64 C2C cluster peer restart & reconnection resilience test.
# Exercises: peer departure, peer arrival, dynamic reconnection, Link-Local retention.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ISO="${1:-build/cellos-x86_64.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-35}"
X86_ACCEL="${X86_ACCEL:-kvm}"
X86_CPU_MODEL="${X86_CPU_MODEL:-host}"
SOCKET_PORT="${C2C_SOCKET_PORT:-23460}"
MCAST_GROUP="${C2C_MCAST_GROUP:-239.0.0.1}"
WORK_DIR="${C2C_WORK_DIR:-build/two-node-restart}"

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: ISO not found: $ISO" >&2
    echo "Build with: bash scripts/build-cloud-images.sh" >&2
    exit 1
fi

mkdir -p "$WORK_DIR"
rm -f "$WORK_DIR"/*.log "$WORK_DIR"/*.in

ACCEL_ARGS=()
if [[ -n "$X86_ACCEL" ]]; then
    ACCEL_ARGS=(-accel "$X86_ACCEL")
fi

echo "=========================================================================="
echo "  Cellos C2C Peer Restart & Reconnection Resilience Gate"
echo "  ISO:         $ISO"
echo "  Accel:       $X86_ACCEL (CPU: $X86_CPU_MODEL)"
echo "  Cluster Bus: $MCAST_GROUP:$SOCKET_PORT"
echo "  Window:      ${BOOT_WINDOW}s"
echo "=========================================================================="

PID_A=""
PID_B1=""
PID_B2=""

cleanup() {
    if [[ -n "$PID_A" ]] && kill -0 "$PID_A" 2>/dev/null; then kill "$PID_A" 2>/dev/null || true; fi
    if [[ -n "$PID_B1" ]] && kill -0 "$PID_B1" 2>/dev/null; then kill "$PID_B1" 2>/dev/null || true; fi
    if [[ -n "$PID_B2" ]] && kill -0 "$PID_B2" 2>/dev/null; then kill "$PID_B2" 2>/dev/null || true; fi
    exec 3>&- 2>/dev/null || true
    exec 4>&- 2>/dev/null || true
    exec 5>&- 2>/dev/null || true
}
trap cleanup EXIT INT TERM

FIFO_A="$WORK_DIR/node-a.in"
FIFO_B1="$WORK_DIR/node-b1.in"
FIFO_B2="$WORK_DIR/node-b2.in"
mkfifo "$FIFO_A" "$FIFO_B1" "$FIFO_B2"
exec 3<>"$FIFO_A"
exec 4<>"$FIFO_B1"
exec 5<>"$FIFO_B2"

LOG_A="$WORK_DIR/node-a.log"
LOG_B1="$WORK_DIR/node-b1.log"
LOG_B2="$WORK_DIR/node-b2.log"

# ── 1. Launch Node A (Permanent Peer) ──────────────────────────────────────────
echo "[c2c-restart] Launching Node A (Permanent Peer, MAC 52:54:00:c2:c0:01)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "socket,id=c2clan,mcast=$MCAST_GROUP:$SOCKET_PORT" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:01" \
    < "$FIFO_A" > "$LOG_A" 2>&1 &
PID_A=$!

# ── 2. Launch Node B Run 1 ───────────────────────────────────────────────────
echo "[c2c-restart] Launching Node B Initial Run (MAC 52:54:00:c2:c0:02)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "socket,id=c2clan,mcast=$MCAST_GROUP:$SOCKET_PORT" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:02" \
    < "$FIFO_B1" > "$LOG_B1" 2>&1 &
PID_B1=$!

# Feeder for Node B Run 1: wait for prompt, query ifconfig, then clean shutdown
(
    for _ in {1..30}; do
        if grep -qa "Cellos >" "$LOG_B1" 2>/dev/null; then break; fi
        sleep 0.5
    done
    sleep 5
    echo "ifconfig" > "$FIFO_B1"
    sleep 1
    echo "/bin/shutdown" > "$FIFO_B1"
) &

wait "$PID_B1" || true
PID_B1=""
echo "[c2c-restart] Node B departed cleanly. Node A remains active on cluster bus."
sleep 2

# ── 3. Restart Node B (Reconnection Run 2) ───────────────────────────────────
echo "[c2c-restart] Restarting Node B (Reconnection to cluster bus)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "socket,id=c2clan,mcast=$MCAST_GROUP:$SOCKET_PORT" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:02" \
    < "$FIFO_B2" > "$LOG_B2" 2>&1 &
PID_B2=$!

# Feeder for Node B Run 2 & Node A shutdown
(
    for _ in {1..30}; do
        if grep -qa "Cellos >" "$LOG_B2" 2>/dev/null; then break; fi
        sleep 0.5
    done
    sleep 5
    echo "ifconfig" > "$FIFO_B2"
    echo "ifconfig" > "$FIFO_A"
    sleep 1
    echo "/bin/shutdown" > "$FIFO_B2"
    echo "/bin/shutdown" > "$FIFO_A"
) &

wait "$PID_B2" || true
wait "$PID_A" || true
PID_B2=""
PID_A=""

# ── 4. Verify Evidence ───────────────────────────────────────────────────────
FAIL=0

for log in "$LOG_A" "$LOG_B1" "$LOG_B2"; do
    if grep -qia "KERNEL PANIC\|\[fault\] Cell" "$log"; then
        echo "FAIL: Panic or fault found in $log" >&2
        FAIL=1
    fi
done

if ! grep -qa "eth0: inet 169.254.192.1  netmask 255.255.0.0" "$LOG_A"; then
    echo "FAIL: Node A did not retain Link-Local address" >&2
    FAIL=1
fi
if ! grep -qa "eth0: inet 169.254.192.2  netmask 255.255.0.0" "$LOG_B1"; then
    echo "FAIL: Node B (run 1) did not configure Link-Local address" >&2
    FAIL=1
fi
if ! grep -qa "eth0: inet 169.254.192.2  netmask 255.255.0.0" "$LOG_B2"; then
    echo "FAIL: Node B (restart run 2) did not re-acquire Link-Local address" >&2
    FAIL=1
fi

if ! grep -qa "first e1000 RX" "$LOG_A"; then
    echo "FAIL: Node A did not receive frames from cluster" >&2
    FAIL=1
fi
if ! grep -qa "first e1000 RX" "$LOG_B1"; then
    echo "FAIL: Node B1 did not receive frames from cluster" >&2
    FAIL=1
fi
if ! grep -qa "first e1000 RX" "$LOG_B2"; then
    echo "FAIL: Node B2 did not receive frames from cluster" >&2
    FAIL=1
fi

if (( FAIL == 0 )); then
    echo "=========================================================================="
    echo "  PASS: C2C Peer Departure & Restart Reconnection Verified"
    echo "  Node A:  $(grep -ai "eth0: inet 169.254.192.1" "$LOG_A" | head -1)"
    echo "  Node B1: $(grep -ai "eth0: inet 169.254.192.2" "$LOG_B1" | head -1)"
    echo "  Node B2: $(grep -ai "eth0: inet 169.254.192.2" "$LOG_B2" | head -1)"
    echo "  Logs:    $LOG_A, $LOG_B1, $LOG_B2"
    echo "=========================================================================="
    exit 0
else
    echo "=========================================================================="
    echo "  FAIL: C2C Peer Restart & Reconnection Verification Failed"
    echo "=========================================================================="
    exit 1
fi
