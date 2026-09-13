#!/usr/bin/env bash
# Fetch the real-weight checkpoint used by the Phase 04 host validation of libs/ai-engine.
#
# The weights are NOT part of the repository: they are third-party artifacts, so they land in
# `.ai-models/` (gitignored) and are verified against a pinned digest. `libs/ai-engine`'s
# `generates_text_from_a_real_checkpoint` test skips itself when the file is absent, so a checkout
# without this step still has a green host suite.
#
# Model: QuantFactory's Q8_0 quantisation of a SmolLM-135M fine-tune. What the engine sees, verified
# by the Phase 04 test run: `general.name = "Cosmo2 135M Webinst Sc2"`, llama architecture, 30 layers,
# 49,152-token byte-level BPE vocabulary, tied output embedding (no `output.weight` tensor), 138 MB.
# Licence: the base SmolLM-135M weights are Apache-2.0 (HuggingFaceTB); this file is a third-party
# quantisation and is therefore fetched into a gitignored cache rather than committed.
#
# Usage: scripts/fetch-ai-test-model.sh [--dest DIR]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEST_DIR="$ROOT/.ai-models"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dest)
            DEST_DIR="$2"
            shift 2
            ;;
        *)
            echo "usage: $0 [--dest DIR]" >&2
            exit 2
            ;;
    esac
done

URL="https://huggingface.co/QuantFactory/SmolLM-135M-Instruct-GGUF/resolve/main/SmolLM-135M-Instruct.Q8_0.gguf"
EXPECTED_SHA256="76520babb0daebccb6e17d2f38504ece61356a0ca958d8e8795ef4d23c23c1f0"
NAME="SmolLM-135M-Instruct.Q8_0.gguf"

for tool in curl sha256sum mkdir mv rm; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "FAIL: required tool not found: $tool" >&2
        exit 2
    }
done

mkdir -p "$DEST_DIR"
TARGET="$DEST_DIR/$NAME"

verify() {
    local actual
    actual="$(sha256sum "$1" | cut -d' ' -f1)"
    [[ "$actual" == "$EXPECTED_SHA256" ]]
}

if [[ -f "$TARGET" ]] && verify "$TARGET"; then
    echo "OK: $TARGET (digest verified)"
    echo "$TARGET"
    exit 0
fi

echo "[fetch-ai-model] downloading $NAME"
TMP="$TARGET.part"
rm -f -- "$TMP"
curl --fail --location --retry 3 --output "$TMP" "$URL"
if ! verify "$TMP"; then
    echo "FAIL: digest mismatch for $TMP — refusing to keep an unverified checkpoint" >&2
    rm -f -- "$TMP"
    exit 1
fi
mv -- "$TMP" "$TARGET"
echo "OK: $TARGET (digest verified)"
echo "$TARGET"
