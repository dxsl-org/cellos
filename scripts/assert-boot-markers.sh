#!/usr/bin/env bash
# Assert that a cleaned QEMU serial log contains the required boot markers.
#
# Usage:
#   scripts/assert-boot-markers.sh path/to/qemu.log label \
#     "desc:::literal marker" ...

set -euo pipefail

LOG_PATH="${1:?missing log path}"
LABEL="${2:?missing label}"
shift 2

if [[ ! -f "$LOG_PATH" ]]; then
  echo "FAIL: ${LABEL} log not found: $LOG_PATH" >&2
  exit 1
fi

missing=()
for spec in "$@"; do
  desc="${spec%%:::*}"
  marker="${spec#*:::}"
  if ! grep -Fqa -- "$marker" "$LOG_PATH"; then
    missing+=("${desc}: ${marker}")
  fi
done

if [[ "${#missing[@]}" -ne 0 ]]; then
  echo "FAIL: ${LABEL} missing boot markers" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  tail -40 "$LOG_PATH" >&2
  exit 1
fi

echo "PASS: ${LABEL} boot markers present"
