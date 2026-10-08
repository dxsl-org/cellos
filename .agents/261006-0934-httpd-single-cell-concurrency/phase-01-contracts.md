---
phase: 1
title: "Freeze concurrency contracts and workload oracle"
status: pending
priority: P1
tier: thinking
dependencies: []
---
# Phase 01 — Contract, budget, acceptance

## Requirements / architecture
Freeze a narrow HTTP concurrency contract before changing shared interfaces. One reactor, explicit connection IDs with slot generations, bounded queues and absolute monotonic deadlines. Keep HTTP connection-close semantics; do not add pipelining.

## Related files
- Inspect/modify in later phases: httpd main/router/net_ipc/handlers/static_files; net socket_table/handlers/tcp/service-runtime; ostd ipc/executor; API services/ipc.
- Contract documentation: docs/network-api.md; shared prerequisite `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md`.
- Regression location: tests/integration/tests/boot.rs and http-infer.rs; AArch64 smoke in aarch64-boot.rs.

## Implementation steps
1. Inventory exported callers with LSP references before interface edits; capture wire discriminants, syscall/manifest authority and target feature gates. No semantic reuse of old empty Data/zero progress for the new typed contract.
2. Specify `Pending | Progress(n) | Eof | Error` for I/O and exact not-ready accept, owner/death/generation validation, bounded readiness batches, correlation IDs, one receive owner, retained completions, cancellation/drain rules. Phase 02 supplies mechanism, HTTP owns policy.
3. Freeze server profile: target N=256; separate global socket budget, per-owner budget, listener/accept reserve, management sockets and other clients' reserve. Check table + SocketSet + allocator + net/httpd manifest quotas together. Do not reserve every global slot for httpd.
4. Compute memory bound from RX/TX, request buffers, response fragments, pending backend state and IPC completions. Limit active static-file/backend jobs separately; stream with fixed chunk windows rather than 256×64 KiB file copies. Allocate connection storage on heap, never a huge stack array.
5. Proposed deadlines to measure/freeze: headers 5 s absolute, body 10 s absolute, TX no-progress 10 s, backend 30 s, cleanup 5 s after transport cancellation acknowledgement. Data trickle must not refresh absolute header/body deadlines. CPU work per ready connection is bounded by bytes/steps each turn.
6. Define proposed mixed-load fast-route deadline D=max(250 ms, 3×isolated p99) on the named qualification machine, with slow connection intentionally held longer than 5D. If unattainable, report failure and revise the approved target explicitly; no automatic weakening.
7. Add behavioral regression oracles during implementation: incomplete Content-Length 100/body 1 stays pending, no AI side effect; concurrent status completes before remaining 99 bytes are released. Do not use server prematurely responding to slow client as proof of concurrency.
8. Obtain design approval for exact libs/api/libs/types delta (Law 1 first checkpoint). Record proposed IDs/layout without editing the ABI before approval.

## Success criteria
- [ ] Named profile, memory inequality, deadlines, overload behavior and full route workload are recorded and mechanically testable.
- [ ] Transport/backend timeout vs cancellation vs side-effect uncertainty is explicit.
- [ ] Design approval recorded; blocked approval does not block source research or host-only model work.

## Assumptions
256 fits target memory and selected backend mix: unverified, measure in Phase 05. Deadline values are proposed policy, not current observed performance. Existing artifacts may differ from working tree: rebuild exact implementation before comparisons.

## Security
Reject conflicting Content-Length, invalid/overflow length and Transfer-Encoding framing not implemented; reject TE+CL; distinguish too many headers (431), total request too large (413), malformed framing (400), timeout (408). Reject unsupported transfer coding explicitly (501), then close; never dispatch a truncated request. Keep byte budget before append.

## Risk assessment / rollback
Design-only phase is reversible. Published/approved ABI IDs and external acceptance agreements cannot silently be reassigned. Revert contract draft before code, not by pretending existing async support exists.

## Deviation log
None.
