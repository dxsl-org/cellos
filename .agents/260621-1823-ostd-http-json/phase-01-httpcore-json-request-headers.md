---
phase: 01
title: http-core — json + request builder + header parsing
tier: medium
depends_on: []
status: Complete
---

# Phase 01 — `libs/http-core`: JSON + Request + Header Parsing

## Context Links
- [plan.md](plan.md) · [scout-report.md](scout-report.md)
- Generalizes hypha `build_post` (`llm-gateway/src/http.rs:22-31`), `json_escape` (L34-48),
  `extract_content` (L60-102), `http_body` (L51-55).

## Overview
- **Priority:** High (foundation; everything else builds on this crate).
- **Status:** Complete.
- Create the host-testable crate `libs/http-core` and land the non-tricky pieces: JSON via
  `serde_json` (alloc), an HTTP/1.1 `RequestBuilder`, and `parse_response_headers` via `httparse`.

## Key Insights
- New crate uses `#![cfg_attr(not(test), no_std)]` + `extern crate alloc`, **defines no allocator /
  panic_handler** → `cargo test -p http-core` runs on the host (the fix for ostd's untestability).
- `serde_json` + `httparse` both already build no_std+alloc in this workspace (`httpd` Cargo.toml:14,16).
- `httparse::Response::parse()` → `Status::Partial` when incomplete; body begins at offset `n`.
- embedded-io 0.7 API verified: `Read::read -> Result<usize,E>`, `Write::write/write_all`,
  `ErrorKind::TimedOut`. `BodyReader` (Phase 02) is generic over `embedded_io::Read`, so http-core
  takes a direct `embedded-io = "0.7"` dep (a mock Read drives host tests).

## Requirements
**Functional**
- JSON: re-export `serde_json::{Value, json!, from_slice, to_string, to_vec, Error}`; add
  `get_str(&Value, path: &[&str]) -> Option<&str>` (nested + duplicate-key safe).
- `RequestBuilder`: method, path, host, ordered headers `&[(&str,&str)]`, optional body →
  `Vec<u8>`. Auto-set `Host`, `Content-Length` (when body present), `Connection: close`.
- `parse_response_headers(&[u8]) -> Result<ParsedHeaders, HttpError>` where `ParsedHeaders` =
  `{ status: u16, content_length: Option<usize>, framing: Framing, body_offset: usize }`,
  `Framing = ContentLength | Chunked` (**no UntilClose** — decision 4).
- `HttpError` enum: `NeedMoreData, MalformedStatus, MalformedHeader, TooManyHeaders, BadChunk, …`.

**Non-functional**
- No `unsafe` in http-core's own code. Bounded header count (fixed `[Header; N]`, document N).

## Architecture
- New crate `libs/http-core/` (add to workspace `members`). Files:
  `lib.rs` (crate attrs, re-exports, `HttpError`, `Framing`), `json.rs` (`get_str` + re-exports),
  `request.rs` (`RequestBuilder`), `response.rs` (`parse_response_headers`, `ParsedHeaders`).
- Deps: `serde_json {default-features=false, features=["alloc"]}`, `serde {default-features=false}`,
  `httparse {default-features=false}`, `embedded-io = "0.7"`.

## Related Code Files
- **Create:** `libs/http-core/Cargo.toml`, `src/lib.rs`, `src/json.rs`, `src/request.rs`, `src/response.rs`
- **Modify:** workspace `Cargo.toml` (`members += "libs/http-core"`)

## Implementation Steps
1. Scaffold crate with `#![cfg_attr(not(test), no_std)]`, `extern crate alloc`, deps above.
2. `json.rs`: re-exports + `get_str` path helper.
3. `request.rs`: `RequestBuilder` → request-line/Host/headers/Content-Length/CRLF/body into `Vec<u8>`.
4. `response.rs`: wrap `httparse::Response::parse`; map `Partial`→`NeedMoreData`; derive `Framing`
   (chunked if `Transfer-Encoding: chunked`, else ContentLength); compute `body_offset`.
5. `#[cfg(test)]` host tests (matrix below).
6. `cargo test -p http-core` (host) green; `cargo check -p http-core --target riscv64gc-unknown-none-elf`.

## Todo List
- [x] Crate scaffold + workspace member
- [x] json.rs (re-exports + get_str)
- [x] RequestBuilder → Vec<u8>
- [x] parse_response_headers + ParsedHeaders + Framing + HttpError
- [x] Host unit tests
- [x] Host test green + tri-arch `cargo check`

## Success Criteria
- `cargo test -p http-core` runs on host and passes.
- Request bytes for a known (method,path,host,body) match RFC 9112 layout exactly.
- Framing resolves to Chunked for `Transfer-Encoding: chunked`, else ContentLength.
- `get_str` finds a nested key, returns `None` on miss, deterministic on duplicate keys (document which).
- `cargo check -p http-core` clean for the bare-metal target (no_std path compiles).

## Risk Assessment
- **Host vs no_std drift:** a host-only item could compile under test but not on bare metal → the
  tri-arch `cargo check` step is the gate.
- **httparse SIMD** is x86-only; scalar fallback on RISC-V/ARM64 — correctness unaffected.

## Security Considerations
- `from_slice` returns `Result` — never `.unwrap()` on untrusted JSON.
- Bound header count + total header bytes against a hostile server (document the cap).

## Next Steps
- Phase 02 adds `BodyReader` to this crate, consuming `ParsedHeaders.framing` + `body_offset`.
