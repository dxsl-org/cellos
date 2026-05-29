#!/usr/bin/env bash
# Measure line coverage for the ViOS kernel and libs using cargo llvm-cov.
# Requires: llvm-tools-preview component (pinned in rust-toolchain.toml).
# Usage: bash scripts/measure-coverage.sh
set -euo pipefail

cargo llvm-cov \
  --workspace \
  --target riscv64gc-unknown-none-elf \
  -Z build-std=core,alloc \
  --lcov \
  --output-path lcov.info \
  2>&1 | tee coverage.log

echo "Coverage report written to lcov.info"
echo "Open with: genhtml lcov.info -o coverage/"
