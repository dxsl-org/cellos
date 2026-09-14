#!/usr/bin/env bash
# Assert that the frozen AI interface still matches the Law 1 record.
#
# The record (.agents/260913-2002-g2-level-a-ai-inference/law1-confirmation.md) lists exactly what the
# owner confirmed 2x on 2026-09-14. Whole-file digests are recorded there as provenance, but this check
# asserts the *confirmed surface* — constant values, wire variants, opcode numbering, and the client
# method names — so an unrelated edit elsewhere in the same file does not trip it, while a removal,
# rename, or value change does.
#
# Exit codes: 0 everything matches, 1 a confirmed item changed (needs the ABI process), 2 setup problem.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROTO="libs/ai-proto/src/lib.rs"
PROTO_CODES="libs/ai-proto/src/lib.rs"
SDK="libs/ai-sdk/src/lib.rs"
SDK_TRANSPORT="libs/ai-sdk/src/transport.rs"
SDK_OSTD="libs/ai-sdk/src/ostd_transport.rs"
ABI="libs/api/src/abi/syscall.rs"
SPEC17="docs/specs/17-ipc-wire-contract.md"

for file in "$PROTO" "$SDK" "$SDK_TRANSPORT" "$SDK_OSTD" "$ABI" "$SPEC17"; do
    [[ -f "$file" ]] || {
        echo "FAIL: required source missing: $file" >&2
        exit 2
    }
done

failures=0

# Literal match: the confirmed items are code text, and several contain regex metacharacters.
check_pattern() {
    local file="$1" pattern="$2" what="$3"
    if ! grep -Fq -- "$pattern" "$file"; then
        echo "FAIL: ${file}: missing ${what}" >&2
        failures=$((failures + 1))
    fi
}

# 1. Size/uniform limits keep their confirmed values (record item 7).
check_const() {
    local name="$1" ty="$2" value="$3"
    if ! grep -Eq "pub const ${name}: ${ty} = ${value};" "$PROTO"; then
        echo "FAIL: ${PROTO}: ${name} is no longer '${ty} = ${value}'" >&2
        failures=$((failures + 1))
    fi
}

check_const AI_IPC_BUF_SIZE usize 4096
check_const MAX_PROMPT_BYTES usize 2048
check_const MAX_REPLY_TEXT_BYTES usize 2048
check_const TOKEN_ID_BYTES usize 4
check_const EMBED_VALUE_BYTES usize 4
check_const MAX_TOKENS_PER_POLL u8 16
check_const MAX_TOKENS_PER_REQUEST u16 1024
check_const MAX_EMBED_DIM u16 768
check_const MAX_SESSIONS u8 4
check_const MAX_TEMPERATURE_MILLI u16 4000

# 2. Opcode registry keeps the ratified Spec 24 §6 numbers (record item 5).
check_opcode() {
    local name="$1" value="$2"
    if ! grep -Eq "pub const ${name}: u16 = ${value};" "$PROTO_CODES"; then
        echo "FAIL: ${PROTO_CODES}: opcode ${name} is no longer ${value}" >&2
        failures=$((failures + 1))
    fi
}

check_opcode DESCRIBE 0x0600
check_opcode INFER_SUBMIT 0x0601
check_opcode INFER_STREAM_POLL 0x0602
check_opcode INFER_CANCEL 0x0603
check_opcode INFER_EMBED 0x0604

# 3. Wire variants and the service id survive (record items 1-4).
check_pattern "$PROTO" "    Describe," "AiRequest variant Describe"
check_pattern "$PROTO" "    InferSubmit(" "AiRequest variant InferSubmit"
check_pattern "$PROTO" "    InferStreamPoll {" "AiRequest variant InferStreamPoll"
check_pattern "$PROTO" "    InferCancel {" "AiRequest variant InferCancel"
check_pattern "$PROTO" "    InferEmbed {" "AiRequest variant InferEmbed"
check_pattern "$PROTO" "    Description(" "AiResponse variant Description"
check_pattern "$PROTO" "    Accepted {" "AiResponse variant Accepted"
check_pattern "$PROTO" "    TokenChunk {" "AiResponse variant TokenChunk"
check_pattern "$PROTO" "    Embedding {" "AiResponse variant Embedding"
check_pattern "$PROTO" "    Failed {" "AiResponse variant Failed"
check_pattern "$ABI" "pub const AI: u16 = 15;" "service::AI = 15"
check_pattern "$SPEC17" "AiRequest" "Spec 17 registry row for the AI service"

# 4. Client surface (record items 9-11).
check_pattern "$SDK" "pub struct AiClient" "AiClient"
check_pattern "$SDK" "pub fn new(transport" "AiClient::new"
check_pattern "$SDK" "pub fn with_device(" "AiClient::with_device"
check_pattern "$SDK" "pub fn describe(&mut self)" "AiClient::describe"
check_pattern "$SDK" "pub fn submit(&mut self" "AiClient::submit"
check_pattern "$SDK" "pub fn poll(&mut self" "AiClient::poll"
check_pattern "$SDK" "pub fn cancel(&mut self" "AiClient::cancel"
check_pattern "$SDK" "pub fn embed(&mut self" "AiClient::embed"
check_pattern "$SDK" "pub fn generate(&mut self" "AiClient::generate"
check_pattern "$SDK" "pub async fn prompt(&mut self" "AiClient::prompt"
check_pattern "$SDK" "pub struct TokenStream" "TokenStream"
check_pattern "$SDK" "pub fn request_id(&self)" "TokenStream::request_id"
check_pattern "$SDK" "pub fn text(&self)" "TokenStream::text"
check_pattern "$SDK" "pub enum AiClientError" "AiClientError"
check_pattern "$SDK" "impl From<AiClientError> for crate::types::ViError" "the ViError conversion"
check_pattern "$SDK_TRANSPORT" "fn round_trip<'r>(" "AiTransport::round_trip"
check_pattern "$SDK_OSTD" "impl AiTransport for OstdTransport" "the bare-metal transport"

# 5. Semantics the confirmation pinned explicitly (record §1).
check_pattern "$PROTO" "(AiResponse::TokenChunk { .. }, AiRequest::InferCancel { .. })" \
    "the cancel/terminal-chunk pairing"
check_pattern "$PROTO" "(AiResponse::Failed { .. }, _)" "the Failed-matches-any-request rule"

if [[ "$failures" -ne 0 ]]; then
    echo "" >&2
    echo "The AI interface changed away from its Law 1 record ($failures item(s))." >&2
    echo "Removal, rename, layout/discriminant change, or addition now requires the ABI process" >&2
    echo "(two fresh explicit confirmations); if this change WAS confirmed, update" >&2
    echo ".agents/260913-2002-g2-level-a-ai-inference/law1-confirmation.md in the same commit." >&2
    exit 1
fi

echo "OK: the frozen AI interface matches its Law 1 record"
