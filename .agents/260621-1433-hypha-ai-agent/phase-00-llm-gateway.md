# Phase 00 — `llm-gateway` Cell

## Context Links
- [plan.md](./plan.md) · [architecture.md](./architecture.md) · [os-gaps.md](./os-gaps.md)
- Reference cell: [cells/demos/https-demo/src/main.rs](../../cells/demos/https-demo/src/main.rs)
- TLS API: [libs/ostd/src/tls.rs:31](../../libs/ostd/src/tls.rs#L31)

## Overview
- **Priority**: P0 (foundation — everything else depends on reaching an LLM).
- **Status**: ✅ Code complete — builds for riscv64 (ET_DYN PIE); `hypha-boot` integration test covers spawn path; live LLM round-trip deferred to user env step (mock proxy).
- **Description**: A standalone Cell that owns the `network` capability + the LLM API key, accepts
  a prompt over IPC, performs an HTTPS request to an LLM endpoint, and returns the completion.
  No tools, no streaming yet — prove the round-trip.

## Key Insights
- ViCell has **no HTTP library** (os-gap G1) — we hand-roll HTTP/1.1 over the raw TLS helpers
  `tls_connect/write/read/close` ([tls.rs:31](../../libs/ostd/src/tls.rs#L31)). `https-demo`
  already proves the exact call sequence.
- **DNS over QEMU NAT is unverified** (os-gap G3). De-risk by targeting a **host-side LLM proxy**
  at a pinned IP via `10.0.2.2`, not a public hostname, for this phase.
- Prompt/response exceed the 4096B IPC buffer → use **Grant** ([syscall.rs:957](../../libs/ostd/src/syscall.rs#L957)).
- JSON bodies need a **no_std JSON** dep (os-gap G4) — pick and validate it here.

## Requirements
**Functional**
1. Register as a service (a Hypha-private service id; see os-gap G7 — for P0 a single fixed id is fine).
2. Receive `LlmRequest::Complete { grant_id, len }`; read the prompt JSON from the Grant.
3. Open TLS to the proxy, send a valid HTTP/1.1 POST with the JSON body, read the full response.
4. Parse the (non-streaming) JSON completion; reply `LlmReply::Text(..)` (ToolCalls deferred to P2).
5. Read the API key from a file (`/etc/hypha/llm.key` or `/data/...`) — never embedded in the binary.

**Non-functional**
- `#![forbid(unsafe_code)]`; owned buffers across any async boundary (Law 2).
- Manifest declares **only** `network`. No spawn, no block_io beyond what Grant needs.
- Graceful errors (`ViResult`) on connect/timeout/HTTP-error — no panic.

## Architecture
- Single Cell, `app_entry!(network = true, handler = ...)`
  ([runtime.rs:177](../../libs/ostd/src/runtime.rs#L177)).
- Request handling synchronous-with-yields like `https-demo` (TLS client path is blocking).
- Data flow: core → Grant(prompt) → gateway reads Grant → TLS POST → response buffer →
  parse → `LlmReply::Text` over IPC (small) or Grant (if large).

## Related Code Files
- **Create**: `cells/apps/hypha/llm-gateway/` (`Cargo.toml`, `build.rs`, `src/main.rs`, linker via `cell-build`).
- **Create**: `libs/agent-proto/` (shared `LlmRequest`/`LlmReply` + `AgentToolRequest/Response`).
- **Reference (read-only)**: `https-demo`, `cells/services/net/`.

## Implementation Steps
1. Scaffold `libs/agent-proto` with postcard enums (LlmRequest/LlmReply minimal).
2. Scaffold the gateway cell (Cargo.toml + build.rs emitting linker script + manifest `network=true`).
3. Port the TLS connect→write→read→close sequence from `https-demo`; replace GET with POST + body.
4. Add no_std JSON dep (os-gap G4); build request body + parse response `content`.
5. Wire IPC: recv `LlmRequest`, read prompt from Grant, send `LlmReply`.
6. Stand up a trivial host proxy (e.g. a local script forwarding to the real LLM) and pin its IP.
7. Boot on QEMU; verify a real completion round-trips.

## Todo List
- [x] `libs/agent-proto` crate with LlmRequest/LlmReply (+ ToolCall/AgentTool* contract) — builds
- [x] gateway cell scaffold (manifest `network=false`, syscalls `[Send,Recv,Log,LookupService]`)
- [x] HTTP/1.0 POST over TLS (hand-rolled, `http.rs`)
- [x] JSON body build + content extract (hand-rolled for P0; no_std JSON dep deferred)
- [x] registered in root `Cargo.toml` + `gen_disk.ps1` (build + `/bin/llm-gateway` embed)
- [x] **builds for riscv64** (ET_DYN PIE, 63 KB) — `cargo build --release -p hypha-llm-gateway` green
- [ ] Grant-based prompt read — deferred (P0 is standalone, hardcoded prompt; needed P1+)
- [x] host LLM proxy at 10.0.2.2:8080 (plaintext `--plain` mode) — **confirmed working (boot run #2)**
- [x] boot + round-trip verified on QEMU — **CONFIRMED 2026-06-22** (response bytes: 359, reply printed)

> Scope note: P0 first cut is a **standalone** proof (hardcoded prompt → TLS POST → print reply),
> not yet a service. The IPC service form (recv `LlmRequest`, reply `LlmReply`, Grant for large
> prompts) moves to P0b/P1 when `core` exists to call it.

## Success Criteria
- A boot run where Hypha-side test sends a prompt and the gateway returns the LLM's text completion,
  observed in the console. No panics; key loaded from file, not embedded.
- os-gaps G1/G3/G4 each updated with their disposition (workaround vs module) after this phase.

## Risk Assessment
- **LLM reachability (G3, 🔴)** — mitigate with host proxy + pinned IP; validate NAT path separately.
- **Hand-rolled HTTP correctness (G1, 🟡)** — keep to minimal HTTP/1.1, `Connection: close`,
  fixed `Content-Length`; avoid chunked/streaming this phase (defer to G2).
- **JSON dep fit (G4)** — confirm chosen crate is no_std + builds under `forbid(unsafe)`.

## Security Considerations
- API key isolated to this Cell (only Cell with `network`); stored in a file, gateway-only read.
- TLS cert validation on (client mode) — same path as `https-demo`.
- No spawn capability — gateway cannot launch anything.

## Next Steps
- P1: `hypha` core consumes this gateway for a plain chat loop (shell/UART I/O).
- Promote hand-rolled HTTP to `ostd::http` if a second consumer appears (G1 → module).
