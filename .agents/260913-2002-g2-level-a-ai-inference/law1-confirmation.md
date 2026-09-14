# Law 1 record — AI inference service interface (Spec 24 §6)

**Status**: 2 of 2 confirmations recorded. **FROZEN** (2026-09-14).
**Owner**: sole accountable maintainer (ADR-0013).
**Baseline revision**: `4d385c92` (the interface below is exactly the code at this revision).

Spec 23 §2.1 requires **two explicit confirmations** before an ABI surface is treated as FROZEN, and
Spec 24 CP-1 names the same gate for this interface. This file exists so both confirmations bind to
one list of exact items and the same source digests, rather than to a memory of them.

## 1. What is being confirmed

| # | Item | Location |
|---|---|---|
| 1 | `service::AI = 15`, the well-known service id clients resolve with `LookupService` | `libs/api/src/abi/syscall.rs:1075` |
| 2 | Byte-0 namespace row for the AI service (postcard variant indices, own receiver) | `docs/specs/17-ipc-wire-contract.md` §3 |
| 3 | `AiRequest`: `Describe`, `InferSubmit`, `InferStreamPoll`, `InferCancel`, `InferEmbed` | `libs/ai-proto/src/lib.rs:235-264` |
| 4 | `AiResponse`: `Description`, `Accepted`, `TokenChunk`, `Embedding`, `Failed`, plus the `matches()` reply/request rule | `libs/ai-proto/src/lib.rs:294-357` |
| 5 | Logical opcodes `DESCRIBE 0x0600`, `INFER_SUBMIT 0x0601`, `INFER_STREAM_POLL 0x0602`, `INFER_CANCEL 0x0603`, `INFER_EMBED 0x0604` and the variant-order map | `libs/ai-proto/src/lib.rs:75-99` |
| 6 | Wire types: `InferSubmit`, `Describe`, `DeviceTarget`, `GpuKind`, `NpuKind`, `Quant`, `FinishReason`, `AiError`, `limit::Violation` | `libs/ai-proto/src/lib.rs:113-213, 215-229, 267-292` |
| 7 | Capacity constants: `AI_IPC_BUF_SIZE` 4096, `MAX_PROMPT_BYTES` 2048, `MAX_REPLY_TEXT_BYTES` 2048, `TOKEN_ID_BYTES` 4, `EMBED_VALUE_BYTES` 4, `MAX_TOKENS_PER_POLL` 16, `MAX_TOKENS_PER_REQUEST` 1024, `MAX_EMBED_DIM` 768, `MAX_SESSIONS` 4, `MAX_TEMPERATURE_MILLI` 4000 | `libs/ai-proto/src/lib.rs:41-73, 231` |
| 8 | Numeric payload encodings (`&[u8]` little-endian) and their helpers: `encode_token_ids`, `token_ids`, `encode_embedding_values`, `embedding_values` | `libs/ai-proto/src/lib.rs:408-452` |
| 9 | `AiClient` surface: `new`, `with_device`, `device`, `describe`, `submit`, `poll`, `cancel`, `embed`, `generate`, `prompt`; `InferParams`, `Token`, `Chunk`, `Generation`, `ServiceInfo`, `TokenStream` (`request_id`, `text`, `cancel`, `Iterator<Item = AiResult<Token>>`) | `libs/ai-sdk/src/lib.rs:92-431` |
| 10 | Error contract: `AiClientError` variants and `From<AiClientError> for ViError` | `libs/ai-sdk/src/lib.rs:48-96` |
| 11 | `AiTransport::round_trip` (the only target-specific seam; `OstdTransport` implements it for bare-metal targets) | `libs/ai-sdk/src/transport.rs`, `libs/ai-sdk/src/ostd_transport.rs` |

Semantics that are part of the confirmation, not just signatures:

- A session id is **service-assigned**; `InferSubmit` carries no client-chosen id.
- `InferCancel` is answered with the session's terminal `TokenChunk` (`done: true`,
  `FinishReason::Cancelled`), and `AiResponse::matches` accepts that pairing. A cancel for an unknown
  or already-finished session is reported as `Failed { UnknownRequest }` (idempotent for the client).
- `Failed` may answer **any** request; every other reply variant matches exactly one request variant.
- `DeviceTarget` values other than `Auto`/`Cpu` are refused with `AiError::NotSupported` — never
  silently downgraded. `Describe.backends` reports what the build actually implements.
- Token ids travel as little-endian `u32` in a `&[u8]`; embeddings as little-endian `f32`.

## 2. Recorded deviations from the ratified Spec 24 §6 signature

Both were shipped in the first implementation commit and are part of what this confirmation covers:

1. `prompt` keeps the async shape but its `TokenStream` is a **synchronous iterator**; the async
   reactor (Spec 20 §5) is not landed, and hiding a blocking IPC round trip behind `await` would be
   worse than the visible poll loop. Streaming itself is real (the service holds sessions and the
   caller polls them).
2. Errors are `AiClientError` (with `From<AiClientError> for ViError`) rather than a bare `ViResult`,
   so a caller can distinguish transport failure from a typed service refusal.

## 3. Confirmation log

| # | Date (UTC) | Statement | Binds to |
|---|---|---|---|
| 1 | 2026-09-14 | Owner instruction: "xác nhận Law 1 cho interface AI" — confirmed against the item list above as presented in-session at revision `4d385c92` | `libs/ai-proto/src/lib.rs` sha256 `3d555d58…`, `libs/ai-sdk/src/lib.rs` sha256 `7e4c0fbb…`, `libs/api/src/abi/syscall.rs` sha256 `bc203ac4…`, Spec 24 sha256 `88f7a065…` |
| 2 | 2026-09-14 | Second explicit owner confirmation of the **same** list and digests (selected through an explicit yes/no prompt, not inferred from the first instruction) | identical digests to #1 |

Both confirmations are recorded, so the surface above is **FROZEN** under Spec 23 §2.1: removal,
rename, layout/discriminant change, or addition requires the ABI process (including two fresh explicit
confirmations) before it lands. The `libs/api/src/abi/syscall.rs` row is part of the FROZEN `api::abi`
surface already; the two crates listed here are frozen by this record, whose digests pin exactly which
revision was confirmed — a later commit that changes those files without a new ABI confirmation leaves
this record stale by construction, and CI's digest check (see §4) fails.

## 4. Keeping the record honest

The record is only meaningful while the confirmed surface is unchanged. `scripts/check-ai-law1-digests.sh`
asserts every item in §1 that is expressible as code text — the ten constants and their values, the five
opcode numbers, all ten wire variants, `service::AI = 15`, the Spec 17 row, the `AiClient` methods,
`TokenStream`, `AiClientError` with its `ViError` conversion, `AiTransport::round_trip`, and the two
pinned reply-matching rules — and fails with a per-item message. It is wired into CI (job *AI Inference
Oracle*), so drift is a check failure rather than a claim nobody re-reads. Verified both ways: it passes
on the frozen tree and fails on a one-value mutation (`MAX_SESSIONS: u8 = 4` -> `8`).

Whole-file digests above are provenance for *which revision* was confirmed, not the gate: an unrelated
edit elsewhere in one of those files must not require an ABI confirmation, while a change to a confirmed
item must. After a *confirmed* ABI change, update the table, the digests, and this check in the same
commit as the change.
