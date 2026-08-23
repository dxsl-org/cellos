#!/usr/bin/env bash
# Build an isolated RV64 kernel whose native-domain assertions are available only
# through test hooks. Production build outputs and feature tuples are untouched.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REL="target/riscv64gc-unknown-none-elf/release"
DOMAIN_TARGET="target/native-domain-test"
DOMAIN_KERNEL="$REL/cellos-kernel-native-domain-test"

# Reuse the signed test fixture/image construction. It never enables
# native-domains; that feature is compiled only into the isolated kernel below.
bash scripts/build-test-hooks-ci.sh

rm -rf "$DOMAIN_TARGET"
EMBEDDED_OVERRIDE="kernel/src/embedded-test-hooks" \
CARGO_TARGET_DIR="$DOMAIN_TARGET" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    --features test-hooks,native-domains \
    -p cellos-kernel

mkdir -p "$REL"
cp "$DOMAIN_TARGET/riscv64gc-unknown-none-elf/release/cellos-kernel" "$DOMAIN_KERNEL"
printf 'PASS: native-domain test-hooks kernel: %s\n' "$DOMAIN_KERNEL"
