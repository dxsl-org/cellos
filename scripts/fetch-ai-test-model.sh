#!/usr/bin/env bash
# Fetch the real-weight checkpoint used by the Phase 04 host validation of libs/ai-engine.
#
# The weights are NOT part of the repository: they are third-party artifacts, so they land in
# `.ai-models/` (gitignored) and are verified against a pinned digest. `libs/ai-engine`'s
# `generates_text_from_a_real_checkpoint` test skips itself when the file is absent, so a checkout
# without this step still has a green host suite.
#
# Available checkpoints (all fetched into a gitignored cache, never committed):
#
#   smollm     QuantFactory/SmolLM-135M-Instruct-GGUF, Q8_0 - a 30-layer instruction-tuned fine-tune
#              (`general.name = "Cosmo2 135M Webinst Sc2"`), byte-level BPE vocabulary with control
#              tokens and its own chat template, tied output. 138 MB.
#   stories260k  ggml-org/models-moved `tinyllamas/stories260K.gguf` - llama2.c's 260K-parameter
#              story model: real trained weights, SentencePiece vocabulary, F32 tensors, 1.2 MB.
#              Small enough for a Cell's VA slot; this is what Phase 05 serves from `/bin/ai`.
#   stories15m   ggml-org/models-moved `tinyllamas/stories15M-q8_0.gguf` - the same family at 15M
#              parameters (SentencePiece, Q8_0, untied output). Host-side only today: 26.7 MB of
#              weights exceed a Cell slot while the engine copies tensors.
#
# Licence: llama2.c (the stories models) is MIT and its weights come from TinyStories training runs;
# the model-zoo repo declares no license field, so these stay in the gitignored cache. SmolLM's base
# weights are Apache-2.0 (HuggingFaceTB).
#
# Usage: scripts/fetch-ai-test-model.sh [--model NAME] [--dest DIR]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEST_DIR="$ROOT/.ai-models"
MODEL="smollm"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --model)
            MODEL="$2"
            shift 2
            ;;
        --dest)
            DEST_DIR="$2"
            shift 2
            ;;
        *)
            echo "usage: $0 [--model smollm|stories260k|stories15m] [--dest DIR]" >&2
            exit 2
            ;;
    esac
done

case "$MODEL" in
    smollm)
        URL="https://huggingface.co/QuantFactory/SmolLM-135M-Instruct-GGUF/resolve/main/SmolLM-135M-Instruct.Q8_0.gguf"
        EXPECTED_SHA256="76520babb0daebccb6e17d2f38504ece61356a0ca958d8e8795ef4d23c23c1f0"
        NAME="SmolLM-135M-Instruct.Q8_0.gguf"
        ;;
    stories260k)
        URL="https://huggingface.co/ggml-org/models-moved/resolve/main/tinyllamas/stories260K.gguf"
        EXPECTED_SHA256="270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d"
        NAME="stories260K.gguf"
        ;;
    stories15m)
        URL="https://huggingface.co/ggml-org/models-moved/resolve/main/tinyllamas/stories15M-q8_0.gguf"
        EXPECTED_SHA256="2eda49203f2f044f3dddf29a7dd7cc861ef5a0340f518a19613d73ba6d9c06b6"
        NAME="stories15M-q8_0.gguf"
        ;;
    *)
        echo "FAIL: unknown model '$MODEL' (expected smollm, stories260k, or stories15m)" >&2
        exit 2
        ;;
esac

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
