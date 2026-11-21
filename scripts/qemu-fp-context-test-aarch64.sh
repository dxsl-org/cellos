#!/usr/bin/env bash
# Boot the per-task FP register file regression cell on AArch64 QEMU and assert
# its markers, on the EL1 host and on the EL2 (VHE) host.
#
# What this proves, per run:
#   1. two threads of one Tier 1 SAS cell (the class std cells run in; those are
#      built with `+neon`) each load their own pattern into q0-q31, FPCR and FPSR
#      before a Yield and read it back after, 200 rounds each, and see their own
#      values every round: the kernel saves and restores the FP/SIMD file per
#      task (`__switch_el1` on the EL1 host, `__switch_el2` on the VHE host);
#   2. the host regime is the one the run asked for (an EL2 run that silently
#      fell back to EL1 would not exercise `__switch_el2`);
#   3. with two CPUs, hart 1 is online, so a thread can resume on another CPU
#      than the one that saved its registers.
#
# The lane assembles its own image: on AArch64 a cell is reachable only from the
# kernel-embedded VIFS1, so the cell set of `qemu-aarch64-boot` plus
# /bin/fp-context-test is embedded into a kernel built in its own target
# directory (target/fp-context-aarch64), leaving the in-tree embedded pair and
# the default kernel untouched. Cells build in the default target directory, as
# in the CI job that runs this lane.
#
# Usage: scripts/qemu-fp-context-test-aarch64.sh [--runs "el1:1 el1:2 el2:1 el2:2"] [--no-build]
#   --runs      space-separated <host>:<cpus> list; host el1 (virt, cortex-a57)
#               or el2 (virt,virtualization=on, cortex-a76: VHE host); cpus 1 or 2
#   --no-build  boot the image the previous run built
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUNS="el1:1 el1:2 el2:1 el2:2"
BUILD=1
while [[ $# -gt 0 ]]; do
    case "$1" in
        --runs)
            RUNS="$2"
            shift 2
            ;;
        --no-build)
            BUILD=0
            shift
            ;;
        *)
            echo "FAIL: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
for run in $RUNS; do
    [[ "$run" =~ ^el[12]:[12]$ ]] || { echo "FAIL: bad run '$run' (want el1|el2 : 1|2)" >&2; exit 2; }
done

TARGET="aarch64-unknown-none-softfloat"
REL="target/$TARGET/release"
LANE_DIR="$ROOT/target/fp-context-aarch64"
EMBEDDED="$LANE_DIR/embedded"
KERNEL="$LANE_DIR/$TARGET/release/cellos-kernel"
IMAGE="$KERNEL.Image"
QEMU="${ViCell_QEMU_AARCH64:-qemu-system-aarch64}"
BOOT_WINDOW="${BOOT_WINDOW:-150}"
PROMPT_WINDOW="${PROMPT_WINDOW:-90}"
MARKER_WINDOW="${MARKER_WINDOW:-60}"

for tool in cargo python3 "$QEMU"; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found on PATH" >&2; exit 1; }
done

if [[ "$BUILD" == "1" ]]; then
    # littlefs (service-vfs) cross-compiles its C core with clang; bindgen needs
    # the aarch64 sysroot. Same values as the qemu-aarch64-boot cell build.
    export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
    export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
    export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat:---target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu}"

    echo "==> Building cells ($TARGET)"
    cargo build --release --target "$TARGET" -Z build-std=core,alloc \
        -p app-init -p app-cellbox -p service-vfs -p service-config -p service-input \
        -p app-fp-context-test

    CELLS=("$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input" "$REL/fp-context-test")
    for cell in "$REL/app-init" "${CELLS[@]}"; do
        [[ -s "$cell" ]] || { echo "FAIL: cell missing: $cell" >&2; exit 1; }
    done

    # An unsigned cell is domain-class and a production AArch64 kernel refuses
    # it; signed, fp-context-test's manifest selects Tier 1. init is embedded
    # separately and is not signed.
    PYTHON_BIN="${PYTHON_BIN:-python3}" source scripts/lib-sign-cells.sh
    sign_cells "${CELLS[@]}"

    echo "==> Assembling VIFS1 with /bin/fp-context-test"
    mkdir -p "$EMBEDDED"
    python3 tools/mkfat32.py --config-features input \
        "$EMBEDDED/kernel_fs.img.tmp" \
        "$REL/cellbox"         /bin/cellbox \
        "$REL/service-vfs"     /bin/vfs \
        "$REL/service-config"  /bin/config \
        "$REL/service-input"   /bin/input \
        "$REL/fp-context-test" /bin/fp-context-test
    mv "$EMBEDDED/kernel_fs.img.tmp" "$EMBEDDED/kernel_fs.img"
    cp "$REL/app-init" "$EMBEDDED/init"

    echo "==> Building the kernel (target dir $LANE_DIR)"
    EMBEDDED_OVERRIDE="$EMBEDDED" CARGO_TARGET_DIR="$LANE_DIR" \
        cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc
    bash scripts/build-aarch64-image.sh "$KERNEL" "$IMAGE"
fi
[[ -s "$IMAGE" ]] || { echo "FAIL: kernel Image missing: $IMAGE (run without --no-build)" >&2; exit 1; }

WORKDIR="$(mktemp -d)"
QEMU_PID=""
cleanup() {
    if [[ -n "$QEMU_PID" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

DISK="$WORKDIR/disk_arm_virt.img"
bash scripts/format-disk-arm.sh "$DISK" >/dev/null

clean_log() { tr -d '\000' < "$1" | sed 's/\x1b\[[0-9;]*m//g'; }

# Returns 0 when `pattern` appears in `log`, 1 on timeout, 2 when `stop` appears first.
wait_for() {
    local log="$1" pattern="$2" seconds="$3" stop="${4:-}" i
    for ((i = 0; i < seconds; i++)); do
        grep -qa "$pattern" "$log" && return 0
        [[ -n "$stop" ]] && grep -qa "$stop" "$log" && return 2
        sleep 1
    done
    return 1
}

overall=0
for run in $RUNS; do
    host="${run%%:*}"
    cpus="${run##*:}"
    case "$host" in
        el1)
            machine="virt,gic-version=2"
            cpu="cortex-a57"
            ;;
        el2)
            machine="virt,virtualization=on,gic-version=2"
            cpu="cortex-a76"
            ;;
    esac
    RAW_LOG="$WORKDIR/$host-$cpus.raw.log"
    LOG="$WORKDIR/$host-$cpus.log"
    FIFO="$WORKDIR/$host-$cpus.stdin"
    echo "==> Run $run: machine=$machine cpu=$cpu smp=$cpus"

    mkfifo "$FIFO"
    timeout "$BOOT_WINDOW" "$QEMU" \
        -machine "$machine" \
        -cpu "$cpu" \
        -smp "$cpus" \
        -m 256M \
        -nographic \
        -kernel "$IMAGE" \
        -drive "if=none,file=$DISK,format=raw,id=hd0" \
        -device virtio-blk-device,drive=hd0 \
        -device virtio-keyboard-device \
        -no-reboot \
        < "$FIFO" > "$RAW_LOG" 2>&1 &
    QEMU_PID=$!
    exec 3> "$FIFO"

    status=0
    if wait_for "$RAW_LOG" "Cellos >" "$PROMPT_WINDOW" "KERNEL PANIC"; then
        sleep 1
        printf 'fp-context-test\n' >&3
        wait_for "$RAW_LOG" "FP-CONTEXT-TEST: PASS" "$MARKER_WINDOW" \
            "\[fp-context-test\] FAIL\|\[fault\] Cell\|KERNEL PANIC" || status=$?
    else
        echo "FAIL[$run]: shell prompt not reached" >&2
        status=1
    fi
    exec 3>&-
    kill "$QEMU_PID" 2>/dev/null || true
    wait "$QEMU_PID" 2>/dev/null || true
    QEMU_PID=""
    clean_log "$RAW_LOG" > "$LOG"

    fail=0
    if [[ "$status" -ne 0 ]]; then
        echo "FAIL[$run]: fp-context-test did not pass (status $status)" >&2
        fail=1
    fi
    require() {
        if grep -qaF -- "$1" "$LOG"; then
            echo "PASS[$run]: $2"
        else
            echo "FAIL[$run]: $2 (missing: $1)" >&2
            fail=1
        fi
    }
    require "[fp-context-test] thread 0 ok rounds=" "thread 0 kept q0-q31/FPCR/FPSR across every yield"
    require "[fp-context-test] thread 1 ok rounds=" "thread 1 kept q0-q31/FPCR/FPSR across every yield"
    require "FP-CONTEXT-TEST: PASS" "cell PASS marker"
    if [[ "$host" == "el2" ]]; then
        require "[INFO] host regime: EL2 VHE" "EL2 VHE host (__switch_el2)"
    elif grep -qaF "[INFO] host regime: EL2" "$LOG"; then
        echo "FAIL[$run]: EL1 run booted an EL2 host" >&2
        fail=1
    fi
    if [[ "$cpus" == "2" ]]; then
        require "[smp] hart 1 online" "second CPU online"
    fi
    if [[ "$fail" -ne 0 ]]; then
        grep -a "fp-context-test\|\[fault\]\|PANIC\|host regime\|\[smp\]" "$LOG" | tail -20 >&2 || true
        tail -20 "$LOG" >&2
        overall=1
    fi
done

if [[ "$overall" -ne 0 ]]; then
    echo "FP-CONTEXT-TEST-AARCH64: FAIL runs=\"$RUNS\"" >&2
    exit 1
fi
echo "FP-CONTEXT-TEST-AARCH64: PASS runs=\"$RUNS\""
