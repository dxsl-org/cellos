#!/usr/bin/env bash
# Compatibility entry point for the canonical, repo-relative x86 ISO builder.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/../scripts/x86/make-iso-ci.sh" "$@"
