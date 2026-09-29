#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# run-std-smoke-qemu.sh: Build and boot Tier 1 Rust std cell (std-smoke) in QEMU.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ARCH="${1:-riscv64}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-45}"

# Only riscv64 and aarch64 publish a CellOS target spec; each keeps its own
# bare-metal bootstrap target and QEMU machine recipe.
case "$ARCH" in
    riscv64)
        SYSROOT_TARGET="riscv64gc-unknown-cellos"
        TARGET_BOOTSTRAP="riscv64gc-unknown-none-elf"
        QEMU_BIN="${ViCell_QEMU:-qemu-system-riscv64}"
        ;;
    aarch64)
        SYSROOT_TARGET="aarch64-unknown-cellos"
        TARGET_BOOTSTRAP="aarch64-unknown-none-softfloat"
        QEMU_BIN="${ViCell_QEMU:-qemu-system-aarch64}"
        ;;
    x86_64)
        SYSROOT_TARGET="x86_64-unknown-cellos"
        TARGET_BOOTSTRAP="x86_64-unknown-none"
        QEMU_BIN="${ViCell_QEMU:-qemu-system-x86_64}"
        ;;
    *)
        echo "FAIL: unsupported arch: $ARCH (supported: riscv64, aarch64, x86_64)" >&2
        exit 2
        ;;
esac

# The bootstrap cell set follows the machine's drivers: riscv64/aarch64 boot the
# generic `virt` machine with virtio-blk, x86_64 the q35 machine with nvme/e1000
# (the same set the x86 image lanes stage).
case "$ARCH" in
    x86_64)
        BOOTSTRAP_PKGS=(app-init app-shell service-vfs service-config service-platform driver-nvme driver-e1000)
        BOOTSTRAP_BINS=(app-init app-shell service-vfs service-config platform driver-nvme driver-e1000)
        FAT_DRIVERS=("_REL_/driver-nvme" /bin/nvme "_REL_/driver-e1000" /bin/e1000)
        ;;
    *)
        BOOTSTRAP_PKGS=(app-init app-shell service-vfs service-config service-platform driver-virtio-blk)
        BOOTSTRAP_BINS=(app-init app-shell service-vfs service-config platform driver-virtio-blk)
        FAT_DRIVERS=("_REL_/driver-virtio-blk" /bin/block)
        ;;
esac
TARGET_CELL="targets/${SYSROOT_TARGET}.json"

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

echo "==> Step 1: Building sysroot overlay and std-smoke cell..."
bash scripts/build-cellos-sysroot.sh "$SYSROOT_TARGET"

STAGING_DIR="$(pwd)/target/cellos-rust-src/library"
export __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR"

cargo +nightly-2026-05-01 build --release \
    --manifest-path cells/demos/std-smoke/Cargo.toml \
    -Z build-std=core,alloc,std,panic_abort \
    -Z build-std-features=compiler-builtins-mem \
    -Z json-target-spec \
    --target "$TARGET_CELL"

STD_SMOKE_BIN="cells/demos/std-smoke/target/${SYSROOT_TARGET}/release/std-smoke"
if [[ ! -s "$STD_SMOKE_BIN" ]]; then
    echo "FAIL: std-smoke binary not found at $STD_SMOKE_BIN" >&2
    exit 1
fi

echo "==> Step 2: Building bootstrap cells..."
case "$ARCH" in
    riscv64)
        export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
        export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
        export CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="-C relocation-model=pic"
        ;;
    aarch64)
        export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
        export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
        export CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
        ;;
    x86_64)
        export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
        export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
        # x86_64 cells are position-independent; only the kernel is static.
        export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
        ;;
esac

PKG_ARGS=()
for pkg in "${BOOTSTRAP_PKGS[@]}"; do
    PKG_ARGS+=(-p "$pkg")
done

cargo build --release --target "$TARGET_BOOTSTRAP" \
    -Z build-std=core,alloc \
    "${PKG_ARGS[@]}"

REL="target/$TARGET_BOOTSTRAP/release"
BOOTSTRAP_CELLS=()
for bin in "${BOOTSTRAP_BINS[@]}"; do
    BOOTSTRAP_CELLS+=("$REL/$bin")
done
FAT_DRIVER_ARGS=()
for entry in "${FAT_DRIVERS[@]}"; do
    FAT_DRIVER_ARGS+=("${entry/_REL_/$REL}")
done
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
    "${FAT_DRIVER_ARGS[@]}" \
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

if [[ "$ARCH" == "x86_64" ]]; then
    # x86_64 has no direct -kernel entry: Limine loads the kernel from a
    # dual-firmware ISO, and the machine provides the nvme disk and e1000 NIC
    # that this cell set's drivers probe for.
    ISO="$WORK/std-smoke.iso"
    X86_KERNEL="$KERNEL" X86_ISO_ROOT="$WORK/iso-root" \
        bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null
    QEMU_ARGS=(
        -machine q35
        -cpu "${X86_CPU_MODEL:-qemu64,+pdpe1gb}"
        -m 256M
        -smp 1
        -nographic
        -monitor none
        -cdrom "$ISO"
        -boot d
        -no-reboot
        -drive "file=$DISK,format=raw,if=none,id=nvme0"
        -device nvme,drive=nvme0,serial=CELLOSSTD
        -netdev user,id=net0
        -device e1000,netdev=net0,mac=52:54:00:12:34:56
    )
else
    QEMU_ARGS=(
        -machine virt
        -m 256M
        -smp 1
        -nographic
        -monitor none
        -kernel "$KERNEL"
        -drive "file=$DISK,format=raw,if=none,id=hd0"
        -device virtio-blk-device,drive=hd0
        -device virtio-rng-device
    )
    case "$ARCH" in
        # riscv64 boots the ELF through the bundled OpenSBI (the kernel is a
        # direct -kernel payload); aarch64 enters the kernel ELF directly. Both
        # use the generic `virt` machine, and neither needs semihosting: the
        # runner stops QEMU on the cell's PASS marker.
        riscv64) QEMU_ARGS+=(-bios default) ;;
        aarch64) QEMU_ARGS+=(-cpu cortex-a57) ;;
    esac
fi

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
