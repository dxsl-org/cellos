#!/usr/bin/env bash
# Build the private x86_64 ISO for the Phase 08/09C block witnesses
# (tests/integration/tests/{ahci,nvme}-x86.rs, `VICELL_AHCI_FAULT_ISO`).
#
# The image differs from the canonical headless image in one way that matters:
# /bin/ahci and /bin/nvme are built with `--features fault-inject`
# (cells/drivers/ahci/src/fault_inject.rs, the `F08B-DIE`/`F09C-DIE-ISSUED`
# hooks in cells/drivers/nvme/src/dispatch.rs). It carries only the cells the
# witnesses need (init with the canonical `input` feature, vfs, machine-pin, input, cellbox,
# platform, the serial terminal, ahci, nvme, and e1000 as an independent PCI
# device owner beside the replaced storage driver), dev-key signed like every
# other lane, plus the signed operator policy the `policy-required` kernel
# insists on.
#
# Everything is built under its own cargo target directory so a fault-injecting
# driver never lands in target/x86_64-unknown-none/release, which the canonical
# assembler and ISO builders read. The canonical route
# (scripts/assemble-x86-image.py) is not used: its pinned recipe has no feature
# knob, by design.
#
# Usage: bash scripts/x86/make-ahci-fault-iso.sh [iso-out]
#   iso-out defaults to $CARGO_TARGET_DIR/vicell-x86-ahci-fault.iso.
#   CARGO_TARGET_DIR defaults to target/ahci-fault-08a.
#   X86_KERNEL_FEATURES (default empty) appends measurement-only kernel
#   features, e.g. `bench-counters` for the Phase 03 retirement-latency
#   milestones; use a separate CARGO_TARGET_DIR for such an image.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

TARGET="x86_64-unknown-none"
CARGO_TARGET_DIR="$(realpath -m -- "${CARGO_TARGET_DIR:-target/ahci-fault-08a}")"
export CARGO_TARGET_DIR
BIN_DIR="$CARGO_TARGET_DIR/$TARGET/release"
EMBEDDED="$CARGO_TARGET_DIR/embedded"
ISO_OUT="${1:-$CARGO_TARGET_DIR/vicell-x86-ahci-fault.iso}"

if [[ -z "${PYTHON_BIN:-}" ]]; then
    for candidate in python3 python; do
        if "$candidate" -c 'import sys' >/dev/null 2>&1; then
            PYTHON_BIN="$candidate"
            break
        fi
    done
fi
: "${PYTHON_BIN:?Python 3 is required}"

# ── Cells (PIC; RUSTFLAGS replaces the kernel's static x86 flags) ────────────
echo "[ahci-fault] building cells into $BIN_DIR"
RUSTFLAGS="-C relocation-model=pic" cargo build --release --locked \
    --target "$TARGET" -Z build-std=core,alloc \
    -p app-init -p service-vfs -p service-machine-pin -p service-input -p app-cellbox -p service-platform \
    -p driver-serial-x86 -p driver-ahci -p driver-nvme -p driver-e1000 \
    --features app-init/input,driver-ahci/fault-inject,driver-nvme/fault-inject

# shellcheck source=scripts/lib-sign-cells.sh
source "$REPO_ROOT/scripts/lib-sign-cells.sh"
SIGN_TARGETS=()
for binary in app-init service-vfs service-machine-pin service-input cellbox platform driver-serial-x86 driver-ahci driver-nvme driver-e1000; do
    SIGN_TARGETS+=("$BIN_DIR/$binary")
done
sign_cells "${SIGN_TARGETS[@]}"

# ── kernel_fs.img ─────────────────────────────────────────────────────────────
mkdir -p "$EMBEDDED"
WORK=$(mktemp -d "$CARGO_TARGET_DIR/policy-tmp.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
# shellcheck source=scripts/lib-bake-policy.sh
source "$REPO_ROOT/scripts/lib-bake-policy.sh"
bake_policy "$WORK/POLICY.BIN"

MSYS2_ARG_CONV_EXCL='*' "$PYTHON_BIN" tools/mkfat32.py \
    --config-features input \
    --config-output-dir "$EMBEDDED/boot-config" \
    "$EMBEDDED/kernel_fs.img" \
    "$BIN_DIR/app-init" /bin/init \
    "$BIN_DIR/service-vfs" /bin/vfs \
    "$BIN_DIR/service-machine-pin" /bin/machine-pin \
    "$BIN_DIR/service-input" /bin/input \
    "$BIN_DIR/cellbox" /bin/cellbox \
    "$BIN_DIR/platform" /bin/platform \
    "$BIN_DIR/driver-serial-x86" /bin/serial \
    "$BIN_DIR/driver-ahci" /bin/ahci \
    "$BIN_DIR/driver-nvme" /bin/nvme \
    "$BIN_DIR/driver-e1000" /bin/e1000 \
    "$WORK/POLICY.BIN" /POLICY.BIN

"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
for required in init vfs machine-pin input cellbox platform serial ahci nvme e1000; do
    if ! grep -Fq -- "LFN '$required'" "$WORK/fat-layout.txt"; then
        echo "FAIL: kernel_fs.img does not contain /bin/$required" >&2
        exit 1
    fi
done
assert_policy_in_image "$WORK/fat-layout.txt"
cp "$BIN_DIR/app-init" "$EMBEDDED/init"

# ── Kernel (canonical features) and ISO ──────────────────────────────────────
KERNEL_FEATURES="$("$PYTHON_BIN" -c 'import importlib.util, sys
spec = importlib.util.spec_from_file_location("assemble_x86_image", "scripts/assemble-x86-image.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
print(",".join(module.CANONICAL_KERNEL_FEATURES))')"
if [[ -n "${X86_KERNEL_FEATURES:-}" ]]; then
    KERNEL_FEATURES="$KERNEL_FEATURES,${X86_KERNEL_FEATURES// /,}"
fi
echo "[ahci-fault] kernel features: $KERNEL_FEATURES"
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release --locked \
    -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc \
    --features "$KERNEL_FEATURES"

X86_KERNEL="$BIN_DIR/cellos-kernel" X86_ISO_ROOT="$CARGO_TARGET_DIR/iso-root" \
    bash scripts/x86/make-iso-ci.sh "$ISO_OUT"
echo "[ahci-fault] ISO: $ISO_OUT"
