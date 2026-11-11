#!/usr/bin/env bash
# Stack & Tile on the x86 desktop image (ADR-0027 amendment): stack two windows
# from the window menu, pick a tab, Win+drag a tab out, Win+drag a window
# beside another to tile it, move the tile group. Pixel checks over QMP.
#
# Usage: STACK_TILE_WORK=$(mktemp -d /tmp/cellos-stack-tile-XXXX) \
#        [STACK_TILE_ACCEL=kvm|tcg] bash scripts/qemu-stack-tile-test.sh
# Builds a private image under $STACK_TILE_WORK; nothing in the repo is written.
set -euo pipefail
W="${STACK_TILE_WORK:?set STACK_TILE_WORK to an owned /tmp/cellos-stack-tile-* directory}"
case "$(realpath -m "$W")" in /tmp/cellos-stack-tile-*) ;; *) echo "FAIL: STACK_TILE_WORK must be /tmp/cellos-stack-tile-*" >&2; exit 1 ;; esac
ACCEL="${STACK_TILE_ACCEL:-kvm}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p "$W"

echo "==> Assembling desktop cells"
CARGO_TARGET_DIR="$W/cells-target" python3 scripts/assemble-x86-image.py --profile desktop --out-dir "$W/embedded"
echo "==> Building kernel"
EMBEDDED_OVERRIDE="$W/embedded" CARGO_TARGET_DIR="$W/kernel-target" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc \
    --features "dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,qemu-dev-https,large-cell-va"
echo "==> Building ISO"
rm -rf "$W/iso-root" "$W/cellos.iso"
X86_KERNEL="$W/kernel-target/x86_64-unknown-none/release/cellos-kernel" X86_ISO_ROOT="$W/iso-root" \
    bash scripts/x86/make-iso-ci.sh "$W/cellos.iso"

echo "==> Driving Stack & Tile ($ACCEL)"
python3 scripts/stack_tile_drive.py --iso "$W/cellos.iso" --evidence "$W/evidence" --accel "$ACCEL"
