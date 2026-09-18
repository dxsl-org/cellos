#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# run-std-smoke-qemu.sh: Build and boot Tier 1 Rust std cell (std-smoke) in QEMU.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ARCH="${1:-riscv64}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-45}"
QEMU_BIN="${ViCell_QEMU:-qemu-system-riscv64}"

echo "==> Tier 1 Rust std QEMU Runner ($ARCH)"

for tool in cargo rustc mktemp truncate timeout grep "$QEMU_BIN"; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "FAIL: required tool not found: $tool" >&2
        exit 2
    }
done

if command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN=python3
else
    echo "FAIL: python3 not found" >&2
    exit 2
fi

WORK="$(mktemp -d -t cellos-std-smoke-XXXXXX)"
cleanup() {
    if [[ -n "${QEMU_PID:-}" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

TARGET_CELL="targets/riscv64gc-unknown-cellos.json"
TARGET_BOOTSTRAP="riscv64gc-unknown-none-elf"

echo "==> Step 1: Building sysroot overlay and std-smoke cell..."
bash scripts/build-cellos-sysroot.sh riscv64gc-unknown-cellos

STAGING_DIR="$(pwd)/target/cellos-rust-src/library"
export __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR"

cargo +nightly-2026-05-01 build --release \
    --manifest-path cells/demos/std-smoke/Cargo.toml \
    -Z build-std=core,alloc,std,panic_abort \
    -Z build-std-features=compiler-builtins-mem \
    -Z json-target-spec \
    --target "$TARGET_CELL"

STD_SMOKE_BIN="cells/demos/std-smoke/target/riscv64gc-unknown-cellos/release/std-smoke"
if [[ ! -s "$STD_SMOKE_BIN" ]]; then
    echo "FAIL: std-smoke binary not found at $STD_SMOKE_BIN" >&2
    exit 1
fi

echo "==> Step 2: Building bootstrap cells..."
export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="-C relocation-model=pic"

cargo build --release --target "$TARGET_BOOTSTRAP" \
    -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-vfs -p service-config -p service-platform -p driver-virtio-blk

REL="target/$TARGET_BOOTSTRAP/release"
BOOTSTRAP_CELLS=(
    "$REL/app-init"
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/platform"
    "$REL/driver-virtio-blk"
)
for bin in "${BOOTSTRAP_CELLS[@]}"; do
    [[ -s "$bin" ]] || { echo "FAIL: bootstrap cell missing: $bin" >&2; exit 1; }
done

echo "==> Step 3: Signing cells with dev key..."
source scripts/lib-sign-cells.sh
sign_cells "${BOOTSTRAP_CELLS[@]}" "$STD_SMOKE_BIN"

echo "==> Step 4: Assembling VIFS1 ramdisk..."
EMBEDDED="$WORK/embedded"
mkdir -p "$EMBEDDED"

"$PYTHON_BIN" scripts/sign-policy.py --out "$WORK/POLICY.BIN" >/dev/null
printf 'Cellos-Rust-Std\n' > "$WORK/hostname"
printf 'Cellos Tier 1 Rust std runtime verification\n' > "$WORK/readme.txt"

"$PYTHON_BIN" tools/mkfat32.py \
    "$EMBEDDED/kernel_fs.img" \
    "$REL/app-init"          /bin/init \
    "$REL/app-shell"         /bin/shell \
    "$REL/service-vfs"       /bin/vfs \
    "$REL/service-config"    /bin/config \
    "$REL/platform"          /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$STD_SMOKE_BIN"         /bin/std-smoke \
    "$WORK/hostname"         /etc/hostname \
    "$WORK/readme.txt"       /readme.txt \
    "$WORK/POLICY.BIN"       /POLICY.BIN
cp -- "$REL/app-init" "$EMBEDDED/init"

"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
if ! grep -a -Fq "LFN 'std-smoke'" "$WORK/fat-layout.txt"; then
    echo "FAIL: VIFS1 is missing std-smoke" >&2
    exit 1
fi

echo "==> Step 5: Building kernel with embedded test image..."
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release \
    --target "$TARGET_BOOTSTRAP" \
    -Z build-std=core,alloc \
    -p cellos-kernel

KERNEL="$REL/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel missing at $KERNEL" >&2; exit 1; }

echo "==> Step 6: Launching QEMU..."
LOG="$WORK/serial.log"
DISK="$WORK/disk.img"
truncate -s 16M "$DISK"

QEMU_ARGS=(
    -machine virt
    -m 256M
    -smp 1
    -nographic
    -monitor none
    -bios default
    -kernel "$KERNEL"
    -drive "file=$DISK,format=raw,if=none,id=hd0"
    -device virtio-blk-device,drive=hd0
    -device virtio-rng-device
)

timeout --foreground "$BOOT_TIMEOUT" "$QEMU_BIN" "${QEMU_ARGS[@]}" > "$LOG" 2>&1 &
QEMU_PID=$!

echo "==> Waiting for std-smoke PASS marker..."
status=1
for _ in $(seq 1 "$BOOT_TIMEOUT"); do
    if grep -a -Fq "[std-smoke] PASS: All Rust std PAL invariants verified successfully!" "$LOG" 2>/dev/null; then
        status=0
        break
    fi
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done

if [[ "$status" -eq 0 ]]; then
    echo "==> [SUCCESS] std-smoke executed and verified in QEMU!"
    echo "--- Serial Output Summary ---"
    grep -a -F "[std-smoke]" "$LOG" || true
    exit 0
else
    echo "FAIL: std-smoke did not report PASS within ${BOOT_TIMEOUT}s" >&2
    echo "--- Complete Log Tail ---"
    tail -n 40 "$LOG" || true
    exit 1
fi
