---
phase: 02
title: http-core — BodyReader (content-length + chunked decoder)
tier: thinking
depends_on: [01]
status: Complete
---

# Phase 02 — `http-core`: BodyReader

## Context Links
- [plan.md](plan.md) · [scout-report.md](scout-report.md) · [phase-01](phase-01-httpcore-json-request-headers.md)

## Overview
- **Priority:** High — the correctness-critical core. Isolated as its own phase (the red-team's
  "sacred" review boundary) precisely because chunked decoding has subtle split-boundary bugs.
- **Status:** Planned.
- An incremental body reader decoding **Content-Length** and **chunked** framing over an
  `embedded_io::Read`, returning bytes without buffering the (possibly unbounded) body. **No
  UntilClose** (decision 4 — no EOF signal over TLS).

## Key Insights
- Streaming LLM responses are unbounded → poll-style `read(&mut self, t, out) -> Result<usize>`;
  `Ok(0)` = body complete (per framing, **not** per transport EOF).
- `httparse::parse_chunk_size(&buf[pos..]) -> (consumed, chunk_len: u64)` parses the hex size line +
  CRLF and skips chunk extensions — use it, don't hand-parse hex.
- Three classic chunked bugs to defend (research finding 5): (1) not consuming trailing CRLF after
  chunk data; (2) chunk-size line split across two reads (need a small line accumulator); (3) u64→
  usize overflow on chunk size.
- Body completion is framing-driven: ContentLength counts to N; chunked ends at `0\r\n` (then drain
  trailer to final CRLF). A 0-byte transport read mid-body means "not ready yet" → spin-yield, never
  treat as EOF. This is what lets the decoder work over the EOF-less TLS transport.

## Requirements
**Functional**
- `BodyReader` built from `Framing` + the leftover post-header bytes already received during header
  parsing (critical: the first chunk-size line often arrives in the same packet as the headers).
- State:
  - `ContentLength { remaining: usize }` → `min(remaining, out.len())`, then `Ok(0)`.
  - `Chunked { chunk_remaining: u64, line_buf: [u8; 32], line_len: usize, done: bool }` → drives the
    size→data→CRLF state machine; handles `parse_chunk_size` `Partial` by buffering the partial line;
    terminates on `0\r\n`; drains (discards) any trailer up to the final CRLF.
- `fn read<R: embedded_io::Read>(&mut self, r: &mut R, out: &mut [u8]) -> Result<usize, HttpError>`.

**Non-functional**
- No whole-body allocation; bounded internal state (≤32 B line buffer + leftover cursor).
- No `unsafe`.

## Architecture
- Add `libs/http-core/src/body.rs`; re-export `BodyReader` from `lib.rs`.
- Leftover seeding: `BodyReader::new(framing, leftover: &[u8])` consumes `leftover` before issuing
  transport reads.

## Related Code Files
- **Create:** `libs/http-core/src/body.rs`
- **Modify:** `libs/http-core/src/lib.rs` (re-export)

## Implementation Steps
1. `BodyReader` + state enum + leftover seeding.
2. ContentLength path (leftover first, then transport, counted).
3. Chunked state machine: size (line accumulator for Partial) → data → CRLF → repeat → `0\r\n`
   terminator → trailer drain.
4. Bounded spin-yield contract: a 0-read while body incomplete = retry; document the
   caller/transport timeout that prevents an unbounded loop.
5. Host unit tests with a **mock `embedded_io::Read`** yielding bytes in adversarial fragmentations.

## Todo List
- [x] BodyReader + state enum + leftover seeding
- [x] ContentLength read path
- [x] Chunked decoder (size/data/CRLF/terminator/trailer)
- [x] Mock-Read host tests: fragmented size line, multi-chunk, trailer present, exact-boundary, leftover-only
- [x] `cargo test -p http-core` green (45 pass: 22 from P01 + 23 new)

## Success Criteria
- Chunked body reassembles correctly when the mock splits bytes at every adversarial boundary
  (mid-size-line, mid-data, between data and CRLF, header/first-chunk in one packet).
- Content-Length returns exactly N bytes then `Ok(0)`.
- `0\r\n\r\n` (with and without a trailer section) ends the stream cleanly.
- Malformed chunk size → `HttpError::BadChunk`, no panic, no overflow.

## Risk Assessment
- **Split-boundary** is the dominant bug class → 32-B `line_buf` + explicit Partial handling; the
  adversarial-fragmentation test is the gate (now actually runnable thanks to Phase 01's crate split).
- **Trailer fields** rare but drain them or risk mis-framing (cheap insurance).

## Security Considerations
- Guard the u64→usize chunk-size cast; reject sizes above a sane ceiling.
- Never an unbounded read loop: completion is framing-bounded; transport stall is timeout-bounded.

## Next Steps
- Phase 03 wires `BodyReader` into `HttpClient` over `TcpStream` and `TlsStream`.
