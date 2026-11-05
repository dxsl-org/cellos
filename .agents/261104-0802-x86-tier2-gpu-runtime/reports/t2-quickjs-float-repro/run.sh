#!/usr/bin/env bash
# T-2 reproducer: does ocel-quickjs's x86_64 C engine get correct floats across
# its boundary with the cell's soft-float Rust? QEMU-free; runs on an x86_64
# Linux host.
#
#   run.sh [TREE]        TREE = Cellos checkout to test (default: this repo)
#
# 1. Builds `ocel-quickjs` for x86_64-unknown-none in TREE (C = clang, the flags
#    are the cell's own build.rs) and takes its `libquickjs.a`.
# 2. Builds lib.rs for x86_64-unknown-none: libs/api's math exports from TREE,
#    plus the cell's Rust float boundary from TREE (see build.rs).
# 3. Links both into driver.c and runs it. Exit status 1 = a wrong result.
#
# Needs: clang, cc, the repo's pinned nightly with the x86_64-unknown-none
# target, and the workspace's crates in the local cargo cache (`--offline`).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
TREE=$(cd "${1:-$HERE/../../../..}" && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/t2-float-repro.XXXXXX")
echo "tree: $TREE ($(git -C "$TREE" rev-parse --short HEAD 2>/dev/null || echo unknown))"
echo "work: $WORK"

(cd "$TREE" && CC_x86_64_unknown_none=clang cargo build --quiet --release --offline \
    -p ocel-quickjs --target x86_64-unknown-none --target-dir "$WORK/cell")
LIBQJS=$(echo "$WORK"/cell/x86_64-unknown-none/release/build/ocel-quickjs-*/out/libquickjs.a)
[ -f "$LIBQJS" ] || { echo "no engine archive: the cell built its stub" >&2; exit 2; }

# The prebuilt core for this target uses the kernel code model; the program
# below links non-PIE so its absolute relocations resolve.
(cd "$HERE" && T2_TREE="$TREE" RUSTFLAGS="" cargo build --quiet --release --offline \
    --target x86_64-unknown-none --target-dir "$WORK/harness")
LIBRS="$WORK/harness/x86_64-unknown-none/release/libt2_float_repro.a"

echo "C objects: $(nm "$LIBQJS" | awk '$1 == "U" { print $2 }' | sort -u | \
    grep -cxE 'a?(sin|cos|tan)h?|atan2|sqrt|cbrt|pow|exp|expm1|log(2|10|1p)?|fabs|floor|ceil|round|trunc|fmod|hypot|fmin|fmax|copysign|lrint|modf') undefined C99 math symbols"

cc -no-pie -O1 -I "$TREE/cells/services/ocel-quickjs/vendor/quickjs" "$HERE/driver.c" \
    -Wl,--start-group "$LIBQJS" "$LIBRS" -Wl,--end-group -o "$WORK/repro"
"$WORK/repro"
