#!/usr/bin/env bash
set -euxo pipefail

if [ "$#" -lt 6 ]; then
  echo "Usage: $0 <manifest> <bundle.jsonl> <repo> <revision> <run-id:attempt> <sequence-store> [--gh-binary <path>]" >&2
  exit 1
fi

MANIFEST="$1"
BUNDLE="$2"
REPO="$3"
REVISION="$4"
SEQUENCE="$5"
SEQUENCE_STORE="$6"
GH_BIN="gh"
if [ "$#" -ge 8 ] && [ "$7" = "--gh-binary" ]; then
  GH_BIN="$8"
fi

if ! command -v "$GH_BIN" >/dev/null 2>&1; then
  echo "Error: gh cli not found" >&2
  exit 1
fi

SIGNER_WORKFLOW="github.com/${REPO}/.github/workflows/ci.yml@refs/heads/main"
MANIFEST_WORKFLOW="${REPO}/.github/workflows/ci.yml@refs/heads/main"
VERIFY_RESULT="$(mktemp)"
trap 'rm -f "$VERIFY_RESULT"' EXIT
echo "Verifying attestation bundle for $MANIFEST (repo: $REPO)..."
if ! "$GH_BIN" attestation verify "$MANIFEST" --bundle "$BUNDLE" --repo "$REPO" \
  --signer-workflow "$SIGNER_WORKFLOW" --deny-self-hosted-runners --format json > "$VERIFY_RESULT"; then
  echo "FAIL: Attestation verification rejected the bundle." >&2
  exit 1
fi
ATTESTED_SHA256="$(python3 scripts/extract-attested-subject-digest.py "$VERIFY_RESULT")"
python3 scripts/verify-authenticated-evidence.py "$MANIFEST" \
  --expected-revision "$REVISION" --expected-sequence "$SEQUENCE" \
  --expected-workflow-ref "$MANIFEST_WORKFLOW" \
  --expected-runner-prefix "github-hosted:" \
  --expected-manifest-sha256 "$ATTESTED_SHA256"
python3 scripts/consume-evidence-sequence.py \
  --store "$SEQUENCE_STORE" --repository "$REPO" \
  --workflow-ref "$MANIFEST_WORKFLOW" --sequence "$SEQUENCE" \
  --manifest "$MANIFEST" --expected-manifest-sha256 "$ATTESTED_SHA256"
echo "PASS: attestation, contents, and sequence consumption verified."
