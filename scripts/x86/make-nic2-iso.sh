#!/usr/bin/env bash
# Build the private x86_64 ISO for the Phase 18 two-NIC witness
# (tests/integration/tests/nic2-x86.rs, `VICELL_NIC2_ISO`).
#
# The image differs from the canonical headless image in two ways that matter:
# - /bin/e1000e and /bin/igb are built with `--features nic-driver/fault-inject`
#   (libs/nic-driver/src/fault_inject.rs): a transmitted frame carrying
#   `F18-DIE-pci-BB-DD-F` for a driver's own function makes that driver exit 1
#   without quiescing its controller;
# - its boot config is scripts/x86/nic2-boot-config, copied verbatim: two
#   `/bin/e1000e` entries and one `/bin/igb` entry, all `restart = "on-failure"`,
#   so one driver instance runs per NIC and init restarts a killed one.
# It carries only the cells the witness needs (init with the canonical `input`
# feature, vfs, machine-pin, input, cellbox, platform, the serial terminal, net,
# e1000e, igb, and `nc`, which carries the fault marker as a DNS hostname),
# dev-key signed like every other lane, plus the signed operator policy the
# `policy-required` kernel insists on.
#
# Everything is built under its own cargo target directory so a fault-injecting
# driver never lands in target/x86_64-unknown-none/release, which the canonical
# assembler and ISO builders read. The canonical route
# (scripts/assemble-x86-image.py) is not used: its pinned recipe has no feature
# knob, by design.
#
# Usage: bash scripts/x86/make-nic2-iso.sh [iso-out]
#   iso-out defaults to $CARGO_TARGET_DIR/vicell-x86-nic2.iso.
#   CARGO_TARGET_DIR defaults to target/nic2-18.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

TARGET="x86_64-unknown-none"
CARGO_TARGET_DIR="$(realpath -m -- "${CARGO_TARGET_DIR:-target/nic2-18}")"
export CARGO_TARGET_DIR
BIN_DIR="$CARGO_TARGET_DIR/$TARGET/release"
EMBEDDED="$CARGO_TARGET_DIR/embedded"
ISO_OUT="${1:-$CARGO_TARGET_DIR/vicell-x86-nic2.iso}"
CONFIG_DIR="$SCRIPT_DIR/nic2-boot-config"

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
echo "[nic2] building cells into $BIN_DIR"
RUSTFLAGS="-C relocation-model=pic" cargo build --release --locked \
    --target "$TARGET" -Z build-std=core,alloc \
    -p app-init -p service-vfs -p service-machine-pin -p service-input -p app-cellbox -p service-platform \
    -p driver-serial-x86 -p service-net -p driver-e1000e -p driver-igb -p app-net-tools \
    --features app-init/input,nic-driver/fault-inject

# shellcheck source=scripts/lib-sign-cells.sh
source "$REPO_ROOT/scripts/lib-sign-cells.sh"
SIGN_TARGETS=()
for binary in app-init service-vfs service-machine-pin service-input cellbox platform driver-serial-x86 \
    service-net driver-e1000e driver-igb nc; do
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
    --config-dir "$CONFIG_DIR" \
    --config-output-dir "$EMBEDDED/boot-config" \
    "$EMBEDDED/kernel_fs.img" \
    "$BIN_DIR/app-init" /bin/init \
    "$BIN_DIR/service-vfs" /bin/vfs \
    "$BIN_DIR/service-machine-pin" /bin/machine-pin \
    "$BIN_DIR/service-input" /bin/input \
    "$BIN_DIR/cellbox" /bin/cellbox \
    "$BIN_DIR/platform" /bin/platform \
    "$BIN_DIR/driver-serial-x86" /bin/serial \
    "$BIN_DIR/service-net" /bin/net \
    "$BIN_DIR/driver-e1000e" /bin/e1000e \
    "$BIN_DIR/driver-igb" /bin/igb \
    "$BIN_DIR/nc" /bin/nc \
    "$WORK/POLICY.BIN" /POLICY.BIN

for name in system services autoload; do
    cmp -s "$EMBEDDED/boot-config/$name.toml" "$CONFIG_DIR/$name.toml" \
        || { echo "FAIL: image $name.toml differs from $CONFIG_DIR" >&2; exit 1; }
done
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
for required in init vfs machine-pin input cellbox platform serial net e1000e igb nc; do
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
echo "[nic2] kernel features: $KERNEL_FEATURES"
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release --locked \
    -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc \
    --features "$KERNEL_FEATURES"

X86_KERNEL="$BIN_DIR/cellos-kernel" X86_ISO_ROOT="$CARGO_TARGET_DIR/iso-root" \
    bash scripts/x86/make-iso-ci.sh "$ISO_OUT"
echo "[nic2] ISO: $ISO_OUT"
