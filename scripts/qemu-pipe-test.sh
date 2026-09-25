#!/usr/bin/env bash
# Boot the kernel pipe smoke cell in QEMU and assert its markers.
#
# What this proves, in one run:
#   1. two independent cells are admitted as `FFI`-class Tier 2 paged domains,
#   2. a payload four times the ring crosses domains without loss or reordering;
#      the peer reports only after it fills the fixed-capacity ring, then its
#      next write blocks until the parent drains,
#   3. EOF is observed exactly when the last writer end closes,
#   4. a write with no reader end returns BrokenPipe,
#   5. a handle this task does not own is denied.
#
# `--harts 2` repeats the run with a second hart online.
#
# Usage: scripts/qemu-pipe-test.sh [--harts 1|2] [kernel-elf] [disk.img]
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
TARGET="${PIPE_TEST_TARGET:-riscv64gc-unknown-none-elf}"
CELL_ELF="target/${TARGET}/release/pipe-test"
PEER_ELF="target/${TARGET}/release/pipe-peer"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"
BOOT_WINDOW="${BOOT_WINDOW:-90}"

for tool in "$QEMU" python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done
[[ -f "$KERNEL" ]] || { echo "FAIL: kernel ELF not found: $KERNEL (build it first)" >&2; exit 1; }
[[ -f "$DISK" ]] || { echo "FAIL: disk image not found: $DISK" >&2; exit 1; }

# The default-feature kernel is rebuilt here on purpose: `scripts/build-test-hooks-ci.sh`
# writes its test-hooks build to the same `cellos-kernel` path, so a stale artifact from
# another lane would silently change what this run proves (or deny the launch edge).
echo "==> Building the default-feature kernel"
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cellos-kernel

echo "==> Building the pipe-test cells ($TARGET)"
cargo build --release --target "$TARGET" -p app-pipe-test -p app-pipe-peer
[[ -f "$CELL_ELF" ]] || { echo "FAIL: parent cell ELF not found: $CELL_ELF" >&2; exit 1; }
[[ -f "$PEER_ELF" ]] || { echo "FAIL: peer cell ELF not found: $PEER_ELF" >&2; exit 1; }

PYTHON_BIN="${PYTHON_BIN:-python3}" source scripts/lib-sign-cells.sh
sign_cells "$CELL_ELF" "$PEER_ELF"

WORKDIR="$(mktemp -d)"
DISK_COPY="$WORKDIR/disk-pipe-test.img"
RAW_LOG="$WORKDIR/qemu.raw.log"
LOG="$WORKDIR/qemu.log"
FIFO="$WORKDIR/stdin"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

echo "==> Preparing a private disk copy with /bin/pipe-test and /bin/pipe-peer"
cp "$DISK" "$DISK_COPY"
python3 tools/add-cell-to-disk.py "$DISK_COPY" "/bin/pipe-test=$CELL_ELF" "/bin/pipe-peer=$PEER_ELF"

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
printf 'pipe-test\n' >&3

if ! wait_for "PIPE-TEST: PASS" 45; then
    echo "FAIL: pipe-test did not reach its PASS marker" >&2
    tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' | tail -40 >&2
    exit 1
fi
exec 3>&-
wait "$QEMU_PID" 2>/dev/null || true

tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"

fail=0
require() {
    if grep -qa "$1" "$LOG"; then
        echo "PASS: $2"
    else
        echo "FAIL: $2 (missing: $1)" >&2
        fail=1
    fi
}

require "\[domain\] admitted cell 'pipe-test'" "Tier 2 parent-domain admission"
require "\[domain\] admitted cell 'pipe-peer'" "Tier 2 peer-domain admission"
require "\[pipe-test\] created capacity=256" "pipe created with the requested capacity"
require "\[pipe-test\] qemu drain payload=1024 scheduler-ticks=" "QEMU drain duration recorded"
require "\[pipe-test\] qemu full-ring wake scheduler-ticks=" "full-ring wake latency recorded"
require "\[pipe-test\] backpressure writer-filled=256 cap=256" "writer filled one bounded ring before the parent drained"
require "\[pipe-test\] read total=1024 checksum=" "full payload crossed the ring in order"
require "\[pipe-test\] eof ok" "EOF observed when the last writer end closed"
require "\[pipe-test\] broken-pipe ok" "write with no reader end returned BrokenPipe"
require "\[pipe-test\] unauthorized ok" "a handle this task does not own was denied"
require "PIPE-TEST: PASS" "cell PASS marker"

if grep -qa "\[pipe-test\] FAIL\|\[fault\] Cell" "$LOG"; then
    echo "FAIL: cell reported a failure stage or the kernel killed it" >&2
    grep -a "pipe-test\] FAIL\|\[fault\] Cell" "$LOG" | head >&2
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
    echo "==> pipe-test evidence failed; log tail:" >&2
    grep -a "pipe-test\|domain\]" "$LOG" | tail -20 >&2
    exit 1
fi

echo "==> cell markers:"
grep -a "\[pipe-test\]" "$LOG" | sed 's/^USER: //' || true
echo "PIPE-TEST-QEMU: PASS target=$TARGET harts=$HARTS kernel=$(basename "$KERNEL")"
