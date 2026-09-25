#!/usr/bin/env bash
# Boot the `cpp-freestanding` smoke cell in QEMU and assert its markers.
#
# What this proves, in one run:
#   1. the cell links without libstdc++/libc++ (the build rejects a hosted runtime),
#   2. it is admitted as an `FFI`-class Tier 2 paged-domain cell,
#   3. C++ static constructors ran (`__init_array`),
#   4. virtual dispatch, templates, and `new`/`delete` work over the shim allocator,
#   5. the POSIX shim's C file ABI reads a file the host wrote through VFS.
#
# The cell is inserted into a *copy* of the disk image: the tracked image is not
# modified, and the copy is discarded on exit.
#
# Usage: scripts/qemu-cpp-smoke.sh [kernel-elf] [disk.img]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/cellos-kernel}"
DISK="${2:-disk_v3.img}"
TARGET="${CPP_SMOKE_TARGET:-riscv64gc-unknown-none-elf}"
CELL_ELF="target/${TARGET}/release/cpp-smoke"
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

echo "==> Building the cell ($TARGET)"
cargo build --release --target "$TARGET" -p app-cpp-smoke
[[ -f "$CELL_ELF" ]] || { echo "FAIL: cell ELF not found: $CELL_ELF" >&2; exit 1; }

# Hosted C++ runtime symbols must not be linked: the profile's promise is that
# the shim's C++ ABI layer is the whole runtime. This is a link-level assertion,
# not a source-level claim.
OBJDUMP_TOOL="${CPP_SMOKE_NM:-riscv64-unknown-elf-nm}"
if command -v "$OBJDUMP_TOOL" >/dev/null 2>&1; then
    if "$OBJDUMP_TOOL" "$CELL_ELF" 2>/dev/null | grep -qE "__cxa_throw|_Unwind_|__gxx_personality|_ZSt"; then
        echo "FAIL: hosted C++ runtime symbols are linked into $CELL_ELF" >&2
        "$OBJDUMP_TOOL" "$CELL_ELF" | grep -E "__cxa_throw|_Unwind_|__gxx_personality|_ZSt" | head >&2
        exit 1
    fi
    echo "==> Link check: no hosted C++ runtime symbols"
else
    echo "==> Link check skipped: $OBJDUMP_TOOL not found"
fi

# Sign with the dev key through the same F1/F5-checked route every image uses.
PYTHON_BIN="${PYTHON_BIN:-python3}" source scripts/lib-sign-cells.sh
sign_cells "$CELL_ELF"

WORKDIR="$(mktemp -d)"
DISK_COPY="$WORKDIR/disk-cpp-smoke.img"
RAW_LOG="$WORKDIR/qemu.raw.log"
LOG="$WORKDIR/qemu.log"
FIFO="$WORKDIR/stdin"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

echo "==> Preparing a private disk copy with /bin/cpp-smoke"
cp "$DISK" "$DISK_COPY"
python3 tools/add-cell-to-disk.py "$DISK_COPY" "/bin/cpp-smoke=$CELL_ELF"

echo "==> Booting (window ${BOOT_WINDOW}s)"
mkfifo "$FIFO"
timeout "$BOOT_WINDOW" "$QEMU" \
    -machine virt \
    -m 256M \
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

if ! wait_for "Cellos >" 60; then
    echo "FAIL: shell prompt not reached; see log below" >&2
    tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g' | tail -40 >&2
    exit 1
fi
sleep 1
printf 'cpp-smoke\n' >&3

if ! wait_for "CPP-SMOKE: PASS" 45; then
    echo "FAIL: cpp-smoke did not reach its PASS marker; see log below" >&2
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

require "\[domain\] admitted cell 'cpp-smoke'" "Tier 2 paged-domain admission"
require "\[cpp-smoke\] static-ctor marker=0xC0FFEE11" "C++ static constructor ran (__init_array)"
require "\[cpp-smoke\] virtual-dispatch area=37" "virtual dispatch through a base pointer"
require "\[cpp-smoke\] virtual-delete area=36" "virtual destructor + operator delete"
require "\[cpp-smoke\] templates total=64" "template instantiation"
require "\[cpp-smoke\] heap churn checksum=" "operator new/delete over the shim allocator"
require "\[cpp-smoke\] vfs client roundtrip bytes=" "VFS service round trip over typed IPC"
require "\[cpp-smoke\] c-abi read magic=ELF" "C file ABI read through the POSIX shim"
require "CPP-SMOKE: PASS" "cell PASS marker"

if grep -qa "\[cpp-smoke\] FAIL\|\[fault\] Cell" "$LOG"; then
    echo "FAIL: cell reported a failure stage or the kernel killed it" >&2
    grep -a "cpp-smoke\] FAIL\|\[fault\] Cell" "$LOG" | head >&2
    fail=1
fi

if [[ "$fail" -ne 0 ]]; then
    echo "==> cpp-smoke evidence failed; log tail:" >&2
    grep -a "cpp-smoke\|domain\]" "$LOG" | tail -20 >&2
    exit 1
fi

echo "CPP-SMOKE-QEMU: PASS target=$TARGET kernel=$(basename "$KERNEL")"
