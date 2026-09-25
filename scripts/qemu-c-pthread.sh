#!/usr/bin/env bash
# Build and run the Tier-2 C pthread witness in an isolated QEMU disk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
TARGET="${CELLOS_TARGET:-riscv64gc-unknown-none-elf}"
KERNEL="target/$TARGET/release/cellos-kernel"
CELL="target/$TARGET/release/c-pthread"
DISK="${1:-disk_v3.img}"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"

for tool in "$QEMU" python3; do command -v "$tool" >/dev/null || {
    echo "FAIL: missing $tool" >&2
    exit 1
}; done
[[ -f "$DISK" ]] || { echo "FAIL: missing disk image $DISK" >&2; exit 1; }

cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cellos-kernel
cargo build --release --target "$TARGET" -p c-pthread
PYTHON_BIN=python3 source scripts/lib-sign-cells.sh
sign_cells "$CELL"

WORK="$(mktemp -d)"
# Optional evidence capture: EVIDENCE_DIR=<dir> keeps the raw QEMU log and the
# stripped log, which is what a published cost/evidence record cites.
EVIDENCE_DIR="${EVIDENCE_DIR:-}"
cleanup() {
    # A trap must not mask the script's own exit status: capture evidence when
    # the log exists, and never fail the cleanup.
    if [[ -n "$EVIDENCE_DIR" && -f "$WORK/log" ]]; then
        mkdir -p "$EVIDENCE_DIR"
        cp "$WORK/log" "$EVIDENCE_DIR/c-pthread-qemu.log"
        tr -d '\000' < "$WORK/log" | sed 's/\x1b\[[0-9;]*m//g' > "$EVIDENCE_DIR/c-pthread-qemu.txt"
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT
cp "$DISK" "$WORK/disk.img"
python3 tools/add-cell-to-disk.py "$WORK/disk.img" "/bin/c-pthread=$CELL"
mkfifo "$WORK/in"
timeout 90 "$QEMU" -machine virt -m 256M -nographic -bios default -kernel "$KERNEL" \
    -drive "file=$WORK/disk.img,format=raw,id=hd0,if=none" -device virtio-blk-device,drive=hd0 \
    <"$WORK/in" >"$WORK/log" 2>&1 &
PID=$!
exec 3>"$WORK/in"
wait_for() {
    local pattern=$1
    for _ in $(seq 1 60); do
        grep -qa "$pattern" "$WORK/log" && return 0
        sleep 1
    done
    return 1
}
wait_for 'Cellos >' || { cat "$WORK/log" >&2; exit 1; }
printf 'c-pthread\n' >&3
wait_for "\[domain\] admitted cell 'c-pthread'" || { cat "$WORK/log" >&2; exit 1; }
wait_for 'C-PTHREAD-QEMU: PASS' || { cat "$WORK/log" >&2; exit 1; }
exec 3>&-
wait "$PID" 2>/dev/null || true
echo 'C-PTHREAD-QEMU: PASS'
