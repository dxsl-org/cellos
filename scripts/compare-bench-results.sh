#!/usr/bin/env bash
# Validate an explicitly selected benchmark capture and compare compatible history.
#
# Usage:
#   scripts/compare-bench-results.sh RESULTS_DIR \
#     --current RESULTS_DIR/perf-CAPTURE_ID.json --current-id CAPTURE_ID

set -euo pipefail

if ! command -v python3 >/dev/null 2>&1; then
  echo "[compare] python3 is required" >&2
  exit 2
fi

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$SCRIPT_DIR/bench_results.py" compare "$@"
