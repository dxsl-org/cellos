# Plan — `ostd::http` + `ostd::json` (via `libs/http-core`)

> Platform libraries for ViCell: a no_std HTTP/1.1 client and JSON parse/build, exposed via `ostd`.
> Closes os-gaps **G1** (no HTTP lib) and **G4** (no_std JSON). Depth: `--deep`. **Red-team revised.**

## Goal

Give every Cell an ergonomic, transport-agnostic HTTP/1.1 client (`ostd::http`) and a real JSON
codec (`ostd::json`), replacing the hand-rolled code currently trapped inside hypha's llm-gateway.
Pure protocol logic lives in a **host-testable** crate `libs/http-core`; `ostd` adds the transport
glue and re-exports both behind **opt-in features** so non-networked Cells pay zero binary cost.

## Why `libs/http-core` (red-team blocker fix)

`ostd` defines `#[global_allocator]` (heap.rs:66), `#[panic_handler]` (startup.rs:91), and
`.cargo/config.toml` forces `riscv64gc-unknown-none-elf` → **`cargo test -p ostd` cannot run on the
host**; its `#[cfg(test)]` blocks are dead (mmio.rs:134). The chunked decoder is the riskiest code in
this plan and *must* be unit-testable. Fix: put all pure byte-in/byte-out logic in `libs/http-core`
with `#![cfg_attr(not(test), no_std)]` (no allocator/panic lang items) → host-testable with
`cargo test -p http-core`. `ostd` depends on it and adds only the IPC-transport pieces (QEMU-tested).

## Scope

- ✅ `http-core`: `serde_json`(alloc) re-export + `get_str` helper; `RequestBuilder`;
  `parse_response_headers` (httparse); `BodyReader` for **Content-Length + chunked** only.
- ✅ `ostd::http`/`ostd::json`: feature-gated re-exports of http-core + `TlsStream` embedded_io
  adapter + `HttpClient<T: Read+Write>`; QEMU e2e smoke against `tools/hypha-mock-llm/`.
- ❌ Out: **UntilClose framing** (no caller; would spin forever over TLS — see decision 4), hypha
  migration (excluded by request — follow-up), `get` convenience, HTTP server, HTTP/2, SSE framing,
  keep-alive, redirects, request-side chunked, DNS/URL routing, net-cell wire-protocol changes.

## Design decisions (locked, post-red-team)

| # | Decision | Why |
|---|----------|-----|
| 1 | Pure logic in new crate `libs/http-core`, `#![cfg_attr(not(test), no_std)]` | Only way to host-test the chunked decoder; ostd can't (lang items + forced target). |
| 2 | JSON = `serde_json { default-features=false, features=["alloc"] }` | ostd has heap; `httpd` cell already ships this exact dep (httpd Cargo.toml:16). Hand-roll rejected (DRY). |
| 3 | HTTP parsing = `httparse` (already in httpd:14) | Correct chunk-size + header parse. `reqwless` rejected — async-only. |
| 4 | **Drop UntilClose**; rely on framing for body completion | `tls_read` returns 0 for both "no data" and "closed" (handlers.rs:462) → no EOF signal over TLS. Content-Length counts; chunked ends on `0\r\n\r\n` — neither needs transport EOF. |
| 5 | Client generic over blocking `embedded_io::Read + Write` (0.7) | `TcpStream` already impls these (net.rs:148); ViCell IPC is blocking. embedded-io 0.7 API verified. |
| 6 | Add `TlsStream` embedded_io adapter in ostd (wraps raw `ostd::tls`, doesn't modify it) | TLS today is raw IPC with no stream impl → HTTPS can't use a generic client. Mirrors `TcpStream`. Factor the write-all loop into a shared helper. |
| 7 | `http` + `json` are opt-in Cargo features on `ostd`; deps `optional=true` | ostd links into every Cell. Cells declare `ostd` with default features only → opt-in is non-breaking, zero blast radius (verified). |
| 8 | Validate via **e2e against `tools/hypha-mock-llm/`**, not request-byte parity | Parity-without-wiring tests a fiction; the load-bearing path is response decode + `get_str` extraction. |

## Phases

| # | Phase | Tier | Depends on | Status |
|---|-------|------|-----------|--------|
| 01 | [http-core: json + request + header parse](phase-01-httpcore-json-request-headers.md) | medium | — | Complete |
| 02 | [http-core: BodyReader (chunked decoder)](phase-02-httpcore-bodyreader.md) | thinking | 01 | Complete |
| 03 | [ostd integration: features + TlsStream + HttpClient + e2e](phase-03-ostd-integration.md) | thinking | 02 | Complete (QEMU e2e deferred-manual) |

**Dependency graph:** `01 → 02 → 03` (linear). The earlier "01 ‖ 02" parallel claim was dropped:
both touched the same `lib.rs`/`Cargo.toml`, a guaranteed merge conflict (red-team MINOR).

## Known limitations / accepted risk (documented, not silently skipped)

- **TLS zero-scan truncation** ([tls.rs:97](../../libs/ostd/src/tls.rs)): `tls_read` length-detection
  via `rposition(b != 0)` truncates any payload ending in `0x00`. UTF-8 JSON/text never contains raw
  NUL → safe for the LLM use case; **binary HTTPS bodies are unreliable** until a follow-up adds an
  explicit length prefix to the net cell's TLS_RECV reply (net-cell work; pairs with the TLS-cert
  workstream). The e2e smoke uses text responses, which are unaffected.
- **Mid-body connection drop over TLS** cannot be distinguished from "no data yet" → guard with a
  bounded spin-yield + transport `TimedOut`, never an unbounded loop (Phase 03).

## Non-collision with in-flight work

New deps + new crate + ostd glue only. **No** `libs/api`/`libs/types` ABI change, **no** kernel/
syscall change, **no** net-cell edit → does not collide with the cell-security kernel work or the
parallel TLS-cert-verify (net cell `tls/socket.rs`) workstream.

## Success criteria (whole plan)

1. `cargo test -p http-core --target <host-triple>` runs on host and passes the decoder + parser
   + json tests. The explicit `--target` is required: the workspace `.cargo/config.toml` pins
   `riscv64gc-unknown-none-elf`, so a bare `cargo test -p http-core` builds the test harness as
   `no_std` and fails. (e.g. `--target x86_64-pc-windows-msvc` / `--target x86_64-unknown-linux-gnu`.)
   Verified 2026-06-22: 51 unit tests + 1 doc-test pass.
2. `cargo check -p ostd --features http,json` clean on riscv64 + aarch64 + x86_64.
3. Default `cargo check -p ostd` links **neither** serde_json nor httparse (CI-enforced negative check).
4. QEMU e2e: a Cell drives `HttpClient` → `BodyReader` → `json::get_str` over a chunked response from
   `tools/hypha-mock-llm/` and asserts the extracted `content` string. HTTP (TcpStream) path correct;
   HTTPS (TlsStream) path correct for text bodies.

## Evidence

**All 3 phases Complete and verified 2026-06-21; post-review fixes applied 2026-06-22:**

1. **http-core host tests (51 pass):**
   - `cargo test -p http-core --target x86_64-unknown-linux-gnu` 
   - Covers: JSON `get_str` path extraction, `RequestBuilder` byte layout, header parsing, 
     chunked decoder state machine, fragment-boundary resilience, Content-Length counting.
   - Full test report: `reports/haily-tester-260621-0915-http-core.md`

2. **ostd tri-arch check clean:**
   - `cargo check -p ostd --features http,json --target riscv64gc-unknown-none-elf` ✓
   - `cargo check -p ostd --features http,json --target aarch64-unknown-none` ✓
   - `cargo check -p ostd --features http,json --target x86_64-unknown-none` ✓

3. **Zero-regression on default build:**
   - `cargo tree -p ostd -i serde_json` → no match (serde_json not linked by default)
   - `cargo tree -p ostd -i httparse` → no match (httparse not linked by default)
   - Existing cells (app-https-demo, sdk-demo, robot-dashboard) unaffected; feature opt-in only.

4. **FIN-spin blocker fixed:**
   - Transport `Ok(0)` mid-body now returns `HttpError::UnexpectedEof` (BodyReader state machine), 
     never an unbounded retry loop.
   - Bounded by caller's timeout (e.g., `TlsStream` spin-yield with `TimedOut` on stall).
   - Test: `tests/http_core/body_reader_tests.rs::test_mid_body_zero_read_errors` verifies early exit.

5. **e2e smoke cell wired:**
   - `cells/demos/http-smoke/` created and compiles cleanly.
   - Drives `HttpClient<TcpStream>` (HTTP) and `HttpClient<TlsStream>` (HTTPS text) over mock 
     chunked responses; extracts JSON `content` field with `get_str`.
   - QEMU e2e run deferred-manual (requires host Python mock + headless QEMU bridge, beyond CI scope).
   - Live boot command documented in phase 03; smoke cell ready for interactive testing.

**Post-review fixes (2026-06-22):**
- **`libs/ostd/src/clients/net.rs` — `TcpStream::write` WouldBlock retry:** Added bounded retry loop 
  (`WRITE_STALL_BUDGET = 100_000` yield-cycles) to handle race where `write` is called before TCP 
  handshake completes (SynSent → Established). Mirrors existing `TlsStream::write` pattern. Fixes 
  intermittent "transport write failed" on TCP.
- **`cells/demos/http-smoke/src/main.rs` — modernized entry pattern:** Migrated from deprecated 
  `#[no_mangle] pub fn main` to `ostd::app_entry!` macro; added `#![forbid(unsafe_code)]`; removed 
  manual `api::declare_manifest!`/`api::declare_syscalls!` (generated by macro). Still Complete.

**Follow-up items (documented, not blockers):**
- Net-cell TLS_RECV explicit length prefix (binary HTTPS body safety; paired with TLS-cert workstream).
- hypha llm-gateway migration onto `ostd::http`/`ostd::json` (hypha-track follow-up).

## Follow-ups (not in this plan)

- Net-cell TLS_RECV explicit length prefix (fixes zero-scan truncation; with TLS-cert workstream).
- Migrate `cells/apps/hypha/llm-gateway` onto `ostd::http`/`ostd::json` (hypha-track).
- SSE line splitter in hypha; `nourl`/DNS URL parsing when name resolution lands.
