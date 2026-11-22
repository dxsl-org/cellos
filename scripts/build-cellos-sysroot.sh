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

TOOLCHAIN="nightly-2026-11-21"
EXPECTED_COMMIT="c7d5ba59d"
RUSTC_COMMIT="$(rustc +"$TOOLCHAIN" --version --verbose | grep 'commit-hash:' | awk '{print substr($2, 1, 9)}')"

if [ "$RUSTC_COMMIT" != "$EXPECTED_COMMIT" ]; then
    echo "ERROR: Unexpected rustc commit $RUSTC_COMMIT (expected $EXPECTED_COMMIT)" >&2
    exit 1
fi

SYSROOT_BASE="$(rustc +"$TOOLCHAIN" --print sysroot)"
RUST_SRC="$SYSROOT_BASE/lib/rustlib/src/rust/library"

if [ ! -d "$RUST_SRC" ]; then
    echo "ERROR: rust-src component not found at $RUST_SRC" >&2
    echo "Run: rustup component add rust-src --toolchain $TOOLCHAIN" >&2
    exit 1
fi

STAGING_DIR="$(pwd)/target/cellos-rust-src/library"
PATCH_FILE="$(pwd)/patches/rust-std-cellos.patch"

if [ ! -f "$PATCH_FILE" ]; then
    echo "ERROR: Patch file not found: $PATCH_FILE" >&2
    exit 1
fi

# The staged tree depends on both the patch and the rust-src it was applied to;
# keying on the rustc commit too makes a toolchain move (or rollback) restage.
PATCH_DIGEST="$(sha256sum "$PATCH_FILE" | awk '{print $1}')-$RUSTC_COMMIT"
STAMP_FILE="target/cellos-rust-src/.patch.sha256"

mkdir -p "target/cellos-rust-src"
if [ ! -d "$STAGING_DIR" ] || [ "$(cat "$STAMP_FILE" 2>/dev/null || true)" != "$PATCH_DIGEST" ]; then
    echo "==> Staging rust-src library from $RUST_SRC..."
    rm -rf "$STAGING_DIR"
    cp -a "$RUST_SRC" "$STAGING_DIR"
    echo "==> Applying CellOS PAL patch ($PATCH_FILE)..."
    (cd "$STAGING_DIR/.." && patch -p1 < "$PATCH_FILE")
    printf '%s\n' "$PATCH_DIGEST" > "$STAMP_FILE"
else
    echo "==> Staging directory up to date: $STAGING_DIR"
fi

echo "==> Building CellOS std sysroot for target $TARGET..."
export __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR"
export RUST_BACKTRACE=1

# Compile std library check
cargo +"$TOOLCHAIN" check \
    -Z build-std=core,alloc,std \
    -Z json-target-spec \
    --target "$TARGET_SPEC" \
    --manifest-path "Cargo.toml" -p types -p abi -p wire -p posix-shim

echo "==> Sysroot validation for $TARGET PASS"
