---
title: "Phase D — IPC Buffer Length Fix + Lua TCP Bindings"
description: "Fix net cell stale-byte IPC bug, then expose vnet.* TCP API to Lua."
status: pending
priority: P1
effort: 4h
branch: main
tags: [networking, lua, ipc, net-service, riscv64]
created: 2026-06-03
---

# Phase D — IPC Buffer Length Fix + Lua TCP Bindings

## Goal
Two independent, sequenced fixes:
1. **D.1 (HIGH bug):** Net cell appends up to 503 stale bytes to TCP SEND payloads because the receive buffer is reused without length tracking. Pre-zero + zero-scan to recover the true message length.
2. **D.2 (feature):** Expose a `vnet.*` TCP socket API inside the Lua cell, mirroring the verified `nc.rs` IPC pattern, so Lua scripts can do outbound HTTP.

## 3-Task Rule
**SKIPPED** — this plan has exactly 2 phases. The phases are genuinely independent in scope (one Rust net-cell fix, one Lua-cell feature) and combining them would violate KISS. No artificial third phase.

## Phases

| # | Phase | Status | Effort | Blockers |
|---|-------|--------|--------|----------|
| 01 | [IPC Buffer Length Fix](phase-01-ipc-buffer-length-fix.md) | pending | 1.5h | none |
| 02 | [Lua TCP Bindings](phase-02-lua-tcp-bindings.md) | pending | 2.5h | Phase 01 (correctness; see note) |

**Dependency note:** Phase 02 functionally depends on Phase 01. The Lua `vnet.send` path
hits the same net-cell SEND handler. If D.1 is not fixed first, Lua-sent HTTP requests carry
trailing garbage and the host server may reject or mis-parse them. Land 01 before validating 02.

## File Ownership (no overlap)
- **Phase 01:** `cells/services/net/src/main.rs`
- **Phase 02:** `cells/runtimes/lua/src/ffi.rs`, `cells/runtimes/lua/src/bindings_net.rs` (NEW), `cells/runtimes/lua/src/main.rs`, `tests/integration/tests/boot.rs`

No file is touched by both phases — safe to parallelize editing, but validate sequentially.

## Success Criteria (whole plan)
- `cargo build --release` clean for both `net` and `lua` cells.
- `cargo clippy -- -D warnings` clean (net cell has no wildcard match arms — keep it that way).
- Integration test `network_curl_http_get` still passes (D.1 regression guard).
- New integration test `lua_tcp_http_get` passes: Lua connects out, GETs, prints body containing "HELLO".

## Key Verified Facts (re-grepped 2026-06-03)
- `poll_driver.rs:58` `decode_message(buf: &[u8])` **already takes a slice** — no change needed there. Only `main.rs` changes in D.1.
- `main.rs:84` `let mut buf = [0u8; 512]` is the single reused receive buffer for BOTH kernel RxFrame and cell-request messages.
- `main.rs:285` `let data = payload;` → `socket.send_slice(data)` `:289` is the exact bug site.
- `lua/src/main.rs:25` `luaL_openlibs(L)` — register `vnet` immediately after.
- `ffi.rs` already has `lua_tointegerx`, `lua_tolstring`, `lua_pushlstring`, `lua_pushstring`, `lua_pushinteger`, `lua_pushnil`. Missing 4 FFI decls (D.2 adds them).
- `nc.rs` is the canonical TCP-client IPC pattern — D.2 mirrors it (opcodes, LE cap layout, retry loops).
- Test template: `network_curl_http_get` at `boot.rs:270`.

## Unresolved Questions
See per-phase files. None blocking.
