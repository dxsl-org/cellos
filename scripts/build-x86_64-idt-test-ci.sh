#!/usr/bin/env bash
# Build the CPL0 + mandatory real-CPL3 IDT fixture in isolated outputs.
# X86_IDT_PROFILE=protect marks SMEP, SMAP and UMIP required (`x86-smep`,
# `x86-smap`, `x86-umip`: a CPU without one refuses boot) and builds into
# separate target/ISO dirs. The base profile enables whichever the CPU has.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

case "${X86_IDT_PROFILE:-base}" in
    base) SUFFIX=""; EXTRA_FEATURES="" ;;
    protect) SUFFIX="-protect"; EXTRA_FEATURES=",x86-smep,x86-smap,x86-umip" ;;
    *) echo "FAIL: X86_IDT_PROFILE must be base or protect" >&2; exit 1 ;;
esac
TARGET_DIR="target/x86-idt-test$SUFFIX"
ISO_DIR="build/x86-idt-test$SUFFIX"
ISO_ROOT="$ISO_DIR/iso-root"
ISO_OUT="$ISO_DIR/vicell-x86-idt-test.iso"
KERNEL="$TARGET_DIR/x86_64-unknown-none/release/cellos-kernel"

mkdir -p "$ISO_DIR"
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
CARGO_TARGET_DIR="$TARGET_DIR" EMBEDDED_OVERRIDE=kernel/src/embedded-x86_64 \
cargo build --release -p cellos-kernel --features "x86-idt-cpl3-test$EXTRA_FEATURES" \
    --target x86_64-unknown-none -Z build-std=core,alloc

X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash scripts/x86/make-iso-ci.sh "$ISO_OUT"

echo "IDT_TEST_KERNEL=$KERNEL"
echo "IDT_TEST_ISO=$ISO_OUT"
