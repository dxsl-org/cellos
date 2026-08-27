#!/usr/bin/env bash
# Build and run only the GetRandom SAS ownership fixture in a fresh QEMU guest.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

QEMU="${VICELL_QEMU:-qemu-system-riscv64}"
TARGET_BASE="target/getrandom-sas-test"
LOG_DIR="${GETRANDOM_SAS_QEMU_LOG_DIR:-$ROOT/.logs/getrandom-sas-qemu}"
mkdir -p "$LOG_DIR"
PRODUCTION_TARGET="$TARGET_BASE/production-relay"

build_production_tuple() {
    CARGO_TARGET_DIR="$PRODUCTION_TARGET" \
    RUSTFLAGS="-C relocation-model=pic" \
    cargo build --release \
        --target riscv64gc-unknown-none-elf \
        -Z build-std=core,alloc \
        --no-default-features \
        --features production-relay-image \
        -p cellos-kernel
    echo "PASS: GetRandom production tuple built without default features"
}


run_fixture() {
    local posture="$1"
    shift
    local target="$TARGET_BASE/$posture"
    local kernel="$target/riscv64gc-unknown-none-elf/release/cellos-kernel"
    local log
    log="$(mktemp "$LOG_DIR/qemu-$posture-XXXXXX.log")"

    EMBEDDED_OVERRIDE="kernel/src/embedded-test-hooks" \
    CARGO_TARGET_DIR="$target" \
    RUSTFLAGS="-C relocation-model=pic" \
    cargo build --release \
        --target riscv64gc-unknown-none-elf \
        -Z build-std=core,alloc \
        "$@" \
        -p cellos-kernel

    set +e
    timeout 30 "$QEMU" \
        -machine virt -m 256M -nographic -bios default -smp 2 -no-reboot \
        -kernel "$kernel" >"$log" 2>&1
    local qemu_status=$?
    set -e

    local terminal_count
    terminal_count="$(python3 -c '
import pathlib
import sys

terminal = b"[ INFO] S22-RV64-GETRANDOM-SAS: PASS"
lines = pathlib.Path(sys.argv[1]).read_bytes().replace(b"\0", b"").replace(b"\r", b"").splitlines()
print(sum(line == terminal for line in lines))
' "$log")"
    if [[ "$terminal_count" != "1" ]]; then
        echo "FAIL: expected exactly one GetRandom SAS terminal; posture=$posture; QEMU status $qemu_status; log=$log" >&2
        exit 1
    fi
    if [[ "$qemu_status" != "0" ]]; then
        echo "FAIL: QEMU exited with status $qemu_status; posture=$posture; log=$log" >&2
        exit 1
    fi
    if [[ "$posture" == "production-zero" ]] && grep -a -q "serving WEAK xorshift32" "$log"; then
        echo "FAIL: production-zero posture used dev-weak-rng; log=$log" >&2
        exit 1
    fi

    echo "PASS: GetRandom SAS fixture; posture=$posture; log=$log"
}

command -v "$QEMU" >/dev/null
build_production_tuple
run_fixture dev-weak --features getrandom-sas-test
run_fixture production-zero --no-default-features --features getrandom-sas-test
