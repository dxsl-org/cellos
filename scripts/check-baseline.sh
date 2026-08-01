#!/usr/bin/env bash
# Smoke-test baseline: format, type check, lint.
# Run from workspace root before every PR.
set -euo pipefail

# Crates whose build scripts compile vendored C needing toolchain pieces a
# CI-equivalent machine does not have. CI excludes exactly this set from both its
# check and clippy steps (.github/workflows/ci.yml); without the same exclusions
# here the script cannot pass on the environment scripts/dev-setup.sh produces,
# so keep the two lists identical.
#
#   lua / tetris-lua      need a full libc (signal.h, stdio.h) — the vendored
#                         freestanding headers supply only string.h
#   doom / tetris-c       link C objects from source clones a checkout lacks
#   app-mlibc-smoke       links a pre-built mlibc libc.a
EXCLUDES=(
  --exclude app-mlibc-smoke
  --exclude doom
  --exclude tetris-c
  --exclude lua
  --exclude tetris-lua
)

TARGET_ARGS=(--target riscv64gc-unknown-none-elf -Z build-std=core,alloc)

cargo fmt --all --check
cargo check --workspace "${EXCLUDES[@]}" "${TARGET_ARGS[@]}"
cargo clippy --workspace "${EXCLUDES[@]}" "${TARGET_ARGS[@]}" -- -D warnings
