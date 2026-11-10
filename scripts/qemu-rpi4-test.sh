#!/usr/bin/env bash
# Opt-in QEMU `raspi4b` lane for the `board-rpi4` kernel (not run by CI).
#
# Boots a raw kernel8.img the way VideoCore firmware does (load at 0x80000,
# DTB in x0, cores 1-3 parked on the DTB's spin table at 0xd8/0xe0/0xe8/0xf0)
# and requires: all four CPUs online through the spin table, each answering
# the SMP-IPI self-test, init reporting "OS ready", and the shell prompt.
#
# QEMU (>= 9.0, `qemu-system-aarch64 -M help | grep raspi4b`) models the
# Cortex-A72 cluster, GIC-400, PL011, generic timer, EMMC2/SDHCI and the
# spin-table boot. It does NOT model PCIe (so no VL805 xHCI/USB), GENET
# Ethernet, RNG200 or the thermal sensor: QEMU replaces those DTB nodes with
# FDT_NOP. No SD card is attached, so the kernel runs from its embedded VIFS1
# ("[mmc] no card found" is expected). A pass here does not qualify physical
# Raspberry Pi 4 hardware.
#
# Usage: scripts/qemu-rpi4-test.sh [kernel8.img]
#   Without an image, builds one into $RPI4_OUT: the aarch64 cells and signed
#   kernel_fs.img of ci.yml's "QEMU Boot Test (aarch64)" job (into
#   $RPI4_OUT/embedded, never the tracked kernel/src/embedded-aarch64), then
#   `cellos-kernel --features board-rpi4` with EMBEDDED_OVERRIDE, flattened to
#   a raw kernel8.img.
# Environment:
#   RPI4_DTB     DTB to pass (default tests/fixtures/dtb/bcm2711-rpi-4-b.dtb,
#                Raspberry Pi firmware; provenance in tests/fixtures/dtb/SOURCES.txt)
#   RPI4_OUT     build/log directory (default /tmp/cellos-rpi4)
#   BOOT_WINDOW  seconds QEMU runs (default 180)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

RPI4_OUT="${RPI4_OUT:-/tmp/cellos-rpi4}"
RPI4_DTB="${RPI4_DTB:-tests/fixtures/dtb/bcm2711-rpi-4-b.dtb}"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
MARKER_HELPER="$REPO_ROOT/scripts/assert-boot-markers.sh"
TARGET=aarch64-unknown-none-softfloat

if ! command -v qemu-system-aarch64 &>/dev/null; then
    echo "FAIL: qemu-system-aarch64 not found on PATH" >&2
    exit 1
fi
if ! qemu-system-aarch64 -M help | grep -q '^raspi4b '; then
    echo "FAIL: $(command -v qemu-system-aarch64) has no raspi4b machine (needs QEMU >= 9.0)" >&2
    exit 1
fi
if [[ ! -f "$RPI4_DTB" ]]; then
    echo "FAIL: DTB not found: $RPI4_DTB" >&2
    exit 1
fi
mkdir -p "$RPI4_OUT"

build_kernel8() {
    local target_dir="$RPI4_OUT/target"
    local rel="$target_dir/$TARGET/release"
    local embedded="$RPI4_OUT/embedded"
    local freestanding="$REPO_ROOT/third_party/freestanding-include"

    echo "[qemu-rpi4-test] building aarch64 cells into $target_dir"
    CARGO_TARGET_DIR="$target_dir" \
    CC_aarch64_unknown_none_softfloat=clang \
    CFLAGS_aarch64_unknown_none_softfloat="--target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$freestanding" \
    BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu" \
        cargo build --release --target "$TARGET" -Z build-std=core,alloc \
        -p app-init -p app-cellbox -p service-vfs -p service-config \
        -p service-net -p service-input -p service-compositor \
        -p periph-demo -p input-test -p app-sys-tools

    echo "[qemu-rpi4-test] assembling signed kernel_fs.img + init in $embedded"
    mkdir -p "$embedded"
    PYTHON_BIN=python3
    # shellcheck source=scripts/lib-sign-cells.sh
    source scripts/lib-sign-cells.sh
    sign_cells \
        "$rel/cellbox" "$rel/service-vfs" "$rel/service-config" \
        "$rel/service-input" "$rel/input-test" "$rel/periph-demo" \
        "$rel/ls" "$rel/cat" "$rel/echo" "$rel/ps" "$rel/kill"
    python3 tools/mkfat32.py --config-features input \
        "$embedded/kernel_fs.img" \
        "$rel/cellbox"        /bin/cellbox \
        "$rel/service-vfs"    /bin/vfs \
        "$rel/service-config" /bin/config \
        "$rel/service-input"  /bin/input \
        "$rel/input-test"     /bin/input-test \
        "$rel/periph-demo"    /bin/periph-demo \
        "$rel/ls"             /bin/ls \
        "$rel/cat"            /bin/cat \
        "$rel/echo"           /bin/echo \
        "$rel/ps"             /bin/ps \
        "$rel/kill"           /bin/kill
    cp "$rel/app-init" "$embedded/init"

    echo "[qemu-rpi4-test] building cellos-kernel --features board-rpi4"
    EMBEDDED_OVERRIDE="$embedded" CARGO_TARGET_DIR="$target_dir" \
        cargo build --release -p cellos-kernel --target "$TARGET" \
        -Z build-std=core,alloc --features board-rpi4

    local objcopy
    for objcopy in aarch64-linux-gnu-objcopy llvm-objcopy rust-objcopy ""; do
        [[ -n "$objcopy" ]] && command -v "$objcopy" &>/dev/null && break
    done
    if [[ -z "$objcopy" ]]; then
        echo "FAIL: no objcopy found (aarch64-linux-gnu-objcopy, llvm-objcopy, rust-objcopy)" >&2
        exit 1
    fi
    "$objcopy" -O binary "$rel/cellos-kernel" "$RPI4_OUT/kernel8.img"
}

if [[ $# -ge 1 ]]; then
    KERNEL8="$1"
else
    build_kernel8
    KERNEL8="$RPI4_OUT/kernel8.img"
fi
if [[ ! -s "$KERNEL8" ]]; then
    echo "FAIL: kernel8.img not found: $KERNEL8" >&2
    exit 1
fi

echo "[qemu-rpi4-test] $(command -v qemu-system-aarch64): $(qemu-system-aarch64 --version | head -1)"
echo "[qemu-rpi4-test] booting kernel=$KERNEL8 dtb=$RPI4_DTB window=${BOOT_WINDOW}s"
LOG="$RPI4_OUT/qemu-rpi4.log"
timeout "$BOOT_WINDOW" qemu-system-aarch64 \
    -M raspi4b \
    -kernel "$KERNEL8" \
    -dtb "$RPI4_DTB" \
    -display none \
    -serial stdio \
    -no-reboot \
    < /dev/null > "$LOG.raw" 2>&1 || true
tr -d '\000' < "$LOG.raw" | sed 's/\x1b\[[0-9;]*m//g' > "$LOG"

if grep -qia "KERNEL PANIC\|\[fault\] Cell" "$LOG"; then
    echo "FAIL: kernel panic / cell fault during raspi4b boot" >&2
    grep -ai "fault\|PANIC" "$LOG" | head >&2
    exit 1
fi

bash "$MARKER_HELPER" "$LOG" "rpi4-raspi4b" \
    "hart 1 spin-table release:::[smp] hart 1 released via spin table at 0xe0" \
    "hart 1 online:::[smp] hart 1 online, parked" \
    "hart 2 online:::[smp] hart 2 online, parked" \
    "hart 3 online:::[smp] hart 3 online, parked" \
    "hart 1 IPI:::[selftest] SMP-IPI: PASS hart=1" \
    "hart 2 IPI:::[selftest] SMP-IPI: PASS hart=2" \
    "hart 3 IPI:::[selftest] SMP-IPI: PASS hart=3" \
    "EL0 Cell syscalls:::USER: Init: OS ready." \
    "shell prompt:::Cellos >"
echo "PASS: raspi4b — 4 CPUs online via spin table, shell prompt reached (log: $LOG)"
