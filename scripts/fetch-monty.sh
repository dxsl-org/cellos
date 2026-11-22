#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# fetch-monty.sh — Fetch pydantic/monty at MONTY_PIN and apply the Cellos platform patches.
#
# Monty (MIT) stays an upstream dependency: nothing from it is committed. This
# script checks the pin out into the gitignored third_party/monty-src/ and
# applies third_party/monty/patches/*.patch in order (see third_party/README.md).
# Cells depend on third_party/monty-src/crates/{monty,monty-types} by path.
#
# Pin: tag v1.7.0-beta.1. It is the first release line whose ruff_python_parser
# (0.0.21) makes `stacker` optional, which the cellos patch set relies on, and
# its rust-version (1.98) is met by the repo toolchain. The pin is a commit
# hash, so a moved tag cannot change what is built. Moving the pin is a
# deliberate edit: re-check every patch and rerun scripts/x86/qemu-monty-smoke.sh.
#
# Usage: bash scripts/fetch-monty.sh
set -euo pipefail

MONTY_PIN="5f4a3dfc493538bb9dc319974393391ba9e7cf14"
MONTY_REPO="https://github.com/pydantic/monty.git"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_DIR="$REPO_ROOT/third_party/monty-src"
PATCH_DIR="$REPO_ROOT/third_party/monty/patches"

[[ -d "$SRC_DIR/.git" ]] || git init -q "$SRC_DIR"
# Re-pointed every run: an interrupted first run or a stale URL cannot persist.
git -C "$SRC_DIR" remote set-url origin "$MONTY_REPO" 2>/dev/null \
    || git -C "$SRC_DIR" remote add origin "$MONTY_REPO"
if ! git -C "$SRC_DIR" cat-file -e "$MONTY_PIN^{commit}" 2>/dev/null; then
    git -C "$SRC_DIR" fetch -q --depth 1 origin "$MONTY_PIN"
fi
git -C "$SRC_DIR" checkout -q -f "$MONTY_PIN"
# Drops the previous run's patches and any build output inside the checkout.
git -C "$SRC_DIR" clean -q -ffdx

for patch in "$PATCH_DIR"/*.patch; do
    git -C "$SRC_DIR" apply "$patch"
    echo "applied ${patch#"$REPO_ROOT"/}"
done
echo "monty @ $(git -C "$SRC_DIR" rev-parse HEAD)"

# Patched dependency crates, wired in by cells/demos/monty-smoke's [patch.crates-io].
# Each is the crates.io tarball at the exact version and checksum pinned here (a
# path-patched crate has no checksum in Cargo.lock), re-extracted on every run,
# then patched.
DEPS_DIR="$REPO_ROOT/third_party/monty-deps"
# jiter: its x86_64 SIMD paths assume SSE2, which the cellos x86_64 spec disables.
JITER_VERSION="0.19.0"
JITER_SHA256="4027f86130c7a1997f880ed407580e58514b867af49e7ce44370f80fa6ff999f"

mkdir -p "$DEPS_DIR"
crate="$DEPS_DIR/jiter-$JITER_VERSION.crate"
if ! { [[ -f "$crate" ]] && echo "$JITER_SHA256  $crate" | sha256sum -c --status; }; then
    curl -sSfL -o "$crate" "https://static.crates.io/crates/jiter/jiter-$JITER_VERSION.crate"
    echo "$JITER_SHA256  $crate" | sha256sum -c --status \
        || { echo "FAIL: jiter-$JITER_VERSION.crate checksum mismatch" >&2; exit 1; }
fi
rm -rf "$DEPS_DIR/jiter-$JITER_VERSION"
tar -xzf "$crate" -C "$DEPS_DIR"
for patch in "$PATCH_DIR"/jiter/*.patch; do
    (cd "$REPO_ROOT" && git apply --directory="third_party/monty-deps/jiter-$JITER_VERSION" "$patch")
    echo "applied ${patch#"$REPO_ROOT"/}"
done
