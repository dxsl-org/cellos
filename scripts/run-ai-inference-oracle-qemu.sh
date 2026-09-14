#!/usr/bin/env bash
# Boot the RV64 image in QEMU and run the Spec 24 AI inference oracle.
#
# Builds a private, isolated image (no shared build artifacts are touched):
#   * VIFS1 ramdisk carries the bootstrap cells, /bin/ai, /bin/ai-test, and the
#     deterministic tiny model fixture (models/tiny-llama-64.gguf), so the oracle
#     needs no disk cell-store.
#   * An empty VirtIO-BLK disk is attached so the block driver and VFS come up the
#     same way they do in a normal image.
#
# The oracle cell is spawned by init (`/bin/ai-test`) and prints `[ai-test] PASS`
# only after the service reproduced the reference token ids and embedding.
#
# Usage: scripts/run-ai-inference-oracle-qemu.sh [--boot-timeout SECONDS]
# Exit codes: 0 PASS, 1 FAIL (oracle or marker missing), 2 precondition missing.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="riscv64gc-unknown-none-elf"
QEMU_BIN="${ViCell_QEMU:-qemu-system-riscv64}"
BOOT_TIMEOUT=300

while [[ $# -gt 0 ]]; do
    case "$1" in
        --boot-timeout)
            BOOT_TIMEOUT="$2"
            shift 2
            ;;
        *)
            echo "usage: $0 [--boot-timeout SECONDS]" >&2
            exit 2
            ;;
    esac
done

for tool in cargo rustc mktemp truncate timeout grep "$QEMU_BIN"; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "FAIL: required tool not found: $tool" >&2
        exit 2
    }
done

if command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: Python 3 is required (tools/mkfat32.py)" >&2
    exit 2
fi

MODEL="models/tiny-llama-64.gguf"
[[ -s "$MODEL" ]] || {
    echo "FAIL: model fixture missing — run scripts/gen-ai-test-model.py" >&2
    exit 2
}

# The fixture must match the golden file the oracle cell was compiled against.
"$PYTHON_BIN" scripts/gen-ai-test-model.py --check >/dev/null || {
    echo "FAIL: model fixture is stale — re-run scripts/gen-ai-test-model.py" >&2
    exit 2
}

# A real checkpoint may be deployed instead of the fixture: the oracle decides which scenario to
# run from the vocabulary the service reports, so the image simply carries whichever model is
# named here. `scripts/fetch-ai-test-model.sh` prints the path of the pinned one.
REAL_MODEL="${CELLOS_AI_REAL_MODEL:-}"
if [[ -n "$REAL_MODEL" ]]; then
    [[ -s "$REAL_MODEL" ]] || {
        echo "FAIL: CELLOS_AI_REAL_MODEL points at a missing file: $REAL_MODEL" >&2
        exit 2
    }
    MODEL="$REAL_MODEL"
    echo "[ai-oracle] deploying real checkpoint: $(basename "$REAL_MODEL")"
fi

source scripts/lib-run-scoped-workspace.sh
cleanup_stale_run_scoped_workspaces "${TMPDIR:-/tmp}/cellos-ai-oracle"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/cellos-ai-oracle.XXXXXXXX")"
cleanup() {
    local status=$?
    trap - EXIT
    rm -rf -- "$WORK"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
# Ownership marker: `cleanup_stale_run_scoped_workspaces` (sourced above) only reclaims
# a directory whose owner pid is gone, so a killed run cannot leave a private
# workspace behind forever.
chmod 0700 "$WORK"
printf '%s\n' "$$" > "$WORK/owner.pid"

export CARGO_TARGET_DIR="$WORK/target"
export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="${CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS:--C relocation-model=pic}"

EMBEDDED="$WORK/embedded"
DISK="$WORK/disk.img"
LOG="$WORK/serial.log"
mkdir -p "$EMBEDDED"
truncate -s 32M "$DISK"

# The kernel's default feature set enables `signing-required` and `native-domains`:
# an UNSIGNED cell is admitted to a Tier 2 paged domain, and on this image the
# paged-domain path faults inside the kernel before the shell is reachable. Every
# QEMU image lane therefore signs its cells with the dev key through the same
# reviewed wrapper (scripts/lib-sign-cells.sh -> scripts/cellos-sign, which runs the
# F1/F5 checks). Signing here is image assembly, not a production-gate claim.

# A checkpoint larger than the default arena needs the service's `large-arena` feature; the default
# build keeps every other image lean.
AI_FEATURES=()
if [[ -n "$REAL_MODEL" ]]; then
    model_bytes=$(wc -c < "$REAL_MODEL")
    if [[ "$model_bytes" -gt $((8 * 1024 * 1024)) ]]; then
        AI_FEATURES=(--features service-ai/large-arena)
        echo "[ai-oracle] model is $((model_bytes / 1048576)) MiB — building the service with large-arena"
    fi
fi

echo "[ai-oracle] building RV64 cells"
cargo build --quiet --locked --release --target "$TARGET" \
    -p app-init -p app-shell -p service-vfs -p service-config -p service-platform \
    -p driver-virtio-blk -p ai-test
cargo build --quiet --locked --release --target "$TARGET" -p service-ai "${AI_FEATURES[@]}"

REL="$CARGO_TARGET_DIR/$TARGET/release"
CELL_BINARIES=(
    "$REL/app-init"
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/platform"
    "$REL/driver-virtio-blk"
    "$REL/service-ai"
    "$REL/ai-test"
)
for binary in "${CELL_BINARIES[@]}"; do
    [[ -s "$binary" ]] || {
        echo "FAIL: cell build did not produce: $binary" >&2
        exit 1
    }
done

"$PYTHON_BIN" scripts/sign-policy.py --out "$WORK/POLICY.BIN" >/dev/null
printf 'Cellos-AI-Oracle\n' > "$WORK/hostname"
printf 'Cellos AI inference oracle image\n' > "$WORK/readme.txt"

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "[ai-oracle] signing cells (dev key, F1/F5 checked)"
sign_cells "${CELL_BINARIES[@]}"

echo "[ai-oracle] assembling VIFS1"
"$PYTHON_BIN" tools/mkfat32.py \
    "$EMBEDDED/kernel_fs.img" \
    "$REL/app-init"            /bin/init \
    "$REL/app-shell"           /bin/shell \
    "$REL/service-vfs"         /bin/vfs \
    "$REL/service-config"      /bin/config \
    "$REL/platform"            /bin/platform \
    "$REL/driver-virtio-blk"   /bin/block \
    "$REL/service-ai"          /bin/ai \
    "$REL/ai-test"             /bin/ai-test \
    "$MODEL"                   /bin/ai-model.gguf \
    "$WORK/hostname"           /etc/hostname \
    "$WORK/readme.txt"         /readme.txt \
    "$WORK/POLICY.BIN"         /POLICY.BIN
cp -- "$REL/app-init" "$EMBEDDED/init"

"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
for required in "LFN 'ai'" "LFN 'ai-test'" "LFN 'ai-model.gguf'" "LFN 'vfs'"; do
    grep -a -Fq -- "$required" "$WORK/fat-layout.txt" || {
        echo "FAIL: VIFS1 is missing $required" >&2
        exit 1
    }
done

echo "[ai-oracle] building RV64 kernel"
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --quiet --locked --release --target "$TARGET" -p cellos-kernel
KERNEL="$REL/cellos-kernel"
[[ -s "$KERNEL" ]] || {
    echo "FAIL: kernel build produced no image" >&2
    exit 1
}

echo "[ai-oracle] booting QEMU (timeout ${BOOT_TIMEOUT}s)"
timeout --foreground "$BOOT_TIMEOUT" "$QEMU_BIN" \
    -machine virt \
    -m 256M \
    -smp 1 \
    -nographic \
    -bios default \
    -kernel "$KERNEL" \
    -monitor none \
    -drive "file=$DISK,format=raw,if=none,id=hd0" \
    -device virtio-blk-device,drive=hd0 \
    > "$LOG" 2>&1 &
QEMU_PID=$!

status=1
for _ in $(seq 1 "$BOOT_TIMEOUT"); do
    if grep -a -Fq "[ai-test] PASS" "$LOG" 2>/dev/null; then
        status=0
        break
    fi
    if grep -a -Fq "[ai-test] FAIL" "$LOG" 2>/dev/null; then
        status=1
        break
    fi
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        status=1
        break
    fi
    sleep 1
done

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

EVIDENCE_DIR="$ROOT/.agents/260913-2002-g2-level-a-ai-inference/evidence"
mkdir -p "$EVIDENCE_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="$EVIDENCE_DIR/ai-oracle-$STAMP.log"
cp -- "$LOG" "$EVIDENCE"
echo "[ai-oracle] serial log: $EVIDENCE"

if [[ "$status" -ne 0 ]]; then
    echo "FAIL: the AI inference oracle did not pass"
    grep -a -F -- "[ai]" "$LOG" | tail -20 || true
    grep -a -F -- "[ai-test]" "$LOG" | tail -20 || true
    exit 1
fi

MARKERS=(
    "[ai] model ready:"
    "[ai-test] PASS"
)
if [[ -n "$REAL_MODEL" ]]; then
    # With a real checkpoint deployed the oracle asserts text-likeness and round-trips instead of
    # the fixture's golden ids; both paths must still reach every earlier scenario.
    MARKERS+=(
        "[ai-test] real model continuation:"
        "[ai-test] abandoned session released; service still serving"
        "[ai-test] prompt stream matched:"
        "[ai-test] model vocab "
    )
else
    MARKERS+=(
        "[ai-test] greedy ids matched:"
        "[ai-test] embedding matched:"
        "[ai-test] abandoned session released; service still serving"
        "[ai-test] prompt stream matched:"
    )
fi

for marker in "${MARKERS[@]}"; do
    grep -a -Fq -- "$marker" "$LOG" || {
        echo "FAIL: missing marker: $marker" >&2
        exit 1
    }
done

if grep -a -Fq -- "Cell fault" "$LOG" || grep -a -Fq -- "PANIC" "$LOG"; then
    echo "FAIL: the run contained a cell fault or kernel panic" >&2
    exit 1
fi

grep -a -F -- "[ai" "$LOG"
echo "[ai-oracle] PASS"
