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
SOCKET_PORT="${C2C_SOCKET_PORT:-23456}"
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
echo "  Accel:       $X86_ACCEL (CPU: $X86_CPU_MODEL)"
echo "  L2 Socket:   127.0.0.1:$SOCKET_PORT"
echo "  Window:      ${BOOT_WINDOW}s"
echo "=========================================================================="

# Cleanup handler on exit/interrupt
NODE_A_PID=""
NODE_B_PID=""
cleanup() {
    if [[ -n "$NODE_A_PID" ]] && kill -0 "$NODE_A_PID" 2>/dev/null; then
        kill "$NODE_A_PID" 2>/dev/null || true
    fi
    if [[ -n "$NODE_B_PID" ]] && kill -0 "$NODE_B_PID" 2>/dev/null; then
        kill "$NODE_B_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

# ── 1. Launch Node A (Server / Socket Listener) ──────────────────────────────
echo "[c2c-cluster] Launching Node A (L2 Listener on :$SOCKET_PORT, MAC 52:54:00:c2:c0:01)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "socket,id=c2clan,listen=127.0.0.1:$SOCKET_PORT" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:01" \
    < /dev/null > "$NODE_A_LOG" 2>&1 &
NODE_A_PID=$!

# Wait briefly for Node A to bind the listener port
sleep 1.5

# ── 2. Launch Node B (Client / Socket Connect) ───────────────────────────────
echo "[c2c-cluster] Launching Node B (L2 Connector to :$SOCKET_PORT, MAC 52:54:00:c2:c0:02)..."
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 \
    "${ACCEL_ARGS[@]}" \
    -cpu "$X86_CPU_MODEL" \
    -m 256M \
    -nographic \
    -cdrom "$ISO" \
    -boot d \
    -no-reboot \
    -netdev "socket,id=c2clan,connect=127.0.0.1:$SOCKET_PORT" \
    -device "virtio-net-pci,netdev=c2clan,mac=52:54:00:c2:c0:02" \
    < /dev/null > "$NODE_B_LOG" 2>&1 &
NODE_B_PID=$!

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
