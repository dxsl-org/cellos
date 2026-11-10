#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Two-node x86_64 C2C cluster peer restart & reconnection resilience test.
# Exercises: peer departure, peer arrival, dynamic reconnection, Link-Local retention.
#
# Reconnection is proved with peer-specific evidence from the NET cells' RFC
# 3927 link-local ARP: after Node B restarts, Node A's log (from the moment the
# restarted B was launched) must show B's fresh announcement and B's ARP reply
# to A's request, and the restarted B must have heard A. Frames QEMU's
# multicast netdev loops back to the sender carry the sender's own MAC and are
# never counted.
#
# C2C_NEGATIVE=1 runs Node A alone for the same schedule; the peer assertions
# then MUST fail, which proves the gate can fail.
#
# The ISO must embed the current NET cell; see qemu-two-node-c2c-test.sh's
# header for a build that leaves the tracked embedded artifacts untouched.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ISO="${1:-build/cellos-x86_64.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-45}"
# Node A must outlive Node B's first run, the gap and Node B's second run.
NODE_A_WINDOW=$((2 * BOOT_WINDOW + 10))
X86_ACCEL="${X86_ACCEL:-kvm}"
X86_CPU_MODEL="${X86_CPU_MODEL:-host}"
SOCKET_PORT="${C2C_SOCKET_PORT:-23460}"
MCAST_GROUP="${C2C_MCAST_GROUP:-239.0.0.1}"
WORK_DIR="${C2C_WORK_DIR:-build/two-node-restart}"
NEGATIVE="${C2C_NEGATIVE:-0}"

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: ISO not found: $ISO" >&2
    echo "Build it with the commands in scripts/qemu-two-node-c2c-test.sh's header." >&2
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
if [[ "$NEGATIVE" == "1" ]]; then
    echo "  Mode:        NEGATIVE self-check (Node A only; this run MUST FAIL)"
fi
echo "=========================================================================="

PID_A=""
PID_B1=""
PID_B2=""
FEEDER1_PID=""
FEEDER2_PID=""

cleanup() {
    for pid in "$FEEDER1_PID" "$FEEDER2_PID" "$PID_A" "$PID_B1" "$PID_B2"; do
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then kill "$pid" 2>/dev/null || true; fi
    done
    exec 3>&- 2>/dev/null || true
    exec 4>&- 2>/dev/null || true
    exec 5>&- 2>/dev/null || true
    rm -f "$FIFO_A" "$FIFO_B1" "$FIFO_B2"
}
FIFO_A="$WORK_DIR/node-a.in"
FIFO_B1="$WORK_DIR/node-b1.in"
FIFO_B2="$WORK_DIR/node-b2.in"
# A trapped INT/TERM would otherwise resume the script and grade partial logs.
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkfifo "$FIFO_A" "$FIFO_B1" "$FIFO_B2"
exec 3<>"$FIFO_A"
exec 4<>"$FIFO_B1"
exec 5<>"$FIFO_B2"

LOG_A="$WORK_DIR/node-a.log"
LOG_B1="$WORK_DIR/node-b1.log"
LOG_B2="$WORK_DIR/node-b2.log"
# Node A's log from the restarted Node B's launch on.
LOG_A_AFTER="$WORK_DIR/node-a-after-restart.log"

# wait_for LOG TEXT SECONDS: poll until LOG contains TEXT, at most SECONDS.
wait_for() {
    local deadline=$((SECONDS + $3))
    while (( SECONDS < deadline )); do
        if grep -qaF -- "$2" "$1" 2>/dev/null; then return 0; fi
        sleep 0.5
    done
    return 1
}

# launch_b FIFO LOG: one Node B run in the background; sets LAUNCHED_PID.
launch_b() {
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
        < "$1" > "$2" 2>&1 &
    LAUNCHED_PID=$!
}

# ── 1. Launch Node A (Permanent Peer) ──────────────────────────────────────────
echo "[c2c-restart] Launching Node A (Permanent Peer, MAC 52:54:00:c2:c0:01)..."
timeout "$NODE_A_WINDOW" qemu-system-x86_64 \
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
if [[ "$NEGATIVE" == "1" ]]; then
    echo "[c2c-restart] NEGATIVE self-check: Node B run 1 is not launched."
    sleep "$BOOT_WINDOW"
    : > "$LOG_B1"
else
    echo "[c2c-restart] Launching Node B Initial Run (MAC 52:54:00:c2:c0:02)..."
    launch_b "$FIFO_B1" "$LOG_B1"
    PID_B1=$LAUNCHED_PID

    # Feeder for Node B run 1: prompt and bound link-local address, the ARP
    # verification round trip, then ifconfig and a clean shutdown.
    (
        wait_for "$LOG_B1" "Cellos >" "$((BOOT_WINDOW - 10))" || true
        wait_for "$LOG_B1" "(IPv4 Link-Local RFC 3927)" "$((BOOT_WINDOW - 10))" || true
        sleep 3
        echo "ifconfig" > "$FIFO_B1"
        sleep 1
        echo "/bin/shutdown" > "$FIFO_B1"
    ) &
    FEEDER1_PID=$!

    wait "$PID_B1" || true
    PID_B1=""
    echo "[c2c-restart] Node B departed cleanly. Node A remains active on cluster bus."
fi
sleep 2

# ── 3. Restart Node B (Reconnection Run 2) ───────────────────────────────────
A_OFFSET=$(stat -c %s "$LOG_A")
if [[ "$NEGATIVE" == "1" ]]; then
    echo "[c2c-restart] NEGATIVE self-check: Node B run 2 is not launched."
    : > "$LOG_B2"
    WAIT_LOGS=("$LOG_A")
else
    echo "[c2c-restart] Restarting Node B (Reconnection to cluster bus)..."
    launch_b "$FIFO_B2" "$LOG_B2"
    PID_B2=$LAUNCHED_PID
    WAIT_LOGS=("$LOG_B2")
fi

# Feeder for Node B run 2 & Node A shutdown
(
    for log in "${WAIT_LOGS[@]}"; do
        wait_for "$log" "Cellos >" "$((BOOT_WINDOW - 10))" || true
        wait_for "$log" "(IPv4 Link-Local RFC 3927)" "$((BOOT_WINDOW - 10))" || true
    done
    if [[ "$NEGATIVE" == "1" ]]; then sleep 15; fi
    sleep 3
    echo "ifconfig" > "$FIFO_B2"
    echo "ifconfig" > "$FIFO_A"
    sleep 1
    echo "/bin/shutdown" > "$FIFO_B2"
    echo "/bin/shutdown" > "$FIFO_A"
) &
FEEDER2_PID=$!

if [[ -n "$PID_B2" ]]; then wait "$PID_B2" || true; fi
wait "$PID_A" || true
PID_B2=""
PID_A=""
tail -c "+$((A_OFFSET + 1))" "$LOG_A" > "$LOG_A_AFTER"

# ── 4. Verify Evidence ───────────────────────────────────────────────────────
FAIL=0
check() { # check LOG TEXT MESSAGE: fail unless LOG contains the fixed TEXT.
    if ! grep -qaF -- "$2" "$1"; then
        echo "FAIL: $3" >&2
        FAIL=1
    fi
}

for log in "$LOG_A" "$LOG_B1" "$LOG_B2"; do
    if grep -qia "KERNEL PANIC\|\[fault\] Cell" "$log"; then
        echo "FAIL: Panic or fault found in $log" >&2
        FAIL=1
    fi
    if grep -qaE "link-local: (conflict|relinquished)" "$log"; then
        echo "FAIL: link-local address conflict in $log" >&2
        FAIL=1
    fi
done

check "$LOG_A" "eth0: inet 169.254.192.1  netmask 255.255.0.0" \
    "Node A did not retain Link-Local address"
check "$LOG_B1" "eth0: inet 169.254.192.2  netmask 255.255.0.0" \
    "Node B (run 1) did not configure Link-Local address"
check "$LOG_B2" "eth0: inet 169.254.192.2  netmask 255.255.0.0" \
    "Node B (restart run 2) did not re-acquire Link-Local address"

# Peer-specific evidence: the other node's address *and* MAC.
PEER_B_AT_A="[net] link-local: peer 169.254.192.2 at 52:54:00:c2:c0:02"
PEER_A_AT_B="[net] link-local: peer 169.254.192.1 at 52:54:00:c2:c0:01"
check "$LOG_A" "$PEER_B_AT_A" "Node A never heard Node B (run 1)"
check "$LOG_B1" "$PEER_A_AT_B" "Node B (run 1) never heard Node A"
# Reconnection: only lines Node A logged after the restarted B was launched.
check "$LOG_A_AFTER" "$PEER_B_AT_A (announce)" \
    "Node A did not rediscover the restarted Node B"
check "$LOG_A_AFTER" "$PEER_B_AT_A (reply)" \
    "the restarted Node B did not answer Node A's ARP request"
check "$LOG_B2" "$PEER_A_AT_B" "the restarted Node B never heard Node A"

if (( FAIL == 0 )); then
    echo "=========================================================================="
    echo "  PASS: C2C Peer Departure & Restart Reconnection Verified"
    echo "  Node A:  $(grep -ai "eth0: inet 169.254.192.1" "$LOG_A" | head -1)"
    echo "  Node B1: $(grep -ai "eth0: inet 169.254.192.2" "$LOG_B1" | head -1)"
    echo "  Node B2: $(grep -ai "eth0: inet 169.254.192.2" "$LOG_B2" | head -1)"
    echo "  Node A after restart:"
    grep -aF "link-local: peer" "$LOG_A_AFTER" | sed 's/^/           /'
    echo "  Node B2:"
    grep -aF "link-local: peer" "$LOG_B2" | sed 's/^/           /'
    echo "  Logs:    $LOG_A, $LOG_B1, $LOG_B2"
    echo "=========================================================================="
    exit 0
else
    echo "=========================================================================="
    echo "  FAIL: C2C Peer Restart & Reconnection Verification Failed"
    echo "=========================================================================="
    exit 1
fi
