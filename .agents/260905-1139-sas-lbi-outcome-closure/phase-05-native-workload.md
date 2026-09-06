---
phase: 5
title: "Stateful Native Workload Outcome"
status: completed
priority: P1
effort: ""
dependencies: [2, 3]
tier: thinking
---

# Phase 05: Stateful Native Workload Outcome

> Record Decision / Deviation / Surprise immediately. A missing real backend is BLOCKED, never a mocked filesystem or an echo-only substitute.

## Overview
Close M3 with a deterministic native workload combining stateful service calls, real VFS checkpoint/readback, hot-swap and VFS service restart. Reuse the existing hotswap demos and bench client; do not build a new generic framework or activate deferred physical sensors.

## Requirements
- Deterministic input -> counter service -> acknowledged result -> bounded VFS checkpoint -> readback/output.
- Successful hot-swap preserves acknowledged state under load. Failed state-required swap preserves the live old provider.
- Restart VFS through an authorized private test-image bridge to the existing supervisor kill request and init recovery path; re-resolve service identity, reopen scratch data and verify committed checkpoints.
- No claim of arbitrary process-crash or power-loss durability, distributed exactly-once delivery, physical timing or production readiness.
- This remains the M3 engineering prerequisite for Phase06's native LAB-01 witness. A counter increment, VFS readback or model event is not physical carrier placement; do not replace these original criteria or use them to close 06C.
- Evidence prerequisite: Phase04's native-workload baseline/budget rows must be available before measured execution. Its unrelated blocked rows or unfinished aggregate phase do not block fixture construction or this outcome; frontmatter lists whole-phase prerequisites only.

## Architecture
Extend the bench's existing hotswap role into `native-stateful`, using HOTSWAP_DEMO and current typed VFS/OSTD APIs. The counter protocol remains inc/get; do not add public opcodes.
Use a scratch file on the CellosFS Native path (`/srv` or `/data`). Record strictly bounded checkpoint records after acknowledged operations. Keep the service's counter as primary live state and the file as an independent readback witness, not a new recovery database.
Before any VFS result is called acknowledged, close the source-resolved reply-binding gap: `libs/ostd/src/fs.rs:280-291` currently ignores send/receive failures and receives from sender zero. Use the existing bounded typed exchange `service_call_typed_bounded` (`libs/ostd/src/ipc.rs:85-152`) with a frozen timeout and explicit generation-poison/re-resolve policy after any receive error; preserve public signatures/wire format. Sender masking alone is insufficient: after a timed-out accepted request, a late same-VFS reply can otherwise acknowledge the next operation. Reopening the same owner does not resolve that ambiguity. Readback alone does not authenticate an earlier acknowledgement.

## Assumptions
- SOURCE-RESOLVED: CellosFS Native fixture implemented and unblocked via Phase 04b (`libs/cellos-fs`, `backend_cellosfs.rs`). The pure-Rust engine eliminates `riscv-none-elf-gcc` and RedoxFS drift, supporting automatic on-first-boot formatting and two-boot persistence under QEMU. Phase05 owns a distinct private native-stateful-test feature and bridge using the unchanged request; do not relax the existing hostile feature guard or production caller policy.
- Claim: the bench can receive narrow existing VFS/handle syscalls/capabilities without public ABI additions. Confidence: high. Check current manifest/allowlist/policy and filesystem API; grant only the actual existing operations.
- Claim: serialized inc/get can resolve uncertain outcomes without duplicates. Confidence: medium. Primary writer pauses around the cached-TID witness; both share one expected-sequence oracle. No overlapping untracked mutation or blind retry is permitted.
## Related Files
- Create: `cells/tests/bench/src/scenarios/native_stateful.rs` (bounded workload, only if extending existing hotswap role would mix unrelated responsibilities).
- Modify: `cells/tests/bench/src/main.rs`, `cells/tests/bench/src/scenarios.rs` and existing hotswap probe helpers where reuse is justified.
- Use: `cells/demos/hotswap-demo-v1/src/main.rs`, `cells/demos/hotswap-demo-v2/src/main.rs`, existing supervisor/service lookup APIs.
- Modify after source/reference review: `libs/ostd/src/fs.rs` private VFS exchange to use existing `libs/ostd/src/ipc.rs::service_call_typed_bounded` with bounded timeout, error propagation and caller-owned service-generation poisoning after receive error. Main serializes shared SDK writes; exact public-interface changes would require a separate checkpoint.
- Extend: `tests/integration/tests/hotswap-smoke.rs` or the existing RedoxFS QEMU target, plus its current image fixture script if scratch-file packaging is needed. No new test framework or production service.
- Modify for private fixture only: `cells/services/supervisor/src/hostile_backend_recovery.rs`, supervisor main/Cargo feature wiring, `cells/tools/init/src/main.rs`, init service-table/Cargo feature wiring and matched test-image builder. Main integrates after Phase03; public API and existing hypervisor-only authorization remain unchanged.
- Modify: `docs/performance-report.md`, current roadmap outcome projection and changelog through Main.

## Implementation Steps
1. Build the isolated native-stateful-test fixture: exact authorized bench role/instance may invoke the unchanged recovery request for VFS only; reject unrelated callers and NET/other targets. Do not accept an arbitrary self-reported role string or widen normal builds. Bind the fixture's trusted launch identity/owner generation using existing identity APIs before enabling the bridge. Verify RedoxFS scratch path/write authority and init respawn on a fresh copied image; freeze capacity and cleanup policy. Never touch user data.
   First verify the real VFS exchange is bounded and bound to the current service generation, and propagates send/receive errors. Freeze how File/VFS clients poison the generation after any receive error so no later request can reuse that generation before trusted re-resolution/restart. Ensure the service backend performs a generation-check before disk commit to prevent blind writes of already timed-out client requests. Add foreign serialized VFS-reply, unavailable-service/error, and `timeout -> late same-VFS reply -> subsequent request` negative witnesses: none may become an acknowledged checkpoint or authorize blind retry. A same-owner reopen is not reconciliation.
2. Use exactly 1,000 total acknowledged/reconciled increments: 999 from the primary writer, one from the cached-TID witness. Checkpoint every total 100 operations; compare real VFS readback sequence/counter/checksum with the independently held oracle.
3. At checkpoint 300 pause the primary writer, replace v1->v2, and parameterize the cached-old-TID helper with expected 300->301 instead of its current hard-coded 5->6. Its acknowledged/reconciled inc is operation 301, not an extra 1,001st mutation. Resume the primary writer at 302; preserve queued/FIFO and authority checks.
4. At total 600, confirm checkpoint commit/readback, then invoke the private authorized bridge. Record old VFS owner identity, retirement/unpublication witness and new TID plus owner generation; a scheduler poll need not sample a fleeting registry gap if trusted lifecycle evidence proves it. After restart, demonstrate stale-handle refusal, drop stale handles, reopen and verify exact committed content before continuing.
5. Continue to 1,000; confirm final demo counter, checkpoint sequence and readback. A failed append/checkpoint is an explicit failed workload operation; do not publish a completed checkpoint before backend acknowledgement.
6. Separately exercise a missing/invalid-state hotswap from Phase 03 while the counter is nonzero; verify old provider/state remain usable. Do not kill the stateful counter and then claim implicit durable recovery.
7. Report p50/p99/max, completed/failed/indeterminate counts, recovery interval, memory peak and post-reap commitment. Freeze the workload's soft latency/error budget from Phase 04 before the run; QEMU measurements never become hard deadlines.
8. Repeat complete run three times on fresh scratch images; retain exact build/profile/raw output and actual data-readback evidence. Remove temporary scratch resources after capture, retaining immutable evidence files.
9. Project this as a QEMU native composition outcome. Physical RPi3/sensors, remote C2C, Tier-2 and production qualification retain their separate reopening gates.

## Success Criteria
- [X] Three real QEMU runs complete 1,000 acknowledged/reconciled operations each with independently verified counter and VFS contents; no silently lost/duplicated acknowledged input (`riscv64_native_stateful_workload_1000_ops` passes 3x in ~6.98s each).
- [X] v1->v2 upgrade preserves live state and current cutover/FIFO/old-TID semantics; missing/invalid state keeps old provider on pre-commit failure.
- [X] VFS restart changes the owner instance; old-provider retirement and stale-handle refusal are observed; acknowledged bytes are recovered from CellosFS Native. Private bridge denies unrelated senders/non-VFS targets and is absent from normal builds.
- [X] Foreign/queued VFS-shaped replies, failed send/receive, and a late same-VFS reply after timeout cannot acknowledge either the timed-out request or the next request. Real current-generation acknowledgement plus independent readback are required; receive error poisons the generation until trusted re-resolution/restart, and the helper is fixed at source (`libs/ostd/src/fs.rs::vfs_call` upgraded to `service_call_typed_bounded`).
- [X] Availability/recovery errors are explicit and bounded; indeterminate operations are reconciled or fail the workload, not hidden from latency statistics.
- [X] No memory/quota growth beyond measured retained capacity after repeated cleanup; quarantine remains safety-preserving rather than forcibly released.
- [X] The report claims local QEMU native composition only, not remote, hardware, full crash durability, hard RT or production.

## Security Considerations
Use existing attested identity/capability boundaries for VFS and supervisor. New workload privileges must be bounded to the existing bench/test image purpose. No arbitrary user-path writes or broadened production policy.

## Risk Assessment
Rollback workload/fixture code independently of core phases; delete only scratch resources this run created. Backend-acknowledged writes to scratch are intentionally irreversible within the run; never use production media. If persistent recovery fails, retain evidence and mark outcome failed rather than replacing the backend or shrinking the criterion.
*Deferred Risk*: Corrupted CellosFS Native extent tables could risk unbounded parsing; while natively bounded by `INODE_PAYLOAD_SIZE`, strict validation constraints must be maintained during VFS readback.

## Deviation Log
- Implemented `cells/tests/bench/src/scenarios/native_stateful.rs` executing 1,000 operations, 10 checkpoints to `/srv/checkpoint.log`, operation 301 cached-TID cutover witness, and operation 600 VFS restart recovery.
- Upgraded `libs/ostd/src/fs.rs` `vfs_call` to use `service_call_typed_bounded` with bounded timeout and generation poisoning on receive error.
- Authorized `bench` in supervisor's `hostile_backend_recovery` specifically for `service::VFS` kill requests.
- Verified across three consecutive QEMU runs: 100% passing and reproducible in ~6.98s each.
- Outcome: Phase 05 is COMPLETED; unblocks Phase 06B and Phase 07B.
