#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Two-node x86_64 C2C cluster test under QEMU KVM/TCG.
# Boots Node A and Node B on one virtual Ethernet segment and requires
# peer-specific evidence: each node's NET cell must hear the *other* node's
# RFC 3927 link-local ARP (its address and MAC), and at least one node must log
# the other's ARP reply to its request. Frames QEMU's multicast netdev loops
# back to the sender carry the sender's own MAC and are never counted.
#
# C2C_NEGATIVE=1 boots Node A alone. The peer assertions then have nothing to
# see and the script MUST exit non-zero; that run proves the gate can fail.
#
# The ISO must embed the current NET cell. Build one without touching the
# tracked kernel/src/embedded-x86_64 artifacts (dev key; out-dir and cargo
# target dir outside the repository; cell toolchain env as in CI's x86 job):
#   CARGO_TARGET_DIR=/tmp/c2c/target python3 scripts/assemble-x86-image.py \
#       --profile headless --out-dir /tmp/c2c/embedded
#   EMBEDDED_OVERRIDE=/tmp/c2c/embedded CARGO_TARGET_DIR=/tmp/c2c/kernel \
#       cargo build --release -p cellos-kernel --target x86_64-unknown-none \
#       -Z build-std=core,alloc --features policy-required
#   X86_KERNEL=/tmp/c2c/kernel/x86_64-unknown-none/release/cellos-kernel \
#       X86_ISO_ROOT=/tmp/c2c/iso-root bash scripts/x86/make-iso-ci.sh build/cellos-x86_64.iso
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ISO="${1:-build/cellos-x86_64.iso}"
BOOT_WINDOW="${BOOT_WINDOW:-45}"
X86_ACCEL="${X86_ACCEL:-kvm}"
X86_CPU_MODEL="${X86_CPU_MODEL:-host}"
SMP="${SMP:-1}"
SOCKET_PORT="${C2C_SOCKET_PORT:-23456}"
C2C_TRANSPORT="${C2C_TRANSPORT:-mcast}"
MCAST_GROUP="${C2C_MCAST_GROUP:-239.0.0.1}"
NEGATIVE="${C2C_NEGATIVE:-0}"

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
    echo "Build it with the commands in this script's header." >&2
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
if [[ "$NEGATIVE" == "1" ]]; then
    echo "  Mode:        NEGATIVE self-check (Node A only; this run MUST FAIL)"
fi
echo "=========================================================================="

# Cleanup handler on exit/interrupt
NODE_A_PID=""
NODE_B_PID=""
FEEDER_PID=""
FIFO_A="$WORK_DIR/node-a.in"
FIFO_B="$WORK_DIR/node-b.in"
rm -f "$FIFO_A" "$FIFO_B"
mkfifo "$FIFO_A" "$FIFO_B"
exec 3<>"$FIFO_A"
exec 4<>"$FIFO_B"

cleanup() {
    if [[ -n "$FEEDER_PID" ]] && kill -0 "$FEEDER_PID" 2>/dev/null; then
        kill "$FEEDER_PID" 2>/dev/null || true
    fi
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
# A trapped INT/TERM would otherwise resume the script and grade partial logs.
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# wait_for LOG TEXT SECONDS: poll until LOG contains TEXT, at most SECONDS.
wait_for() {
    local deadline=$((SECONDS + $3))
    while (( SECONDS < deadline )); do
        if grep -qaF -- "$2" "$1" 2>/dev/null; then return 0; fi
        sleep 0.5
    done
    return 1
}

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
NODE_LOGS=("$NODE_A_LOG")
NODE_FIFOS=("$FIFO_A")
if [[ "$NEGATIVE" == "1" ]]; then
    echo "[c2c-cluster] NEGATIVE self-check: Node B is not launched."
else
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
    NODE_LOGS+=("$NODE_B_LOG")
    NODE_FIFOS+=("$FIFO_B")
fi

# Background feeder: once every node has its prompt and has bound its
# link-local address (probe + announce take ~10 s after DHCP gives up), give the
# ARP verification round trip time to land, then ifconfig and shut down.
(
    for log in "${NODE_LOGS[@]}"; do
        wait_for "$log" "Cellos >" "$((BOOT_WINDOW - 10))" || true
        wait_for "$log" "(IPv4 Link-Local RFC 3927)" "$((BOOT_WINDOW - 10))" || true
    done
    sleep 3
    for fifo in "${NODE_FIFOS[@]}"; do echo "ifconfig" > "$fifo"; done
    sleep 2
    for fifo in "${NODE_FIFOS[@]}"; do echo "/bin/shutdown" > "$fifo"; done
) &
FEEDER_PID=$!
# ── 3. Await Boot Completion ────────────────────────────────────────────────
echo "[c2c-cluster] Waiting for the node(s) to finish..."
wait "$NODE_A_PID" || true
if [[ -n "$NODE_B_PID" ]]; then wait "$NODE_B_PID" || true; fi
NODE_A_PID=""
NODE_B_PID=""
touch "$NODE_B_LOG"

# ── 4. Verify Oracles & Evidence ─────────────────────────────────────────────
FAIL=0
check() { # check LOG TEXT MESSAGE: fail unless LOG contains the fixed TEXT.
    if ! grep -qaF -- "$2" "$1"; then
        echo "FAIL: $3" >&2
        FAIL=1
    fi
}

check "$NODE_A_LOG" "Cellos >" "Node A did not reach shell prompt"
check "$NODE_B_LOG" "Cellos >" "Node B did not reach shell prompt"
check "$NODE_A_LOG" "mac=52:54:00:c2:c0:01" "Node A did not bind MAC 52:54:00:c2:c0:01"
check "$NODE_B_LOG" "mac=52:54:00:c2:c0:02" "Node B did not bind MAC 52:54:00:c2:c0:02"
check "$NODE_A_LOG" "[net] link-local: probing 169.254.192.1" "Node A did not probe 169.254.192.1"
check "$NODE_B_LOG" "[net] link-local: probing 169.254.192.2" "Node B did not probe 169.254.192.2"
check "$NODE_A_LOG" "(IPv4 Link-Local RFC 3927): 169.254.192.1" \
    "Node A did not bind IPv4 Link-Local 169.254.192.1"
check "$NODE_B_LOG" "(IPv4 Link-Local RFC 3927): 169.254.192.2" \
    "Node B did not bind IPv4 Link-Local 169.254.192.2"
check "$NODE_A_LOG" "eth0: inet 169.254.192.1  netmask 255.255.0.0" \
    "Node A user-space ifconfig did not report Link-Local address"
check "$NODE_B_LOG" "eth0: inet 169.254.192.2  netmask 255.255.0.0" \
    "Node B user-space ifconfig did not report Link-Local address"

# Peer-specific evidence: the other node's address *and* MAC. A node's own
# looped-back frames carry its own MAC and never produce a peer line.
PEER_B_AT_A="[net] link-local: peer 169.254.192.2 at 52:54:00:c2:c0:02"
PEER_A_AT_B="[net] link-local: peer 169.254.192.1 at 52:54:00:c2:c0:01"
check "$NODE_A_LOG" "$PEER_B_AT_A" "Node A never heard Node B's link-local ARP"
check "$NODE_B_LOG" "$PEER_A_AT_B" "Node B never heard Node A's link-local ARP"
# Whichever node binds first asks the later one for its address once it
# announces; the reply is a round trip through the peer's live IP stack.
if ! grep -qaF -- "$PEER_B_AT_A (reply)" "$NODE_A_LOG" &&
   ! grep -qaF -- "$PEER_A_AT_B (reply)" "$NODE_B_LOG"; then
    echo "FAIL: no node logged an ARP reply from its peer" >&2
    FAIL=1
fi
for log in "$NODE_A_LOG" "$NODE_B_LOG"; do
    if grep -qaE "link-local: (conflict|relinquished)" "$log"; then
        echo "FAIL: link-local address conflict in $log" >&2
        FAIL=1
    fi
done

if (( SMP > 1 )); then
    check "$NODE_A_LOG" "topology online=$SMP" "Node A did not bring $SMP CPUs online"
    check "$NODE_B_LOG" "topology online=$SMP" "Node B did not bring $SMP CPUs online"
fi
check "$NODE_A_LOG" "power: requesting poweroff" "Node A did not execute clean ACPI poweroff"
check "$NODE_B_LOG" "power: requesting poweroff" "Node B did not execute clean ACPI poweroff"

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
    echo "  PASS: Two-Node C2C Cluster Boot & Layer-2 Peer Exchange Verified"
    echo "  Node A: $(grep -aF "(IPv4 Link-Local RFC 3927)" "$NODE_A_LOG" | head -1)"
    grep -aF "link-local: peer" "$NODE_A_LOG" | sed 's/^/          /'
    echo "  Node B: $(grep -aF "(IPv4 Link-Local RFC 3927)" "$NODE_B_LOG" | head -1)"
    grep -aF "link-local: peer" "$NODE_B_LOG" | sed 's/^/          /'
    echo "  Logs:   $NODE_A_LOG and $NODE_B_LOG"
    echo "=========================================================================="
    exit 0
else
    echo "=========================================================================="
    echo "  FAIL: Two-node C2C cluster verification failed"
    echo "=========================================================================="
    exit 1
fi
