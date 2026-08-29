#!/usr/bin/env bash
# Build an x86_64 kernel_fs.img for the ViCell x86 hypervisor cell.
#
# The image carries the Alpine PVH guest plus the complete host runtime needed
# by the x86 VirtIO-MMIO evidence lane:
#   1. Fetch + extract Alpine x86_64 artifacts (scripts/fetch-alpine-x86.sh).
#   2. Build init, platform, PCIe drivers, VFS, Net, and hypervisor as PIE.
#   3. Assemble kernel/src/embedded-hv-x86/kernel_fs.img with the runtime cells,
#      /vmlinux, and /initrd.gz.
#   4. Copy app-init as the separately-embedded INIT_ELF.
#
# x86 cells MUST build with `relocation-model=pic` (RUSTFLAGS below) — the
# config.toml x86 target pins the KERNEL's static/kernel-code-model flags, which
# are wrong for lower-half PIE cells; RUSTFLAGS replaces them wholesale.
#
# After running, build the fixed-address hypervisor kernel with the same static
# relocation contract as CI (the x86 Cell binaries above remain PIC):
#   RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full" \
#   EMBEDDED_OVERRIDE="kernel/src/embedded-hv-x86" \
#   cargo build --release -p cellos-kernel --target x86_64-unknown-none
# then: ./run-x86.ps1 -NoBuild  (or qemu ... -cpu qemu64,+svm -accel tcg)
#
# Usage: bash scripts/make-hypervisor-fs-x86.sh [--skip-fetch]

set -euo pipefail

SKIP_FETCH="${1:-}"
TARGET="x86_64-unknown-none"
BIN_DIR="target/$TARGET/release"
ALPINE_CACHE=".alpine-cache-x86"
INITRD_SOURCE="${INITRD_OVERRIDE:-$ALPINE_CACHE/initramfs-virt}"
EMBEDDED_HV="kernel/src/embedded-hv-x86"

HV_INIT_MIN_VALUE="${HV_INIT_MIN:-0}"
HV_HOSTILE_BACKEND_RECOVERY_VALUE="${HV_HOSTILE_BACKEND_RECOVERY:-0}"
case "$HV_INIT_MIN_VALUE" in
    0|1) ;;
    *)
        echo "ERROR: HV_INIT_MIN must be 0 or 1" >&2
        exit 1
        ;;
esac
case "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" in
    0|1) ;;
    *)
        echo "ERROR: HV_HOSTILE_BACKEND_RECOVERY must be 0 or 1" >&2
        exit 1
        ;;
esac

# ── Step 1: Alpine artifacts (vmlinux ELF + initramfs) ──────────────────────
if [[ "$SKIP_FETCH" != "--skip-fetch" ]]; then
    bash scripts/fetch-alpine-x86.sh "$ALPINE_CACHE"
fi
if [[ ! -f "$ALPINE_CACHE/vmlinux" || ! -f "$ALPINE_CACHE/initramfs-virt" ]]; then
    echo "ERROR: Alpine x86 artifacts missing — run scripts/fetch-alpine-x86.sh" >&2
    exit 1
fi

# ── Step 2: Build x86 runtime cells as PIE ───────────────────────────────────

INIT_FEATURES="service-net/tls-roots-embedded,service-net/tls-ca-private"
HOSTILE_PACKAGE_ARGS=()
if [[ "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" == "1" ]]; then
    INIT_FEATURES+=",app-init/hypervisor-min,app-init/hostile-backend-recovery"
    INIT_FEATURES+=",service-net/hypervisor-bridge,supervisor/hostile-backend-recovery"
    INIT_FEATURES+=",service-hypervisor/hostile-backend-recovery"
    HOSTILE_PACKAGE_ARGS=(-p supervisor)
elif [[ "$HV_INIT_MIN_VALUE" == "1" ]]; then
    INIT_FEATURES+=",app-init/hypervisor-min,service-net/hypervisor-bridge"
fi
INIT_FEATURE_ARGS=(--features "$INIT_FEATURES")

RUSTFLAGS="-C relocation-model=pic" cargo build --release \
    --target "$TARGET" \
    -Z build-std=core,alloc \
    --no-default-features \
    -p service-vfs

RUSTFLAGS="-C relocation-model=pic" cargo build --release \
    --target "$TARGET" \
    -Z build-std=core,alloc \
    "${INIT_FEATURE_ARGS[@]}" \
    -p app-init -p app-shell -p service-config \
    -p service-platform -p driver-nvme -p driver-e1000 \
    -p service-net -p service-hypervisor \
    "${HOSTILE_PACKAGE_ARGS[@]}"

# ── Step 3: Assemble kernel_fs.img ──────────────────────────────────────────
mkdir -p "$EMBEDDED_HV"
# /bin/shell remains available to the ordinary profile. The hypervisor-min
# profile does not start it; it supervises VFS then Net before starting the
# hypervisor. Hostile backend recovery additionally supervises /bin/supervisor
# after Net, leaving the nested guest as the only interactive console.
MKFAT_ARGS=()
add_required_cell() {
    local artifact="$1"
    local destination="$2"
    local src="$BIN_DIR/$artifact"
    if [[ ! -f "$src" ]]; then
        echo "ERROR: $src not built — $destination would be missing" >&2
        exit 1
    fi
    echo "  $destination <- $src"
    MKFAT_ARGS+=("$src" "$destination")
}

add_required_cell app-init /bin/init
add_required_cell service-vfs /bin/vfs
add_required_cell service-config /bin/config
add_required_cell app-shell /bin/shell
add_required_cell platform /bin/platform
add_required_cell driver-nvme /bin/nvme
add_required_cell driver-e1000 /bin/e1000
add_required_cell service-net /bin/net
add_required_cell hypervisor /bin/hypervisor
if [[ "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" == "1" ]]; then
    add_required_cell supervisor /bin/supervisor
fi

echo "  /vmlinux   <- $ALPINE_CACHE/vmlinux ($(du -sh "$ALPINE_CACHE/vmlinux" | cut -f1))"
MKFAT_ARGS+=("$ALPINE_CACHE/vmlinux" "/vmlinux")
echo "  /initrd.gz <- $INITRD_SOURCE ($(du -sh "$INITRD_SOURCE" | cut -f1))"
MKFAT_ARGS+=("$INITRD_SOURCE" "/initrd.gz")

# Signed operator policy. Without /POLICY.BIN the kernel takes the `Absent` branch,
# which is dev-permissive: the whole policy layer runs and changes nothing, and no test
# can tell the difference.
# On Windows the bare name `python3` is the Microsoft Store alias stub, which prints
# an install hint and exits without running anything. Probe for an interpreter that
# actually works — the same guard the build-*-ci.sh scripts carry.
if [[ -z "${PYTHON_BIN:-}" ]]; then
    if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
        PYTHON_BIN=python3
    elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
        PYTHON_BIN=python
    else
        echo "FAIL: a working Python 3 interpreter is required" >&2
        exit 1
    fi
fi
# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
POLICY_TMP=$(mktemp -d "target/hv-x86-policy-tmp.XXXXXX")
trap 'rm -rf "$POLICY_TMP"' EXIT
bake_policy "$POLICY_TMP/POLICY.BIN"
echo "  /POLICY.BIN <- signed operator policy"
MKFAT_ARGS+=("$POLICY_TMP/POLICY.BIN" "/POLICY.BIN")

# MSYS2_ARG_CONV_EXCL: without it Git Bash rewrites every leading-slash DESTINATION
# into a Windows path before Python sees it, and mkfat32.py builds an image whose /bin
# holds "Program Files" instead of the cells. mkfat32 still exits 0. The aarch64
# sibling dodges this by using slash-free destinations; this lane needs the guard.
MSYS2_ARG_CONV_EXCL='*' "$PYTHON_BIN" tools/mkfat32.py \
    "$EMBEDDED_HV/kernel_fs.img" "${MKFAT_ARGS[@]}"

# Prove the runtime layout rather than trusting mkfat32's exit code.
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED_HV/kernel_fs.img" > "$POLICY_TMP/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$POLICY_TMP/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img does not contain /bin" >&2
    cat "$POLICY_TMP/fat-layout.txt" >&2
    exit 1
fi
for required in init platform nvme e1000 net vfs hypervisor; do
    if ! grep -Fq -- "LFN '$required'" "$POLICY_TMP/fat-layout.txt"; then
        echo "FAIL: kernel_fs.img does not contain /bin/$required" >&2
        cat "$POLICY_TMP/fat-layout.txt" >&2
        exit 1
    fi
done
if [[ "$HV_HOSTILE_BACKEND_RECOVERY_VALUE" == "1" ]] \
    && ! grep -Fq -- "LFN 'supervisor'" "$POLICY_TMP/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img does not contain /bin/supervisor" >&2
    cat "$POLICY_TMP/fat-layout.txt" >&2
    exit 1
fi
assert_policy_in_image "$POLICY_TMP/fat-layout.txt" || exit 1

cp "$BIN_DIR/app-init" "$EMBEDDED_HV/init"
echo "[make-hv-x86] init <- $BIN_DIR/app-init"

echo ""
echo "[make-hv-x86] kernel_fs.img created at $EMBEDDED_HV/kernel_fs.img"
ls -lh "$EMBEDDED_HV/kernel_fs.img"
echo ""
echo "Next — build the hypervisor kernel then boot (see header comment)."
