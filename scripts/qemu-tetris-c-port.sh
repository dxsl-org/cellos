#!/usr/bin/env bash
# Build and visually-admit the class-A C reference port in a private QEMU disk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
TARGET="${TETRIS_C_TARGET:-riscv64gc-unknown-none-elf}"
KERNEL="target/$TARGET/release/cellos-kernel"
CELL="target/$TARGET/release/tetris-c"
DISK="${2:-disk_v3.img}"
QEMU="${ViCell_QEMU:-qemu-system-riscv64}"

for tool in "$QEMU" python3; do command -v "$tool" >/dev/null || { echo "FAIL: missing $tool" >&2; exit 1; }; done
[[ -f "$DISK" ]] || { echo "FAIL: missing disk image $DISK" >&2; exit 1; }
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cellos-kernel
cargo build --release --target "$TARGET" -p tetris-c
PYTHON_BIN=python3 source scripts/lib-sign-cells.sh
sign_cells "$CELL"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
cp "$DISK" "$WORK/disk.img"
python3 tools/add-cell-to-disk.py "$WORK/disk.img" "/bin/tetris-c=$CELL"
mkfifo "$WORK/in"
timeout 90 "$QEMU" -machine virt -m 256M -nographic -bios default -kernel "$KERNEL" \
  -drive "file=$WORK/disk.img,format=raw,id=hd0,if=none" -device virtio-blk-device,drive=hd0 \
  -device virtio-keyboard-device -device virtio-gpu-device <"$WORK/in" >"$WORK/log" 2>&1 &
PID=$!; exec 3>"$WORK/in"
wait_for() { local p=$1; for _ in $(seq 1 60); do grep -qa "$p" "$WORK/log" && return 0; sleep 1; done; return 1; }
wait_for 'Cellos >' || { cat "$WORK/log" >&2; exit 1; }
printf 'tetris-c\n' >&3
wait_for "\[domain\] admitted cell 'tetris-c'" || { cat "$WORK/log" >&2; exit 1; }
wait_for 'TETRIS-PORT: READY' || { cat "$WORK/log" >&2; exit 1; }
exec 3>&-; wait "$PID" 2>/dev/null || true
echo 'TETRIS-C-PORT-QEMU: PASS'
