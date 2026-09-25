#!/usr/bin/env bash
# Boot the B0 actor/supervisor witness in QEMU and assert its markers.
#
# What this proves, in one run:
#   1. `/bin/backend-supervisor` is launched by the shell and declares a supervisor
#      tree over `/bin/backend-worker`,
#   2. typed actor call/reply works across cells (a ping answered by each child),
#   3. killing a worker restarts it inside the 100-tick (1 s) bound, with the exit
#      reason observed by the supervisor,
#   4. six abnormal exits inside one intensity window make the supervisor give up
#      on that child alone — the other children still answer afterwards,
#   5. `one_for_all` brings the failed child *and* its sibling back.
#
# Why the canonical image and not a private overlay: the supervisor holds
# `SpawnCap`, and the kernel's launch-profile check deliberately refuses a
# non-empty child ceiling on the `SpawnFromElf` route (caller-supplied bytes must
# not borrow a profile that carries authority). An authority-bearing cell can
# therefore only resolve through the kernel loader, i.e. it must be staged in
# VIFS1 — which is embedded in the kernel binary. `gen_disk.ps1` stages both
# witness cells (`/bin/backend-supervisor` into VIFS1, both into the P2 table and
# the FAT cell-store), so this runner boots the canonical kernel plus `disk_v3.img`
# and fails loudly if that staging is missing.
#
# Usage: scripts/qemu-actor-supervisor.sh [--harts 1|2] [kernel-elf] [disk.img]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

HARTS=1
ARGS=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --harts) HARTS="$2"; shift 2 ;;
        --harts=*) HARTS="${1#*=}"; shift ;;
        *) ARGS+=("$1"); shift ;;
    esac
done
[[ "$HARTS" == "1" || "$HARTS" == "2" ]] || { echo "FAIL: --harts must be 1 or 2" >&2; exit 2; }

KERNEL="${ARGS[0]:-target/riscv64gc-unknown-none-elf/release/cellos-kernel}"
DISK="${ARGS[1]:-disk_v3.img}"
TARGET="${ACTOR_SUPERVISOR_TARGET:-riscv64gc-unknown-none-elf}"
VIFS1="kernel/src/embedded/kernel_fs.img"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
PASS_WINDOW="${PASS_WINDOW:-120}"

for tool in "$QEMU" python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done
[[ -f "$DISK" ]] || { echo "FAIL: disk image not found: $DISK — run ./gen_disk.ps1" >&2; exit 1; }
[[ -f "$VIFS1" ]] || {
    echo "FAIL: $VIFS1 is missing — run ./gen_disk.ps1 (it stages the witness cells)" >&2
    exit 1
}
# Capture first, then match: `inspect_fat.py | grep -q` would die of SIGPIPE as soon
# as grep matched, and `pipefail` would turn that into a false "missing cell".
VIFS1_LAYOUT="$(python3 tools/inspect_fat.py "$VIFS1")"
if ! grep -qa "backend-supervisor" <<<"$VIFS1_LAYOUT"; then
    echo "FAIL: $VIFS1 does not contain /bin/backend-supervisor." >&2
    echo "      The supervisor is an authority-bearing cell, so it must be staged in VIFS1:" >&2
    echo "      run ./gen_disk.ps1 before this runner." >&2
    exit 1
fi

# The kernel embeds $VIFS1 at build time; rebuilding here guarantees the booted
# kernel carries the image just checked (a stale kernel would fail with
# "command not found" instead of naming the real cause).
echo "==> Building the default-feature kernel (embeds $VIFS1)"
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cellos-kernel

WORKDIR="$(mktemp -d)"
RAW_LOG="$WORKDIR/qemu.raw.log"
LOG="$WORKDIR/qemu.log"
FIFO="$WORKDIR/stdin"
# Optional evidence capture: EVIDENCE_DIR=<dir> keeps the raw QEMU log and the
# stripped log, which is what a published evidence record cites.
EVIDENCE_DIR="${EVIDENCE_DIR:-}"
cleanup() {
    # A trap must not mask the script's own exit status: capture evidence when
    # the log exists, and never fail the cleanup.
    if [[ -n "$EVIDENCE_DIR" && -f "$RAW_LOG" ]]; then
        mkdir -p "$EVIDENCE_DIR"
        cp "$RAW_LOG" "$EVIDENCE_DIR/actor-supervisor-harts${HARTS}-qemu.log"
        [[ -f "$LOG" ]] && cp "$LOG" "$EVIDENCE_DIR/actor-supervisor-harts${HARTS}-qemu.txt"
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "==> Booting (harts=$HARTS, window ${BOOT_WINDOW}s)"
mkfifo "$FIFO"
# `-snapshot` gives the guest a temporary overlay: the canonical disk stays
# pristine (the guest writes to its FAT volume during boot) and two runs can never
# collide on QEMU's write lock.
timeout "$BOOT_WINDOW" "$QEMU" \
    -machine virt \
    -m 256M \
    -nographic \
    -snapshot \
    -bios default \
    -smp "$HARTS" \
    -kernel "$KERNEL" \
    -drive "file=$DISK,format=raw,id=hd0,if=none" \
    -device virtio-blk-device,drive=hd0 \
    -device virtio-keyboard-device \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 \
    -device virtio-gpu-device \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 3> "$FIFO"

wait_for() {
    local pattern="$1" seconds="$2" i
    for ((i = 0; i < seconds; i++)); do
        grep -qa "$pattern" "$RAW_LOG" && return 0
        sleep 1
    done
    return 1
}

if ! wait_for "Cellos >" 60; then
    echo "FAIL: shell prompt not reached" >&2
    tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' | tail -40 >&2
    exit 1
fi
sleep 1
printf 'backend-supervisor\n' >&3

if ! wait_for "ACTOR-SUPERVISOR: PASS" "$PASS_WINDOW"; then
    echo "FAIL: the supervisor witness did not reach its PASS marker" >&2
    tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' | tail -40 >&2
    exit 1
fi
exec 3>&-
# The guest keeps running after the marker; stop QEMU now instead of burning the
# rest of the boot window. The markers asserted below are all emitted before PASS.
sleep 1
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"

fail=0
require_any() {
    if grep -qa "$1" "$LOG"; then
        echo "PASS: $2"
    else
        echo "FAIL: $2 (missing either of: $1)" >&2
        fail=1
    fi
}
require() {
    if grep -qa "$1" "$LOG"; then
        echo "PASS: $2"
    else
        echo "FAIL: $2 (missing: $1)" >&2
        fail=1
    fi
}
require_count_at_least() {
    local pattern="$1" minimum="$2" label="$3" count
    count="$(grep -ac "$pattern" "$LOG" || true)"
    if [[ "${count:-0}" -ge "$minimum" ]]; then
        echo "PASS: $label ($count)"
    else
        echo "FAIL: $label (found ${count:-0}, need $minimum)" >&2
        fail=1
    fi
}

require "\[backend\] supervisor up: children w0 w1 w2" "the declared tree started"
require_count_at_least "\[backend-worker\] up" 3 "three workers were spawned and ran"
require "\[backend\] typed call to w0 ok" "typed actor call/reply reached a child"
require "\[backend\] exit observed child=w0 tid=.* reason=0x" "the exit reason was observed"
require "\[backend\] killed w0 tid=" "the supervisor killed its own child"
require "\[backend\] restart-latency ticks=" "a restart latency was measured"
require "\[backend\] restart-latency OK" "the restart landed inside the 1 s bound"
require "\[backend\] storm kill 6/6 on w1" "six abnormal exits hit the storm child"
require "\[backend\] give-up OK" "the restart budget was exhausted on that child only"
require "\[backend\] survivors OK" "the other children still answered after the give-up"
require "\[backend\] one-for-all OK" "one_for_all restarted the failed child and its sibling"
require "ACTOR-SUPERVISOR: PASS" "cell PASS marker"

if grep -qa "\[backend\] FAIL\|ACTOR-SUPERVISOR: FAIL\|\[fault\] Cell" "$LOG"; then
    echo "FAIL: a witness assertion failed or the kernel killed a cell" >&2
    grep -a "\[backend\] FAIL\|ACTOR-SUPERVISOR: FAIL\|\[fault\] Cell" "$LOG" | head >&2
    fail=1
fi

# The shell resolves a bare command name by trying `SpawnFromElf` first. For the
# supervisor that attempt is denied *by design* (an authority-bearing cell must not
# borrow a child ceiling on the caller-supplied-bytes route), and the kernel then
# resolves it through VIFS1 — so that particular DENY is informational here. What
# must never appear is a denial on the worker edge this witness depends on.
if grep -qa "DENY launch edge.*backend-worker" "$LOG"; then
    echo "FAIL: the kernel denied the supervisor's worker edge" >&2
    grep -a "DENY launch edge.*backend-worker" "$LOG" | head >&2
    fail=1
fi
if grep -qa "DENY launch edge.*backend-supervisor" "$LOG"; then
    echo "INFO: the shell's Elf-route attempt was denied as designed; VIFS1 resolved it"
fi

if [[ "$HARTS" == "2" ]]; then
    if grep -qa "\[smp\] hart 1 online" "$LOG"; then
        echo "PASS: second hart online"
    else
        echo "FAIL: requested two-hart run did not bring hart 1 online" >&2
        fail=1
    fi
fi

if [[ "$fail" -ne 0 ]]; then
    echo "==> actor/supervisor evidence failed; log tail:" >&2
    grep -a "\[backend\|\[supervisor" "$LOG" | tail -30 >&2
    exit 1
fi

echo "==> measured restart latency:"
grep -a "restart-latency ticks=" "$LOG" | tail -1 || true
echo "==> witness markers:"
grep -a "\[backend\|\[supervisor\|ACTOR-SUPERVISOR" "$LOG" | sed 's/^USER: //' || true
echo "ACTOR-SUPERVISOR-QEMU: PASS target=$TARGET harts=$HARTS kernel=$(basename "$KERNEL")"
