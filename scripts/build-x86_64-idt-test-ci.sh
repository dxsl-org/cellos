#!/usr/bin/env bash
# Build the CPL0 + mandatory real-CPL3 IDT fixture in isolated outputs.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

TARGET_DIR="target/x86-idt-test"
ISO_DIR="build/x86-idt-test"
ISO_ROOT="$ISO_DIR/iso-root"
ISO_OUT="$ISO_DIR/vicell-x86-idt-test.iso"
KERNEL="$TARGET_DIR/x86_64-unknown-none/release/cellos-kernel"

mkdir -p "$ISO_DIR"
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
CARGO_TARGET_DIR="$TARGET_DIR" \
cargo build --release -p cellos-kernel --features x86-idt-cpl3-test \
    --target x86_64-unknown-none -Z build-std=core,alloc

X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash scripts/x86/make-iso-ci.sh "$ISO_OUT"

echo "IDT_TEST_KERNEL=$KERNEL"
echo "IDT_TEST_ISO=$ISO_OUT"
