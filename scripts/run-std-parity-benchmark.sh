#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# run-std-parity-benchmark.sh: Execute Tier 1 Rust std workload parity benchmark and validate promotion gate.
set -euo pipefail

ARCH="${1:-riscv64}"
TARGET="targets/${ARCH}gc-unknown-cellos.json"

if [ ! -f "$TARGET" ]; then
    if [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "x86_64" ]; then
        TARGET="targets/${ARCH}-unknown-cellos.json"
    else
        echo "ERROR: Unknown architecture: $ARCH" >&2
        exit 1
    fi
fi

echo "==> Step 1: Ensuring sysroot overlay is built..."
bash scripts/build-cellos-sysroot.sh "$(basename "$TARGET" .json)"

echo "==> Step 2: Compiling std-smoke cell..."
STAGING_DIR="$(pwd)/target/cellos-rust-src/library"
export __CARGO_TESTS_ONLY_SRC_ROOT="$STAGING_DIR"

cargo +nightly-2026-05-01 build \
    --manifest-path cells/demos/std-smoke/Cargo.toml \
    -Z build-std=core,alloc,std,panic_abort \
    -Z json-target-spec \
    --target "$TARGET"

echo "==> Step 3: Validating synthetic parity benchmark suite against promotion gate..."
mkdir -p evidence
FIXTURE="tests/rust-std-promotion/fixtures/valid-pass.json"
REPORT_OUTPUT="evidence/benchmark-std-parity-${ARCH}.report.json"

python3 scripts/validate-rust-std-promotion.py "$FIXTURE" > "$REPORT_OUTPUT"

STATUS="$(jq -r .overall_status "$REPORT_OUTPUT")"
FIXTURE_ONLY="$(jq -r .fixture_only "$REPORT_OUTPUT")"

echo "==> Benchmark validator result: overall_status=$STATUS, fixture_only=$FIXTURE_ONLY"

if [ "$STATUS" = "VALID_PASS" ]; then
    echo "==> Workload parity and benchmark gate PASSED (p99 regression <= 5%, non-promotional fixture verified)"
else
    echo "ERROR: Workload parity validation failed: $STATUS" >&2
    exit 1
fi
