#!/usr/bin/env bash
# Forward-edge CFI lane (AArch64 BTI): the authority on `cfi-test`'s verdict.
#
# Builds `cfi-test` with BTI landing pads in its own cargo invocation
# (`-Z branch-protection=bti` must cover core/alloc/compiler_builtins too, or
# the linker drops the GNU property note), checks that the image carries
# `GNU_PROPERTY_AARCH64_FEATURE_1_AND` BTI and the baseline cells do not, embeds
# it beside the default aarch64 lane's cells (EMBEDDED_OVERRIDE, nothing in the
# tree is rewritten), boots QEMU, runs `cfi-test` from the shell, then judges:
#
#   boot log `CFI: BTI enabled`  (FEAT_BTI present, e.g. `-cpu max`):
#     PASS only if the loader armed the image, the control call returned, the
#     violation faulted with EC 0x0D at the printed non-landing-pad address,
#     no other cell was armed, and the shell (a non-opted-in cell) still runs
#     a command afterwards. A returned violation (`cfi-test: SKIP`) is a FAIL:
#     EL0 cannot read ID_AA64PFR1_EL1, so only this harness can tell.
#   boot log `CFI: BTI unavailable` (e.g. `-cpu cortex-a57`):
#     PASS only if the loader logged the unenforced opt-in and the cell reported
#     the returned violation as SKIP.
#
# Usage: scripts/qemu-cfi-test.sh
# Environment:
#   QEMU_CPU      default max (TCG implements FEAT_BTI); cortex-a57 = no BTI
#   CFI_WORK      work directory (default /tmp/cellos-cfi); cargo target dirs,
#                 the embedded image, the disk copy and qemu-cfi.log live here
#   BOOT_WINDOW   seconds for the whole QEMU run (default 150)
#   DISK          disk image to copy (default disk_arm_virt.img; created with
#                 scripts/format-disk-arm.sh when absent)
#
# Bash only. Needs qemu-system-aarch64, clang, aarch64-linux-gnu sysroot,
# readelf and python3 with `cryptography` (cell signing).

set -euo pipefail

QEMU_CPU="${QEMU_CPU:-max}"
WORK="${CFI_WORK:-/tmp/cellos-cfi}"
BOOT_WINDOW="${BOOT_WINDOW:-150}"
DISK_SRC="${DISK:-disk_arm_virt.img}"
TARGET=aarch64-unknown-none-softfloat
# The aarch64 lane's flags (ci.yml); cfi-test adds the landing pads on top.
BASE_FLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
CFI_FLAGS="$BASE_FLAGS -Z branch-protection=bti"
TD="$WORK/target"
CFI_TD="$WORK/target-cfi"
REL="$TD/$TARGET/release"
EMB="$WORK/embedded"
LOG="$WORK/qemu-cfi.log"

export CC_aarch64_unknown_none_softfloat=clang
export CFLAGS_aarch64_unknown_none_softfloat="--target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$PWD/third_party/freestanding-include"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu"
PYTHON_BIN="${PYTHON_BIN:-python3}"
export PYTHON_BIN

fail() {
    echo "FAIL: $*" >&2
    [[ -f "$LOG" ]] && tail -60 "$LOG" >&2
    exit 1
}

mkdir -p "$WORK" "$EMB"
rm -f "$EMB/kernel_fs.img" "$EMB/init" "$LOG"

echo "==> Building the default aarch64 lane's cells (no landing pads)"
CARGO_TARGET_DIR="$TD" CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_RUSTFLAGS="$BASE_FLAGS" \
    cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-init -p app-cellbox -p service-vfs -p service-config -p service-input \
    -p app-sys-tools

echo "==> Building cfi-test with BTI landing pads (whole invocation, core included)"
CARGO_TARGET_DIR="$CFI_TD" RUSTFLAGS="$CFI_FLAGS" \
    cargo build --release --target "$TARGET" -Z build-std=core,alloc -p cfi-test
CFI_ELF="$CFI_TD/$TARGET/release/cfi-test"

has_bti_note() { readelf -n "$1" 2>/dev/null | grep -q "AArch64 feature: .*BTI"; }
has_bti_note "$CFI_ELF" || fail "cfi-test carries no GNU_PROPERTY_AARCH64_FEATURE_1_AND BTI note"
readelf -lW "$CFI_ELF" | grep -q GNU_PROPERTY || fail "cfi-test has no PT_GNU_PROPERTY segment"
for cell in cellbox service-vfs service-config ls; do
    ! has_bti_note "$REL/$cell" || fail "baseline cell $cell unexpectedly opts into BTI"
done
cp "$CFI_ELF" "$WORK/cfi-test"

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing cells"
sign_cells "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input" \
    "$REL/ls" "$REL/echo" "$REL/ps" "$WORK/cfi-test"
has_bti_note "$WORK/cfi-test" || fail "signing dropped the BTI note"

echo "==> Assembling $EMB/kernel_fs.img"
"$PYTHON_BIN" tools/mkfat32.py --config-features input "$EMB/kernel_fs.img" \
    "$REL/cellbox" /bin/cellbox \
    "$REL/service-vfs" /bin/vfs \
    "$REL/service-config" /bin/config \
    "$REL/service-input" /bin/input \
    "$REL/ls" /bin/ls \
    "$REL/echo" /bin/echo \
    "$REL/ps" /bin/ps \
    "$WORK/cfi-test" /bin/cfi-test
cp "$REL/app-init" "$EMB/init"

echo "==> Building the aarch64 kernel (EMBEDDED_OVERRIDE=$EMB)"
CARGO_TARGET_DIR="$TD" CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_RUSTFLAGS="$BASE_FLAGS" \
EMBEDDED_OVERRIDE="$EMB" \
    cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc
KERNEL="$REL/cellos-kernel"

if [[ ! -f "$DISK_SRC" ]]; then
    bash scripts/format-disk-arm.sh "$DISK_SRC"
fi
cp "$DISK_SRC" "$WORK/disk.img"

echo "==> Booting QEMU (cpu=$QEMU_CPU, window=${BOOT_WINDOW}s)"
FIFO="$WORK/serial-in"
rm -f "$FIFO"
mkfifo "$FIFO"
timeout "$BOOT_WINDOW" qemu-system-aarch64 -machine virt -cpu "$QEMU_CPU" -smp 1 -m 256M \
    -nographic -no-reboot -kernel "$KERNEL" \
    -drive "if=none,file=$WORK/disk.img,format=raw,id=hd0" -device virtio-blk-device,drive=hd0 \
    -device virtio-keyboard-device \
    <"$FIFO" >"$WORK/qemu-cfi.raw.log" 2>&1 &
QEMU_PID=$!
exec 3>"$FIFO"

clean_log() { tr -d '\000' <"$WORK/qemu-cfi.raw.log" | sed 's/\x1b\[[0-9;]*m//g' >"$LOG"; }
wait_for() {
    local pattern=$1 seconds=$2
    for _ in $(seq 1 "$seconds"); do
        clean_log
        grep -qaE "$pattern" "$LOG" && return 0
        kill -0 "$QEMU_PID" 2>/dev/null || return 1
        sleep 1
    done
    return 1
}
type_line() { printf '%s\r' "$1" >&3; }

wait_for "Cellos >" 100 || fail "shell prompt not reached"
sleep 2
type_line "cfi-test"
wait_for "\[bti\] Branch Target exception|cfi-test: (SKIP|FAIL)|\[fault\] Cell" 30 || true
sleep 3
# The shell is an ordinary (non-opted-in) cell: it must still run a command,
# whose output (the process table) cannot come from the typed line's echo.
type_line "ps"
sleep 6
exec 3>&-
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true
clean_log

require() { grep -qaE "$2" "$LOG" || fail "$1 (pattern: $2)"; }
refuse() { ! grep -qaE "$2" "$LOG" || fail "$1 (pattern: $2)"; }

require "cfi-test did not start its probe" "cfi-test: aarch64 BTI probe: landing pad"
require "control call to the landing pad did not return" "cfi-test: control OK"
refuse "cfi-test reported FAIL" "cfi-test: FAIL"
# Only the opted-in image may be armed.
OTHERS=$(grep -aE "\[cfi\] '[^']*': BTI armed" "$LOG" | grep -av "cfi-test" || true)
[[ -z "$OTHERS" ]] || fail "a cell other than cfi-test was armed: $OTHERS"
AFTER=$(sed -n '/cfi-test: triggering indirect call/,$p' "$LOG")
grep -qa "cellbox" <<<"$AFTER" || fail "the shell did not run 'ps' after the probe"
[[ $(grep -ac "Cellos >" <<<"$AFTER") -ge 2 ]] || fail "the shell prompt did not return after the probe"

# Match the kernel's boot line exactly: the cell's own SKIP reason quotes it.
if grep -qaE "^\[INFO\] CFI: BTI enabled[[:space:]]*$" "$LOG"; then
    TARGET_ADDR=$(grep -aoE "non-landing-pad target 0x[0-9a-f]+" "$LOG" | head -1 | awk '{print $3}')
    [[ -n "$TARGET_ADDR" ]] || fail "no printed non-landing-pad address"
    require "loader did not arm cfi-test" "\[cfi\] '[^']*cfi-test[^']*': BTI armed"
    refuse "BTI enabled but the violation returned (kernel did not enforce)" "cfi-test: SKIP"
    require "no EC 0x0D fault at the non-landing-pad target $TARGET_ADDR" \
        "\[fault-probe\] a64 .*ec=0xd .*pc=$TARGET_ADDR"
    require "no [bti] line at $TARGET_ADDR" "\[bti\] Branch Target exception at EL0 pc=$TARGET_ADDR"
    require "cfi-test was not terminated" "\[fault\] Cell .*cause=0x3[4-7]"
    echo "PASS: BTI enforced for the opted-in cell (EC 0x0D at $TARGET_ADDR); the shell kept running"
elif grep -qaE "^\[INFO\] CFI: BTI unavailable[[:space:]]*$" "$LOG"; then
    require "loader did not log the unenforced opt-in" "\[cfi\] '[^']*cfi-test[^']*': image opts into BTI but the CPU lacks FEAT_BTI"
    require "cfi-test did not report SKIP" "cfi-test: SKIP: BTI not enforced for EL0"
    refuse "a BTI fault without FEAT_BTI" "\[bti\] Branch Target exception"
    echo "PASS: CPU lacks FEAT_BTI — cfi-test SKIP, nothing armed; the shell kept running"
else
    fail "boot log carries neither 'CFI: BTI enabled' nor 'CFI: BTI unavailable'"
fi
