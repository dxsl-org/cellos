#!/usr/bin/env bash
# Run the immutable Phase08 predesign gate, then the one-hart RV64 manifest
# runtime continuity guard. This is evidence-only: it does not create v3 bytes
# or advance Phase08/ledger readiness.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

python3 scripts/validate-manifest-abi-predesign.py
exec cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test manifest-qemu-continuity -- --nocapture
