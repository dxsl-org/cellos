#!/usr/bin/env bash
# Boot the per-task FP register file regression cell in QEMU and assert its markers.
#
# What this proves, in one run:
#   1. the cell is admitted as an `FFI`-class Tier 2 paged-domain cell,
#   2. two threads, each loading its own pattern into f0-f31 and fcsr before a
#      Yield and reading it back after, see their own values every round:
#      the kernel saves and restores FP state per task (RV64 `__switch`).
#
# `--harts 2` repeats the run with a second hart online, so a thread can resume
# on another hart than the one that saved its registers.
#
# Usage: scripts/qemu-fp-context-test.sh [--harts 1|2] [kernel-elf] [disk.img]
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
TARGET="riscv64gc-unknown-none-elf"
CELL_ELF="target/${TARGET}/release/fp-context-test"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"
BOOT_WINDOW="${BOOT_WINDOW:-120}"
MARKER_WINDOW="${MARKER_WINDOW:-60}"

for tool in "$QEMU" python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done
[[ -f "$DISK" ]] || { echo "FAIL: disk image not found: $DISK" >&2; exit 1; }
# Not rebuilt here: the kernel must embed the init/ramdisk of the image that
# produced `$DISK` (`scripts/gen-disk-ci.sh` builds both together); a plain
# `cargo build` falls back to the in-tree embedded init and never reaches a shell.
[[ -f "$KERNEL" ]] || { echo "FAIL: kernel ELF not found: $KERNEL (run scripts/gen-disk-ci.sh)" >&2; exit 1; }

echo "==> Building the cell ($TARGET)"
cargo build --release --target "$TARGET" -p app-fp-context-test
[[ -f "$CELL_ELF" ]] || { echo "FAIL: cell ELF not found: $CELL_ELF" >&2; exit 1; }

PYTHON_BIN="${PYTHON_BIN:-python3}" source scripts/lib-sign-cells.sh
sign_cells "$CELL_ELF"

WORKDIR="$(mktemp -d)"
DISK_COPY="$WORKDIR/disk-fp-context-test.img"
RAW_LOG="$WORKDIR/qemu.raw.log"
LOG="$WORKDIR/qemu.log"
FIFO="$WORKDIR/stdin"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

echo "==> Preparing a private disk copy with /bin/fp-context-test"
cp "$DISK" "$DISK_COPY"
python3 tools/add-cell-to-disk.py "$DISK_COPY" "/bin/fp-context-test=$CELL_ELF"

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

clean_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g'; }

# Returns 0 when `pattern` appears, 1 on timeout, 2 when `stop` appears first.
wait_for() {
    local pattern="$1" seconds="$2" stop="${3:-}" i
    for ((i = 0; i < seconds; i++)); do
        grep -qa "$pattern" "$RAW_LOG" && return 0
        [[ -n "$stop" ]] && grep -qa "$stop" "$RAW_LOG" && return 2
        sleep 1
    done
    return 1
}

if ! wait_for "Cellos >" 60; then
    echo "FAIL: shell prompt not reached" >&2
    clean_log | tail -40 >&2
    exit 1
fi
sleep 1
printf 'fp-context-test\n' >&3

status=0
wait_for "FP-CONTEXT-TEST: PASS" "$MARKER_WINDOW" "\[fp-context-test\] FAIL\|\[fault\] Cell" || status=$?
exec 3>&-
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true
clean_log > "$LOG"

if [[ "$status" -ne 0 ]]; then
    echo "FAIL: fp-context-test did not pass (status $status)" >&2
    grep -a "fp-context-test\|\[fault\] Cell" "$LOG" | tail -20 >&2
    exit 1
fi

fail=0
require() {
    if grep -qa "$1" "$LOG"; then
        echo "PASS: $2"
    else
        echo "FAIL: $2 (missing: $1)" >&2
        fail=1
    fi
}

require "\[domain\] admitted cell 'fp-context-test'" "Tier 2 paged-domain admission"
require "\[fp-context-test\] thread 0 ok rounds=" "thread 0 kept its f0-f31/fcsr across every yield"
require "\[fp-context-test\] thread 1 ok rounds=" "thread 1 kept its f0-f31/fcsr across every yield"
require "FP-CONTEXT-TEST: PASS" "cell PASS marker"

if [[ "$HARTS" == "2" ]]; then
    require "\[smp\] hart 1 online" "second hart online"
fi

if [[ "$fail" -ne 0 ]]; then
    grep -a "fp-context-test\|domain\]" "$LOG" | tail -20 >&2
    exit 1
fi

echo "FP-CONTEXT-TEST-QEMU: PASS target=$TARGET harts=$HARTS kernel=$(basename "$KERNEL")"
