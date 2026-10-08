---
phase: 4
title: "Single-cell HTTP reactor and incremental handlers"
status: pending
priority: P1
tier: thinking
dependencies: [2, 3]
---
# Phase 04 — HTTP scheduling and routes

## Requirements / architecture
Connection lifecycle: Accept → ReadHeaders → ReadBody → Dispatch/BackendPending → WriteHeaders/WriteBody → DrainClose → Closed. Error/timeout can enter bounded error response or abort. Reactor owns buffers/socket handles; per-turn byte and work budgets enforce round-robin fairness. No request cells/threads.

## Related files
cells/services/httpd/src/{main.rs,net_ipc.rs,router.rs,handlers.rs,static_files.rs,net_ipc_tests.rs}; new connection/reactor module(s) if existing files cannot remain responsibility-bounded; libs/ai-sdk adapters and VFS adapter only as required for nonblocking exchanges; httpd syscall/manifest declaration.

## Implementation steps
1. Replace outer sequential loop and recv_request/send_all loops with bounded connection storage and ready queue. Keep immutable route handlers separate from network transmission; build Response state rather than synchronously send from handler.
2. Incrementally parse with httparse, distinguish Partial/Complete/Error and validated body length. Dispatch exactly once after all declared bytes; unsupported framing is explicit error. Enforce byte limit before append, header count, deadline, EOF/reset and no side effect on incomplete requests. With connection-close semantics, never treat trailing pipelined bytes as another request.
3. Send headers and body fragments with cursors; do not concatenate/copy entire file/response. Handle partial writes, zero progress, writable rearm, and TX stall deadline. Centralize terminal cleanup with explicit asynchronous close tracking; Drop must not secretly block.
4. Preserve all routes: index/status/cells are bounded local work; files/listing become incremental backend operations, preserving live VFS reads and 64 KiB file cap; AI uses submit/poll/cancel session states instead of generate-to-completion. Use Phase 02 adapter for EVERY exchange, including metadata/describe/stat/list/read/cancel.
5. Bound active expensive backend sessions separately from open client count; refusal returns 503 without occupying an unbounded queue. Each AI poll may perform CPU work in AI service; reactor continues unrelated work while reply is pending. Disconnect stops new polls and requests cancellation, retaining state until acknowledged.
6. Static file reads retain bounded chunk windows and release file handles on success/error/disconnect. Streaming failure after headers means abort, not emitting a second HTTP status; premature EOF before declared Content-Length is an error. Do not silently change missing-file/empty-file semantics.
7. Replace iteration-based retries and fixed yield delay with elapsed deadlines/readiness. Sleep only with no ready work, until earliest deadline or completion. A continuously readable client cannot monopolize the loop.
8. Remove obsolete synchronous httpd helpers after all callers migrate. Update behavioral tests; remove source-text/wiring/incidental-default assertions rather than repinning them. Keep CLI and response contract coverage.

## Success criteria
- [ ] Held incomplete POST receives no response and triggers no backend dispatch while unrelated GET completes before POST release; completion after release is correct exactly once.
- [ ] Slow reader, fragmented headers/body, malformed framing, reset, backend busy/death and timeout do not stall other routes.
- [ ] All existing endpoints/file-serving CLI work through the new path; no retained sync handler route or hidden worker cell.
- [ ] Idle sleeps, memory stays within budget and every request's response bytes remain isolated to its connection.

## Assumptions
Current AI/VFS service capacities are below or independent of 256; qualify limits rather than increasing them implicitly. Arbitrary future CPU-heavy user handlers must be incremental/offloaded to existing service architecture; async syntax cannot preempt arbitrary blocking code.

## Security
Requests in one cell share heap/capabilities/failure domain: this is concurrency, NOT request isolation. Enforce lengths/deadlines and avoid references surviving buffer reuse. Route generation prevents stale replies accessing a new connection slot.

## Risk assessment / rollback
Run new path on development images until qualification. Revert httpd/net paired deployment after draining; no permanent legacy/sync httpd fallback. Already dispatched backend effects cannot be undone; cancellation is not rollback.

## Deviation log
None.
