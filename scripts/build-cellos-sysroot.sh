#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# build-cellos-sysroot.sh: Build in-tree CellOS Rust std sysroot via content-addressed source overlay.
set -euo pipefail

TARGET="${1:-riscv64gc-unknown-cellos}"
TARGET_SPEC="$(pwd)/targets/${TARGET}.json"

if [ ! -f "$TARGET_SPEC" ]; then
    echo "ERROR: Target spec not found: $TARGET_SPEC" >&2
    exit 1
fi

EXPECTED_COMMIT="f53b654a8"
RUSTC_COMMIT="$(rustc +nightly-2026-05-01 --version --verbose | grep 'commit-hash:' | awk '{print substr($2, 1, 9)}')"

if [ "$RUSTC_COMMIT" != "$EXPECTED_COMMIT" ]; then
    echo "ERROR: Unexpected rustc commit $RUSTC_COMMIT (expected $EXPECTED_COMMIT)" >&2
    exit 1
fi

SYSROOT_BASE="$(rustc +nightly-2026-05-01 --print sysroot)"
RUST_SRC="$SYSROOT_BASE/lib/rustlib/src/rust/library"

if [ ! -d "$RUST_SRC" ]; then
    echo "ERROR: rust-src component not found at $RUST_SRC" >&2
    echo "Run: rustup component add rust-src --toolchain nightly-2026-05-01" >&2
    exit 1
fi

STAGING_DIR="$(pwd)/target/cellos-rust-src/library"
PATCH_FILE="$(pwd)/patches/rust-std-cellos.patch"

if [ ! -f "$PATCH_FILE" ]; then
    echo "ERROR: Patch file not found: $PATCH_FILE" >&2
    exit 1
fi

mkdir -p "target/cellos-rust-src"
if [ ! -d "$STAGING_DIR" ]; then
    echo "==> Staging rust-src library from $RUST_SRC..."
    cp -a "$RUST_SRC" "$STAGING_DIR"
    echo "==> Applying CellOS PAL patch ($PATCH_FILE)..."
    (cd "$STAGING_DIR/.." && patch -p1 < "$PATCH_FILE")
else
    echo "==> Staging directory already exists: $STAGING_DIR"
fi

echo "==> Building CellOS std sysroot for target $TARGET..."
export __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR"
export RUST_BACKTRACE=1

# Compile std library check
cargo +nightly-2026-05-01 check \
    -Z build-std=core,alloc,std \
    -Z json-target-spec \
    --target "$TARGET_SPEC" \
    --manifest-path "libs/api/Cargo.toml"

echo "==> Sysroot validation for $TARGET PASS"
