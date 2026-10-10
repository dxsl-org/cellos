#!/usr/bin/env bash
# Reuse the signed, production-feature C2C image lane with the native renderer
# consumer instead of the lifecycle fixtures. Outputs stay isolated from both.
set -euo pipefail
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
export C2C_WORKLOAD=render
exec bash "$SCRIPT_DIR/build-x86_64-c2c-lifecycle-ci.sh" "$@"
