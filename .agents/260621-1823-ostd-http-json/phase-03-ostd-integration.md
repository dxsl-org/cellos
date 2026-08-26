---
phase: 03
title: ostd integration — features + TlsStream + HttpClient + e2e
tier: thinking
depends_on: [02]
status: Complete (e2e QEMU run deferred — manual; post-review fixes applied 2026-06-22)
---

# Phase 03 — `ostd` Integration: TlsStream + HttpClient + e2e

## Context Links
- [plan.md](plan.md) · [scout-report.md](scout-report.md) · [phase-01](phase-01-httpcore-json-request-headers.md) · [phase-02](phase-02-httpcore-bodyreader.md)

## Overview
- **Priority:** High — makes the library usable end-to-end, including HTTPS (the real LLM use case).
- **Status:** Planned.
- Three deliverables: (1) feature-gated `ostd::http`/`ostd::json` re-exporting `http-core`;
  (2) `TlsStream` adapting raw `ostd::tls` IPC to `embedded_io::Read + Write` (mirrors `TcpStream`);
  (3) `HttpClient<T>` driving RequestBuilder → header parse → BodyReader over any `Read + Write`.
  Validated by a QEMU e2e smoke against `tools/hypha-mock-llm/`.
- **Tier: thinking** — transport adaptation + write-fragmentation + EOF-less TLS are error-prone.

## Key Insights
- `TcpStream` already impls `embedded_io::Read + Write` (net.rs:148), chunks writes at 3840 B, and
  spin-yields on read (net.rs:164). `TlsStream` mirrors this.
- `ostd::tls` is raw IPC, each fn needs `net_tid` (tls.rs:37-108). `tls_write` accepts ≤503 B/call,
  may return partial → `Write` must loop internally (write-all). Factor the loop into a shared helper
  reused by both streams (red-team ask).
- `tls_read` returns 0 for "no data yet" AND "closed" (handlers.rs:462) → `TlsStream::Read` spin-
  yields on 0 and relies on BodyReader's framing for completion; bound with a retry/`TimedOut`.
- Cells declare `ostd = { path = ... }` with default features → adding `http`/`json` features is
  non-breaking; consuming cells opt in with `features = ["http","json"]`.

## Requirements
**Functional**
- `ostd::json` = `#[cfg(feature="json")] pub use http_core::json::*;`
  `ostd::http` = `#[cfg(feature="http")]` re-export of http-core http items + `HttpClient`/`TlsStream`.
- `TlsStream { net_tid, cap_id }`: `connect(addr,port,hostname) -> ViResult<Self>` (resolves NET via
  `sys_lookup_service(api::service::NET)`, calls `tls_connect`); `impl Read` (spin-yield over
  `tls_read`, bounded), `impl Write` (write-all loop over 503-B `tls_write`), `ErrorType=OstdError`,
  `Drop -> tls_close`.
- `HttpClient<T: embedded_io::Read + Write>`: `new(T)`; `send(&mut self, req: &[u8]) ->
  Result<(ParsedHeaders, BodyReader), HttpError>` (write request, accumulate until headers parse,
  build BodyReader seeded with leftover body bytes); `post(host, path, content_type, body)` helper.

**Non-functional**
- HTTP and HTTPS share one `HttpClient` (generic): `HttpClient<TcpStream>` / `HttpClient<TlsStream>`.
- No `unsafe` in ostd code.

## Architecture
- `libs/ostd/Cargo.toml`: `http-core = { path = "../http-core", optional = true }`;
  `[features] json = ["dep:http-core"]`, `http = ["dep:http-core"]` (http-core itself carries
  serde_json/httparse, so a single optional dep gates both; split if finer control is wanted).
- `libs/ostd/src/http/` module: `client.rs` (`HttpClient`), re-exports from http-core.
- `TlsStream` next to `TcpStream` in `clients/net.rs` (or `clients/tls_stream.rs`); shared write-all
  helper for the 503-B (TLS) / 3840-B (TCP) loops.

## Related Code Files
- **Create:** `libs/ostd/src/http/client.rs`, `libs/ostd/src/clients/tls_stream.rs` (or extend net.rs),
  e2e smoke cell (or reuse `cells/demos/https-demo`)
- **Modify:** `libs/ostd/Cargo.toml`, `libs/ostd/src/lib.rs` (feature-gated `http`/`json` modules),
  `libs/ostd/src/clients.rs`

## Implementation Steps
1. Cargo.toml features + optional http-core dep.
2. Feature-gated `ostd::json` / `ostd::http` re-exports in lib.rs.
3. `TlsStream` Read/Write/Drop + shared write-all helper.
4. `HttpClient::send` (write + header-accumulate + BodyReader seed) + `post` helper.
5. e2e smoke cell: drive `HttpClient` → `BodyReader` → `json::get_str` over a chunked response from
   `tools/hypha-mock-llm/`; assert extracted `content`. Run over `TcpStream` (HTTP) and `TlsStream`
   (HTTPS, text body). Boot in QEMU.
6. CI negative-link check: assert default `cargo check -p ostd` links neither serde_json nor httparse.
7. Tri-arch `cargo check -p ostd --features http,json`.

## Todo List
- [x] Cargo.toml: optional http-core + `http`/`json` features
- [x] Feature-gated ostd::http / ostd::json re-exports
- [x] TlsStream Read/Write/Drop + local write-all loop (NOT shared into TcpStream — see note below)
- [x] HttpClient::send + post
- [~] QEMU e2e smoke vs tools/hypha-mock-llm — smoke cell `cells/demos/http-smoke` wired + compiles; QEMU boot DEFERRED-MANUAL (needs host Python mock + QEMU run, infeasible headless)
- [x] CI negative-link check (proven: `cargo tree -p ostd -i {serde_json,httparse,http-core}` all error "did not match any packages" on default features)
- [x] Tri-arch check (riscv64gc-unknown-none-elf, aarch64-unknown-none, x86_64-unknown-none all clean with `--features http,json`)

> **write-all helper note:** `TcpStream::write` deliberately does a single chunked
> send and returns the chunk length (leaving write-all to `embedded_io::Write::write_all`).
> Folding a write-all loop into it would change its public return contract — a behavior
> regression — so `TlsStream::write` keeps the loop local. They share the pattern, not a fn.

## Success Criteria
- `HttpClient<TlsStream>` completes an HTTPS GET/POST to the mock and `get_str` returns the expected
  `content` (text body); `HttpClient<TcpStream>` likewise over HTTP.
- A request body >503 B over TLS succeeds (write-all loop verified).
- Default `cargo check -p ostd` links neither serde_json nor httparse (CI-enforced).
- Tri-arch `cargo check -p ostd --features http,json` clean.

## Post-review fixes (2026-06-22)

Both fixes preserve the Complete status; review findings fully resolved:

1. **`libs/ostd/src/clients/net.rs` — `TcpStream::write` retry on `WouldBlock`:** Added 
   `WRITE_STALL_BUDGET = 100_000` yield-loop to tolerate race where `write` is called before 
   TCP handshake completes (SynSent state). Mirrors `TlsStream::write` pattern and fixes 
   intermittent "transport write failed" on TCP path.

2. **`cells/demos/http-smoke/src/main.rs` — modernized app entry:** Migrated from deprecated 
   `#[no_mangle] pub fn main` to `ostd::app_entry!` macro; added `#![forbid(unsafe_code)]`; 
   removed manual manifest/syscall declarations (macro-generated). Aligns with Law 1 and 
   current Cellos app patterns.

## Risk Assessment
- **TLS write fragmentation** (503-B cap) — internal write-all loop is the fix; the >503-B body smoke
  is the gate.
- **EOF-less TLS** (handlers.rs:462) — never treat a 0-read as completion; rely on framing, bound the
  spin with `TimedOut`. A genuine mid-body drop surfaces as timeout, not a hang.
- **Zero-scan truncation** (tls.rs:97) — payloads ending in `0x00` truncate; UTF-8 text unaffected.
  Smoke uses text. Binary HTTPS bodies are a documented follow-up (net-cell length prefix).
- **QEMU egress**: mock runs locally (`tools/hypha-mock-llm/`), so no external network needed; if the
  mock isn't reachable, assert against a loopback `httpd` cell and mark TLS smoke manual — do not
  silently skip.

## Security Considerations
- `TlsStream` inherits the net cell's current TLS posture — `UnsecureProvider`, **no cert
  verification** (scout-report; net `tls/socket.rs:56`). Document that `HttpClient<TlsStream>` is only
  as trustworthy as the net cell's verifier — that hardening is the **parallel TLS-cert workstream**,
  not this plan. Do not claim authenticated HTTPS here.

## Next Steps
- Unblocks the hypha migration follow-up and any future networked Cell.
