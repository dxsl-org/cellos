#!/usr/bin/env bash
set -euxo pipefail

if [ "$#" -lt 5 ]; then
  echo "Usage: $0 <manifest> <bundle.jsonl> <repo> <revision> <run-id:attempt> [--gh-binary <path>]" >&2
  exit 1
fi

MANIFEST="$1"
BUNDLE="$2"
REPO="$3"
REVISION="$4"
SEQUENCE="$5"
GH_BIN="gh"
if [ "$#" -ge 7 ] && [ "$6" = "--gh-binary" ]; then
  GH_BIN="$7"
fi

if ! command -v "$GH_BIN" >/dev/null 2>&1; then
  echo "Error: gh cli not found" >&2
  exit 1
fi

SIGNER_WORKFLOW="github.com/${REPO}/.github/workflows/ci.yml@refs/heads/main"
echo "Verifying attestation bundle for $MANIFEST (repo: $REPO)..."
if ! "$GH_BIN" attestation verify "$MANIFEST" --bundle "$BUNDLE" --repo "$REPO" \
  --signer-workflow "$SIGNER_WORKFLOW" --deny-self-hosted-runners; then
  echo "FAIL: Attestation verification rejected the bundle." >&2
  exit 1
fi
python3 scripts/verify-authenticated-evidence.py "$MANIFEST" \
  --expected-revision "$REVISION" --expected-sequence "$SEQUENCE"
echo "PASS: attestation and authenticated evidence contents verified."
