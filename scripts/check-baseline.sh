#!/usr/bin/env bash
# Smoke-test baseline: format, type check, lint.
# Run from workspace root before every PR.
set -euo pipefail

cargo fmt --all --check
cargo check --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
cargo clippy --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings
