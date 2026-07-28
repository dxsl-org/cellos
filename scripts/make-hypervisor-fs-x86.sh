#!/usr/bin/env bash
# Build an x86_64 kernel_fs.img that embeds the Alpine PVH vmlinux + initramfs
# for the ViCell x86 hypervisor cell (Tier 3b P05).
#
# Mirrors make-hypervisor-fs.sh (aarch64) for the x86 personality:
#   1. Fetch + extract Alpine x86_64 artifacts (scripts/fetch-alpine-x86.sh).
#   2. Build the x86 cells (bootstrap + service-hypervisor) as PIE.
#   3. Assemble kernel/src/embedded-hv-x86/kernel_fs.img with:
#        /bin/{init,shell,vfs,config,hypervisor}  +  /vmlinux  +  /initrd.gz
#   4. Copy app-init as the separately-embedded INIT_ELF.
#
# x86 cells MUST build with `relocation-model=pic` (RUSTFLAGS below) — the
# config.toml x86 target pins the KERNEL's static/kernel-code-model flags, which
# are wrong for lower-half PIE cells; RUSTFLAGS replaces them wholesale.
#
# After running, build the hypervisor kernel:
#   RUSTFLAGS="-C relocation-model=pic -C code-model=kernel -C target-feature=-red-zone" \
#   EMBEDDED_OVERRIDE="kernel/src/embedded-hv-x86" \
#   cargo build --release -p vicell-kernel --target x86_64-unknown-none
# then: ./run-x86.ps1 -NoBuild  (or qemu ... -cpu qemu64,+svm -accel tcg)
#
# Usage: bash scripts/make-hypervisor-fs-x86.sh [--skip-fetch]

set -euo pipefail

SKIP_FETCH="${1:-}"
TARGET="x86_64-unknown-none"
BIN_DIR="target/$TARGET/release"
ALPINE_CACHE=".alpine-cache-x86"
EMBEDDED_HV="kernel/src/embedded-hv-x86"

# ── Step 1: Alpine artifacts (vmlinux ELF + initramfs) ──────────────────────
if [[ "$SKIP_FETCH" != "--skip-fetch" ]]; then
    bash scripts/fetch-alpine-x86.sh "$ALPINE_CACHE"
fi
if [[ ! -f "$ALPINE_CACHE/vmlinux" || ! -f "$ALPINE_CACHE/initramfs-virt" ]]; then
    echo "ERROR: Alpine x86 artifacts missing — run scripts/fetch-alpine-x86.sh" >&2
    exit 1
fi

# ── Step 2: Build x86 cells as PIE ──────────────────────────────────────────
echo "[make-hv-x86] Building x86 cells (service-hypervisor + core cells)..."
RUSTFLAGS="-C relocation-model=pic" cargo build --release \
    --target "$TARGET" \
    -Z build-std=core,alloc \
    -p app-init -p service-vfs -p service-config \
    -p service-net -p service-hypervisor

# ── Step 3: Assemble kernel_fs.img ──────────────────────────────────────────
mkdir -p "$EMBEDDED_HV"
# NO /bin/shell in this image: the kernel UART RX ring (sys_read fd 0) is a
# shared stream, and the shell's read_line loop would race the hypervisor for
# keystrokes destined for the guest console. Init tolerates the absence
# ("shell spawn failed") — the Linux guest's /bin/sh IS the console here.
MKFAT_ARGS=()
for cell in init vfs config; do
    src="$BIN_DIR/app-$cell"
    [[ ! -f "$src" ]] && src="$BIN_DIR/service-$cell"
    if [[ -f "$src" ]]; then
        echo "  /bin/$cell <- $src"
        MKFAT_ARGS+=("$src" "/bin/$cell")
    else
        echo "  WARNING: $cell not found — skipping"
    fi
done

if [[ -f "$BIN_DIR/hypervisor" ]]; then
    echo "  /bin/hypervisor <- $BIN_DIR/hypervisor"
    MKFAT_ARGS+=("$BIN_DIR/hypervisor" "/bin/hypervisor")
else
    echo "ERROR: $BIN_DIR/hypervisor not built" >&2
    exit 1
fi

echo "  /vmlinux   <- $ALPINE_CACHE/vmlinux ($(du -sh "$ALPINE_CACHE/vmlinux" | cut -f1))"
MKFAT_ARGS+=("$ALPINE_CACHE/vmlinux" "/vmlinux")
echo "  /initrd.gz <- $ALPINE_CACHE/initramfs-virt ($(du -sh "$ALPINE_CACHE/initramfs-virt" | cut -f1))"
MKFAT_ARGS+=("$ALPINE_CACHE/initramfs-virt" "/initrd.gz")

python3 tools/mkfat32.py "$EMBEDDED_HV/kernel_fs.img" "${MKFAT_ARGS[@]}"

cp "$BIN_DIR/app-init" "$EMBEDDED_HV/init"
echo "[make-hv-x86] init <- $BIN_DIR/app-init"

echo ""
echo "[make-hv-x86] kernel_fs.img created at $EMBEDDED_HV/kernel_fs.img"
ls -lh "$EMBEDDED_HV/kernel_fs.img"
echo ""
echo "Next — build the hypervisor kernel then boot (see header comment)."
