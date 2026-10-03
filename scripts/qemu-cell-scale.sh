#!/usr/bin/env bash
# qemu-cell-scale.sh — measure how many Cells actually fit (D5 gate).
#
# WHY
# ---
# Decision D5 (Spec 19 §3) targets a per-request server profile of "thousands of
# very light cells". The 2026-07-31 measurement found the ceiling was not per-cell
# cost but the 190 MiB hardcoded RISC-V fallback memory map; the DTB memory-node
# parse landed since, and the gate `N=64/128/256/512` has not been re-measured
# (docs/roadmap/beam-parity-backend-roadmap.md §2.2, plan-portfolio.md:33-35).
#
# The production ceilings (`MAX_CELLS` 64, `MAX_SLOTS` 512) bind long before
# memory does, so this lane builds the kernel with `--features
# cell-scale-experiment` (MAX_CELLS/MAX_SLOTS 4096) and spawns parked
# `/bin/bench-probe` children from `/bin/capacity-probe` until the kernel refuses.
# A refusal at exactly 64 or 512 means the experiment ceilings did NOT take
# effect — that is a failure, not a capacity result. A refusal at the experiment
# MAX_SLOTS is reported as a constant bound rather than as a memory measurement.
#
# PREREQUISITE
# ------------
# The probe must be in the kernel's VIFS1 ramdisk: the shell reaches
# capability-bearing cells (`/bin/capacity-probe` carries SpawnCap) only through
# the Path route, which resolves in VIFS1 — the block cell-store is never probed
# on RV64. Build the images once with:
#
#   CELLOS_INCLUDE_CAPACITY_PROBE=1 bash scripts/gen-disk-ci.sh
#
# The experiment kernel is built into a private CARGO_TARGET_DIR: the production
# `cellos-kernel` on the shared target path is never replaced by an unqualified
# build. The disk is copied; the tracked image is not modified.
#
# Usage: scripts/qemu-cell-scale.sh [--memory 2G] [--window 1200]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MEMORY="${CELL_SCALE_MEMORY:-2G}"
WINDOW="${CELL_SCALE_WINDOW:-1200}"
# M heavy cells resident while the light sweep runs (D5 gate §2.3).
HEAVY="${CELL_SCALE_HEAVY:-0}"
TARGET="${CELL_SCALE_TARGET:-riscv64gc-unknown-none-elf}"
TARGET_DIR="${CELL_SCALE_TARGET_DIR:-build/cell-scale-target}"
# Kept in step with `cell-scale-experiment` in kernel/Cargo.toml; a refusal at
# this count is a constant bound, not a memory one.
EXPERIMENT_MAX_SLOTS="${CELL_SCALE_MAX_SLOTS:-4096}"
DISK="${CELL_SCALE_DISK:-disk_v3.img}"
VIFS1="${CELL_SCALE_VIFS1:-kernel/src/embedded/kernel_fs.img}"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"
KERNEL="$TARGET_DIR/$TARGET/release/cellos-kernel"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --memory) MEMORY="${2:?--memory needs a value}"; shift 2 ;;
        --heavy) HEAVY="${2:?--heavy needs a value}"; shift 2 ;;
        --window) WINDOW="${2:?--window needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 1 ;;
    esac
done

for tool in "$QEMU" python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done
[[ -f "$DISK" ]] || { echo "FAIL: disk image not found: $DISK (run scripts/gen-disk-ci.sh)" >&2; exit 1; }
[[ -f "$VIFS1" ]] || { echo "FAIL: embedded VIFS1 not found: $VIFS1" >&2; exit 1; }

if ! python3 tools/inspect_fat.py "$VIFS1" | grep -q "capacity-probe"; then
    echo "FAIL: $VIFS1 does not carry /bin/capacity-probe." >&2
    echo "      The shell reaches capability-bearing cells only through the Path" >&2
    echo "      route, which resolves in VIFS1 (the block cell-store is never probed)." >&2
    echo "      Build the images once with:" >&2
    echo "        CELLOS_INCLUDE_CAPACITY_PROBE=1 bash scripts/gen-disk-ci.sh" >&2
    exit 1
fi

echo "==> Building the cell-scale experiment kernel (features cell-scale-experiment)"
RUSTFLAGS="-C relocation-model=pic" CARGO_TARGET_DIR="$TARGET_DIR" \
    cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p cellos-kernel --features cell-scale-experiment
[[ -f "$KERNEL" ]] || { echo "FAIL: kernel not built: $KERNEL" >&2; exit 1; }

WORKDIR="${CELL_SCALE_LOGDIR:-build/cell-scale-$(date +%s)}"
mkdir -p "$WORKDIR"
DISK_COPY="$WORKDIR/disk-cell-scale.img"
RAW_LOG="$WORKDIR/qemu.raw.log"
LOG="$WORKDIR/qemu.log"
FIFO="$WORKDIR/stdin"
cleanup() {
    if [[ -n "${QEMU_PID:-}" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    # Keep the logs (the count and the failure mode are the evidence); drop only
    # the 577 MB disk copy.
    rm -f "$DISK_COPY" "$FIFO"
}
trap cleanup EXIT INT TERM

echo "==> Booting (memory $MEMORY, window ${WINDOW}s)"
cp "$DISK" "$DISK_COPY"
mkfifo "$FIFO"
timeout "$WINDOW" "$QEMU" \
    -machine virt \
    -m "$MEMORY" \
    -nographic \
    -bios default \
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

normalize_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"; }

fail_with_log() {
    echo "FAIL: $1" >&2
    normalize_log
    echo "      log kept at $LOG" >&2
    tail -40 "$LOG" >&2
    exit 1
}

if ! wait_for "Cellos >" 90; then
    fail_with_log "shell prompt not reached"
fi
sleep 1

echo "==> Running capacity-probe (spawn parked children until the kernel refuses)"
printf 'capacity-probe %s\n' "$HEAVY" >&3

if ! wait_for "MEMINFO_DENIED" 30; then
    fail_with_log "probe did not report the MemInfo denial"
fi

if ! wait_for "OOM_TYPED\|OOM_NOT_REACHED\|SPAWN_GENERIC_ERROR\|command not found\|allocation error" "$WINDOW"; then
    normalize_log
    progress="$(grep -o 'parked count=[0-9]*' "$LOG" | tail -1 || true)"
    echo "FAIL: sweep did not reach a verdict within ${WINDOW}s (${progress:-no progress line})" >&2
    echo "      log kept at $LOG" >&2
    tail -40 "$LOG" >&2
    exit 1
fi
normalize_log

# A kernel-heap exhaustion halts the kernel in `alloc_error_handler` (a `wfi`
# loop), so it must be reported as the failure mode it is — never as a count.
if grep -qa "allocation error" "$LOG"; then
    progress="$(grep -o 'parked count=[0-9]*' "$LOG" | tail -1 || true)"
    echo "FAIL: kernel heap exhausted mid-sweep (${progress:-unknown count}); the alloc" >&2
    echo "      error handler halts the kernel, so no spawn OOM was ever reported." >&2
    echo "      log kept at $LOG" >&2
    grep -a -B4 "allocation error" "$LOG" | tail -20 >&2
    exit 1
fi
if grep -qa "command not found" "$LOG"; then
    fail_with_log "the shell could not launch /bin/capacity-probe"
fi
if grep -qa "SPAWN_GENERIC_ERROR" "$LOG"; then
    fail_with_log "spawn failed with a non-OOM error"
fi
if grep -qa "OOM_NOT_REACHED" "$LOG"; then
    fail_with_log "the sweep hit its own bound without the kernel refusing — raise SPAWN_BOUND"
fi

count="$(grep -o 'OOM_TYPED count=[0-9]*' "$LOG" | tail -1 | grep -o '[0-9]*')"
[[ -n "$count" ]] || fail_with_log "OOM_TYPED line carried no count"

# A heavy run must prove its heavy cells became heavy: the grant line is the only
# evidence that M cells are resident with their footprint, not merely spawned.
if [[ "$HEAVY" -gt 0 ]]; then
    resident="$(grep -c 'heavy resident: grant=' "$LOG" || true)"
    if [[ "$resident" -lt "$HEAVY" ]]; then
        fail_with_log "only $resident of $HEAVY heavy cells reported a resident grant"
    fi
fi

if [[ "$count" -eq 64 || "$count" -eq 512 ]]; then
    fail_with_log "refusal at exactly $count — the experiment ceilings did not take effect (production MAX_CELLS=64 / MAX_SLOTS=512)"
fi

# A refusal at the experiment MAX_SLOTS is a constant bound, not a memory one.
# Say so explicitly: a constant-bound count must never read as a capacity result.
if [[ "$count" -eq "$EXPERIMENT_MAX_SLOTS" ]]; then
    echo "CELL-SCALE: parked=$count memory=$MEMORY heavy=$HEAVY bound=va-slots(MAX_SLOTS=$EXPERIMENT_MAX_SLOTS) kernel=cell-scale-experiment target=$TARGET"
    echo "WARN: the VA-slot ceiling bound before memory did — raise MAX_SLOTS to measure the memory ceiling" >&2
    exit 0
fi

echo "CELL-SCALE: parked=$count memory=$MEMORY heavy=$HEAVY bound=memory kernel=cell-scale-experiment target=$TARGET"
