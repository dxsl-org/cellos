#!/usr/bin/env bash
# Build and run only the GetRandom SAS ownership fixture in a fresh QEMU guest.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

QEMU="${VICELL_QEMU:-qemu-system-riscv64}"
TARGET="target/getrandom-sas-test"
KERNEL="$TARGET/riscv64gc-unknown-none-elf/release/cellos-kernel"
LOG_DIR="${GETRANDOM_SAS_QEMU_LOG_DIR:-$ROOT/.logs/getrandom-sas-qemu}"
mkdir -p "$LOG_DIR"
LOG="$(mktemp "$LOG_DIR/qemu-XXXXXX.log")"

command -v "$QEMU" >/dev/null
EMBEDDED_OVERRIDE="kernel/src/embedded-test-hooks" \
CARGO_TARGET_DIR="$TARGET" \
RUSTFLAGS="-C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    --features getrandom-sas-test \
    -p cellos-kernel

set +e
timeout 30 "$QEMU" \
    -machine virt -m 256M -nographic -bios default -smp 2 -no-reboot \
    -kernel "$KERNEL" >"$LOG" 2>&1
QEMU_STATUS=$?
set -e

terminal_count="$(python3 -c '
import pathlib
import sys

terminal = b"[ INFO] S22-RV64-GETRANDOM-SAS: PASS"
lines = pathlib.Path(sys.argv[1]).read_bytes().replace(b"\0", b"").replace(b"\r", b"").splitlines()
print(sum(line == terminal for line in lines))
' "$LOG")"
if [[ "$terminal_count" != "1" ]]; then
    echo "FAIL: expected exactly one GetRandom SAS terminal; QEMU status $QEMU_STATUS; log=$LOG" >&2
    exit 1
fi

if [[ "$QEMU_STATUS" != "0" ]]; then
    echo "FAIL: QEMU exited with status $QEMU_STATUS; log=$LOG" >&2
    exit 1
fi

echo "PASS: GetRandom SAS fixture; log=$LOG"
