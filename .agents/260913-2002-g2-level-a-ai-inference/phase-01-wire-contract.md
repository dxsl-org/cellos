# Phase 01 — Wire Contract (`libs/ai-proto`)

**Status**: completed
**Ceiling**: host
**Law 1**: yes — new `service::AI` const and new service wire namespace.

## Deliverable

- `libs/ai-proto`: `AiRequest`/`AiResponse` postcard enums, `DeviceTarget`, bounded limits,
  `AiError`, `Describe` capability record, and the Spec 24 §6 opcode map.
- `api::syscall::service::AI = 15` (additive; next free ID after `KMS = 13`,
  `HYPERVISOR = 14` lives in `api::abi::hypervisor`).
- Spec 17 §3 byte-0 registry row for the AI service receiver.
- `libs/ai-sdk::AiClient` (submit/poll/cancel/embed/describe + `prompt()`), typed-IPC only.

## Gates

- Host tests: round-trip every variant, reject oversize payloads, reject unknown/forbidden values
  (temperature/top-k/limits), and prove the client's request-id handling never reuses a live id.
- Law 1: two explicit owner confirmations recorded in the changelog before the ABI const lands.

## Result

- `libs/ai-proto` (6/6 host tests), `libs/ai-sdk` (8/8, scripted-transport protocol tests), real
  `ostd_transport` behind `ostd-transport` for bare-metal targets only.
- `api::syscall::service::AI = 15`; Spec 17 §3 amended with the AI namespace row.
- Deviations from the ratified signature, recorded deliberately: `prompt()` keeps the async shape
  but returns a synchronous `TokenStream` (no async reactor yet), and errors are `AiClientError`
  with `From<AiClientError> for ViError` instead of a bare `ViResult`.
- **Law 1: FROZEN** — 2 of 2 confirmations recorded on 2026-09-14 (owner). The item list, the source
  digests both confirmations bind to, and the digest check that keeps the record honest are in
  [`law1-confirmation.md`](./law1-confirmation.md).

## Notes

- The Spec 24 opcode numbers (`0x0601`…) are the logical registry names; the wire selects variants by
  postcard discriminant per Spec 17 §3 (byte-0 range `0x00`–`0x19`). The mapping is asserted by a test.
- `AiClient::prompt` keeps the Spec 24 async signature shape; streaming is a bounded poll loop until
  the async reactor (Spec 20 §5) lands.
