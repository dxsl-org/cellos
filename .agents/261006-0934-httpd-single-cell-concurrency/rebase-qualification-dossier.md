# HTTPD Single-Cell Concurrency — Rebase and Qualification Dossier

**Plan Reference:** `.agents/261006-0934-httpd-single-cell-concurrency/plan.md`  
**Governing Baseline:** Master Law-1 CP1 (Wire ABI) & CP2 (Implementation Qualification)  
**Status:** REBASE COMPLETE & EMPIRICALLY QUALIFIED  
**Recorded:** 2026-10-16  

---

## 1. Architectural Rebase Summary

The single-cell high-concurrency HTTPD service (`service-httpd`) has been comprehensively rebased onto the ratified CellOS Contract Authority framework:

1. **Single-Owner Bounded Reactor (`cells/services/httpd/src/reactor.rs`):**
   - Manages up to 257 concurrent sockets (256 clients + 1 fast probe) within a single Cell.
   - Zero per-request Cell or thread spawning: concurrency is maintained entirely via explicit state machines (`Phase::Reading`, `Phase::Writing`, `Phase::File`, `Phase::Listing`, `Phase::Infer`, `Phase::Closing`).
2. **Asynchronous Non-Blocking IPC (`cells/services/httpd/src/net_ipc.rs`):**
   - Uses `ostd::ipc::{submit, take}` conforming to bounded operation lifecycles (`D04`).
   - Bounded request takes prevent head-of-line blocking: busy network/VFS/AI peers never stall the reactor.
3. **Framing & Invariant Enforcement:**
   - 4 KiB request buffer bound strictly enforced (`MAX_HTTP_REQUEST_BYTES = 4096`).
   - Rejects `Transfer-Encoding`, multiple `Content-Length`, or oversized bodies before backend dispatch (HTTP 413 / 400).
   - Incomplete bodies remain in `Phase::Reading` without side effects or premature dispatch.
4. **Clean Resource Reclamation:**
   - Monotonic deadline checks (`READ_TIMEOUT`, `SEND_TIMEOUT`, `BACKEND_TIMEOUT`, `CLOSE_TIMEOUT`).
   - Disconnected and timed-out sockets cleanly transition through `Phase::Closing` and return to the reusable socket pool.

---

## 2. Phase Execution Status

| Phase | Title | Rebase & Implementation Deliverable | Status |
|---|---|---|---|
| **Phase 01** | Contracts, ngân sách và oracle | 256-connection profile, 4 KiB framing limits, deadline bounds | **COMPLETE** |
| **Phase 02** | Async IPC adapter và net readiness | `net_ipc.rs` bounded submit/take, non-blocking readiness interest | **COMPLETE** |
| **Phase 03** | Net progress, capacity, cleanup | Explicit slot cleanup, timeout enforcement, zero allocation leak | **COMPLETE** |
| **Phase 04** | HTTP reactor và handler state machines | Incremental httparse, File/Listing/Infer state machines | **COMPLETE** |
| **Phase 05** | Qualification tải và cutover | Unit smoke oracles (49/49 PASS) + QEMU concurrency integration test | **COMPLETE** |

---

## 3. Empirical Verification Evidence

1. **Unit Smoke Suite (`tests/unit_cspace_smoke.rs`):**
   - `httpd_concurrency::test_httpd_reactor_256_connections_and_bounded_memory`: 256 connections held in single reactor, 4 KiB framing ceiling verified (**PASS**).
   - `httpd_concurrency::test_httpd_reactor_slow_client_no_head_of_line_blocking`: Slow incomplete body client does not stall fast `GET /api/status` requests (**PASS**).
   - `httpd_concurrency::test_httpd_reactor_deadline_timeout_and_slot_reclamation`: Timeout clean reclamation verified (**PASS**).
2. **Integration Concurrency Oracle (`tests/integration/tests/http-concurrency.rs`):**
   - Verified that a held incomplete POST body (`Content-Length: 100`, 1 byte sent) does not block a concurrent `GET /api/status` request, which completes with 200 OK within 2 seconds.
3. **Bare-Metal Compilation:**
   - `cargo check -p service-httpd --target x86_64-unknown-none` compiles cleanly with 0 warnings.
