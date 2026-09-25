#!/usr/bin/env bash
# Boot the Tier-2 C child-launch adapter witness in QEMU and assert its markers.
#
# What this proves, in one run:
#   1. `c-spawn` (caller of the C adapter) and `c-spawn-child` (the launched
#      child) are both admitted as `FFI`-class Tier 2 paged domains,
#   2. the command line staged by `cellos_spawn` reaches the child byte-for-byte,
#      including an item that contains a space,
#   3. the pipe endpoint granted with the launch is usable by the child, its
#      ordered payload crosses the ring intact, and EOF is exact,
#   4. the child's exit status is observable through `cellos_child_wait`,
#   5. an unreviewed target is denied by the kernel launch edge,
#   6. an over-long command line is refused before the kernel sees it,
#   7. a denied launch leaves no staged command line behind.
#
# `--harts 2` repeats the run with a second hart online.
#
# Usage: scripts/qemu-c-spawn.sh [--harts 1|2] [kernel-elf] [disk.img]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

HARTS=1
ARGS=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --harts)
            HARTS="$2"
            shift 2
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done
[[ "$HARTS" == "1" || "$HARTS" == "2" ]] || { echo "FAIL: --harts must be 1 or 2" >&2; exit 2; }

KERNEL="${ARGS[0]:-target/riscv64gc-unknown-none-elf/release/cellos-kernel}"
DISK="${ARGS[1]:-disk_v3.img}"
TARGET="${C_SPAWN_TARGET:-riscv64gc-unknown-none-elf}"
CELL_ELF="target/${TARGET}/release/c-spawn"
CHILD_ELF="target/${TARGET}/release/c-spawn-child"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"

for tool in "$QEMU" python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done
[[ -f "$DISK" ]] || { echo "FAIL: disk image not found: $DISK" >&2; exit 1; }

# The default-feature kernel is rebuilt here on purpose: `scripts/build-test-hooks-ci.sh`
# writes its test-hooks build to the same `cellos-kernel` path, so a stale artifact from
# another lane would silently change what this run proves (or deny the launch edge).
echo "==> Building the default-feature kernel"
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cellos-kernel

echo "==> Building the C spawn witness cells ($TARGET)"
cargo build --release --target "$TARGET" -p app-c-spawn -p app-c-spawn-child
[[ -f "$CELL_ELF" ]] || { echo "FAIL: launcher cell ELF not found: $CELL_ELF" >&2; exit 1; }
[[ -f "$CHILD_ELF" ]] || { echo "FAIL: child cell ELF not found: $CHILD_ELF" >&2; exit 1; }

PYTHON_BIN="${PYTHON_BIN:-python3}" source scripts/lib-sign-cells.sh
sign_cells "$CELL_ELF" "$CHILD_ELF"

WORKDIR="$(mktemp -d)"
DISK_COPY="$WORKDIR/disk-c-spawn.img"
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
        cp "$RAW_LOG" "$EVIDENCE_DIR/c-spawn-harts${HARTS}-qemu.log"
        [[ -f "$LOG" ]] && cp "$LOG" "$EVIDENCE_DIR/c-spawn-harts${HARTS}-qemu.txt"
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "==> Preparing a private disk copy with /bin/c-spawn and /bin/c-spawn-child"
cp "$DISK" "$DISK_COPY"
python3 tools/add-cell-to-disk.py "$DISK_COPY" \
    "/bin/c-spawn=$CELL_ELF" "/bin/c-spawn-child=$CHILD_ELF"

echo "==> Booting (harts=$HARTS, window ${BOOT_WINDOW}s)"
mkfifo "$FIFO"
timeout "$BOOT_WINDOW" "$QEMU" \
    -machine virt \
    -m 256M \
    -nographic \
    -bios default \
    -smp "$HARTS" \
    -kernel "$KERNEL" \
    -drive "file=$DISK_COPY,format=raw,id=hd0,if=none" \
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
printf 'c-spawn\n' >&3

if ! wait_for "C-SPAWN-QEMU: PASS" 45; then
    echo "FAIL: c-spawn did not reach its PASS marker" >&2
    tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' | tail -40 >&2
    exit 1
fi
exec 3>&-
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

require "\[domain\] admitted cell 'c-spawn'" "Tier 2 launcher-domain admission"
require "\[domain\] admitted cell 'c-spawn-child'" "Tier 2 launched-child-domain admission"
require "\[c-spawn\] launched tid=" "the reviewed child launched"
require "\[c-spawn\] route elf-bytes reviewed edge" "the launch used the reviewed ELF edge"
require "\[c-spawn-child\] argv items=2" "the child observed both command-line items"
require "\[c-spawn\] child argv items=2" "the launcher observed the child's argv report"
require "\[c-spawn\] ordered payload bytes=192" "the granted endpoint carried an ordered payload"
require "\[c-spawn-child\] payload bytes=192" "the child wrote the full payload"
require "\[c-spawn\] child reported status=42" "the child's status crossed the granted endpoint"
require_any "\[c-spawn\] wait published status=42\|\[c-spawn\] wait already-terminal" \
    "the launcher observed the child's terminal state"
require "\[c-spawn\] unreviewed target denied" "an unreviewed target is denied"
require "\[c-spawn\] overlong argv rejected" "an over-long command line is refused before the kernel sees it"
require "\[c-spawn\] denied launch left no staged argv" "a denied launch left no staged command line"
require "C-SPAWN-QEMU: PASS" "cell PASS marker"

if grep -qa "\[c-spawn\] FAIL\|\[c-spawn-child\] argv mismatch\|\[fault\] Cell" "$LOG"; then
    echo "FAIL: a witness stage failed or the kernel killed a cell" >&2
    grep -a "c-spawn.*FAIL\|argv mismatch\|\[fault\] Cell" "$LOG" | head >&2
    fail=1
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
    echo "==> c-spawn evidence failed; log tail:" >&2
    grep -a "c-spawn\|domain\]" "$LOG" | tail -20 >&2
    exit 1
fi

echo "==> cell markers:"
grep -a "\[c-spawn" "$LOG" | sed 's/^USER: //' || true
echo "C-SPAWN-QEMU: PASS target=$TARGET harts=$HARTS kernel=$(basename "$KERNEL")"
