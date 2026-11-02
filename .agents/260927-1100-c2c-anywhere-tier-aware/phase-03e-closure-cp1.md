# C2C Phase 03 slice e CP1: closure of retained-reply lifetime, dispatched cancellation and admission fairness

Status: **draft for the CELL-CONTRACT-0024 lock handoff (section 7). Documentation only.** This file edits no source, lock, plan, spec or changelog, and claims nothing is implemented. Written 2026-11-02 against HEAD `5638153118921e53c510571ba5dc50c26d73b0a7` (slice d committed in `9fd97ae8a`, `563815311`); `git status --short` shows only `?? .agents/archive/`. Inputs (SHA-256, first 16 hex): `phase-03-async-ipc.md` `691e0dda9967aa81`, `plan.md` `3c09006649e0fcbd`, `cell-contract-0024-locks.md` `ba004baa4fb5a9a2`, `plan-portfolio.md` `f66fdbc57e4fe27f`, Spec 20 `4dc8b4cd904bcd9e`, Spec 17 `916f4cb1db491086`, ADR-0023 `138bed58e53bb305`, ADR-0024 `275b068c1f7672a7`, 03d CP1 `f74b9a0f652723a1`, 03d CP2 approval `daf825a5925aec36`. Digest form: SHA-256 of the whole file, no normalization, never stored in the file it hashes.

Claim hygiene: every `file:line` was read in this session. `[INFERENCE]` = not verified by running or by opening the exact code. `OPEN (E-n)` = owner decision (section 12). No build, test or QEMU run was made for this packet.

## 1. Scope

**In:** the three obligations Phase 03 still lists as open outside slice d (`phase-03-async-ipc.md:376`): (a) retained-reply lifetime, (b) cancellation of a dispatched operation, (c) fairness under sustained admission competition and a full peer queue; plus one defect found while auditing (b): receive commits by queue position (G1). **ABI: none** (section 6). **Out:** section 11. **Goal:** on CP2, every Phase 03 success criterion (`phase-03-async-ipc.md:39-42`) maps to evidence (section 10) and the phase can be marked done at the stated x86 QEMU ceiling (E-7).

## 2. Remaining-gap inventory

What is **already proven** (not re-done): token identity and refusal of late/duplicate/foreign-generation replies (`async_ipc.rs:447-475`, tokens never reused `:264-270`); terminal only from `Dispatched`; take removes (`:714-719`). Host tests: `cancelled_and_consumed_late_reply_cannot_complete_successor` (`:1397`), `queued_cancel_removes_only_exact_request_and_is_terminal` (`:1577`), `stale_and_late_replies_never_satisfy_another_operation_or_generation` (`:2226`), two-hart models (`:1842`, `:2284`). Lanes: `CANCEL-LATE-REPLY`, `DEADLINE`, `RESTART`, `MSW-STALE`, `MULTI-CALLER`, `PEER-PRESSURE` (`local-service-lifecycle-x86.rs:125-138,371-379`).

| # | Gap | Obligation | Evidence (read this session) |
|---|---|---|---|
| G1 | **Receive commit by position can deliver an operation twice.** `Recv`, `RecvScatter` and `RecvTimeout` snapshot the first mask match and call `dispatch` in one `SCHEDULER` hold, copy out unlocked, then remove `pending_msgs[pos]` if its `sender_tid` matches. With a sender mask, an entry *ahead of* `pos` (another sender's queued op) can be removed between the holds by `cancel`/`expire`/`owner_died`; `pos` then names the delivered sender's *next* message, which is removed instead. The delivered wire stays queued and is delivered again (double execution); the removed op stays `Queued` and later ends `PreDispatchTimeout`/`Cancelled` without delivery (honest). `TryRecv` and `task.rs` `ipc_recv*` already commit by `delivery_id`. Trigger needs a concurrent remover between the holds: another hart, or a tick during copy-out `[INFERENCE: whether IRQs are enabled during x86 syscall copy-out]`. | b, criterion 2-3 | `syscall.rs:5299-5301` (dispatch in snapshot hold), `:5452-5456`, `:5609-5613`, `:5815-5819`; contrast `:5949-5957`, `task.rs:2747-2758,2876-2887`; removers `async_ipc.rs:633-639,781-791,856-873,948-957` |
| G2 | Cancellation definiteness is correct by construction but only partly tested. `Cancelled` is produced only from `Queued`, in the same hold that removes the wire (`async_ipc.rs:629-657`); `dispatch` binds at snapshot (`:416-445`; `syscall.rs:3057-3074,5590-5592,5684-5686,5914-5916`). Untested producers: `on_owner_exits` (`:760-808`) and `cutover_peer` (`:879-911`, only caller `hotswap.rs:439`); no test interleaves cancel with the receive boundary. | b | grep: no test calls `cutover_peer`/`on_owner_exits` |
| G3 | The **synchronous** caller-timeout path is never witnessed: bounded `exchange` cancels then takes on budget expiry (`libs/ostd/src/ipc.rs:476-496`); unbounded `service_call`/`service_call_typed` rely on the 3000-tick kernel expiry (`async_ipc.rs:18`). Lanes reach `service_call` only for slot cleanup and PeerGone (`local_service_lifecycle.rs:172,275-287`); cancel/deadline legs use `PendingCall` (`:326-354,469-532`). | a | as cited |
| G4 | **Retention after the caller drops interest.** `PendingCall` has no `Drop`; dropping an untaken handle keeps the slot until thread exit (`ipc.rs:289-291`; owner exit `async_ipc.rs:760-808`). 64 such drops make the thread `Busy` forever (`:340-342`). Memory per retained op is fixed at submit: a 4096-byte reply buffer (`:328-330`; `ipc_wire.rs:17`) plus `Operation` (152 B, 03d CP2 A10); ≤ 64 per task (`:14`), ≤ 32 threads per Cell (`scheduler.rs:22`), so ≤ 2048 × 4248 B ≈ 8.7 MB per Cell. No test asserts the bound or release on take/exit. Charging of the reply buffer to the caller Cell is `[INFERENCE]`. | a | as cited |
| G5 | Restart + re-lookup: `ServiceRef::call` uses unbounded `service_call_typed` and invalidates on PeerGone/Indeterminate (`libs/ostd/src/service.rs:167-203`). Registry restart with re-lookup is C2C Phase 02 step 3 (`phase-02-local-boundary.md:33`, status pending); the bench has no `RegisterService` authority (`cells/tests/bench/src/main.rs:18-39`). Operation-level replacement is covered by `RESTART` + G2's `cutover_peer` test. | a | E-6 |
| G6 | **No fairness bound.** Submit checks only `pending_msgs.len() >= 64` (`async_ipc.rs:351-353`; `tcb.rs:27`). One Cell can hold all 64 slots; the lane asserts exactly that (`local_service_lifecycle.rs:1119-1124`). Bounded sync callers spin on `Busy` with `sys_yield` (`ipc.rs:465`); who wins a freed slot is unbounded. Existing evidence is finite (`phase-03-async-ipc.md:280-301`). | c | as cited |

Observation, not an obligation: `on_owner_exits` does not call `reservation_terminal` under `cellos_p01_trace` (`async_ipc.rs:796-805` vs `:658-659`); trace accounting only `[INFERENCE: no production effect]`.

## 3. Design (a): retained-reply lifetime

1. **SDK `Drop for PendingCall`** (`libs/ostd/src/ipc.rs`, OPEN E-5). The handle gains a `settled: Cell<bool>`, set when `try_take` returns a terminal or `InvalidOperation` (nothing left to release). `Drop` on an unsettled handle runs the existing `discard_call` (cancel, then take into an `IPC_BUF_SIZE` scratch, which always fits `MAX_IPC_WIRE_PAYLOAD`). Dropping stops waiting and releases the slot; it does not roll back execution (dispatched → `Indeterminate`, Spec 20 `:75`). A settled handle's drop issues no syscall, so the sync path is unchanged. Detach stays possible with `core::mem::forget` after `operation()`; no new API. `IpcCancel` shares allowlist bit 1 with `IpcTake` (`libs/api/src/abi/syscall.rs:1061`), which every handle already needs for `try_take`, so no new denial.
2. **No kernel retention timeout.** A live owner keeps its terminal until take (Spec 20 `:99`); the bound stays per-task 64 × fixed size, now asserted (row A6).
3. **Tests only** for G2/G3/G5 operation paths: host tests for `cutover_peer` and `on_owner_exits`; lane legs for bounded and unbounded sync timeout followed by a late reply and a successor call.

## 4. Design (b): cancellation of a dispatched operation

1. **Keep the semantics; no cancel-ack.** `IpcCancel` on `Queued` → `Cancelled` with the wire removed under the same hold; on `Dispatched` → `Indeterminate`; on a terminal → no-op, the existing terminal wins (`async_ipc.rs:619-663`). A provider acknowledgement would need a new provider-side syscall or `IpcReply` meaning (Law-1, two confirmations) and gives no stronger caller guarantee than `Indeterminate` unless providers implement rollback. OPEN E-2, proposal: none.
2. **Fix G1** with one helper `PendingMailbox::remove_delivered(pos, sender_tid, delivery_id: Option<u64>)` in `kernel/src/task/pending_mailbox.rs`: a wire is found by `delivery_id` (unique, `task.rs:368-374`); an inline record keeps the `pos + sender_tid` rule. The four receive arms call it (`syscall.rs:5452-5456`, `:5609-5613`, `:5815-5819`, `:5949-5962`); TryRecv's inline copy is folded into the same helper so one convention remains. Raw wires benefit identically. KR-owned lines (section 7).
3. **Definiteness invariant (stated, then tested):** an operation ends `Cancelled` or `PreDispatchTimeout` only if no receive snapshot of its wire ever happened; any snapshot first moves it to `Dispatched`, after which only `Reply`, `Indeterminate` or `PeerGone` are reachable.

## 5. Design (c): peer-queue admission headroom

**Rule** (in `async_ipc::submit`, beside `:351`, as a pure predicate `peer_queue_admits(receiver, binding)` for host tests): with `D = HOTSWAP_MSG_QUEUE_DEPTH = 64` and a new private `PEER_QUEUE_HEADROOM = H = 8`, refuse `Busy` when `len ≥ D`, **or** when `len ≥ D − H` and the receiver already holds an operation wire whose header `(sender_cell_id, sender_generation)` equals the submitter's `(cell, generation)`. Identity comes from the kernel-written wire header (`async_ipc.rs:302-308`), never caller input. Scan ≤ 64 entries under `SCHEDULER`, no allocation. Raw `Send`/`Post`/input admission is unchanged (`task.rs:2585,2671,3139`). OPEN E-4.

**Bounds** (all under `SCHEDULER`, so SMP-safe; FIFO append `pending_mailbox.rs:188`; a mask-0 receive takes the head, `syscall.rs:5271,5578,5655,5881`):
- **B1 per-Cell cap:** a Cell holds ≤ D − H = **56** queued operation wires at one receiver (it can add only below 56, or when it holds none).
- **B2 fresh admission:** a Cell holding no queued wire is refused only when `len = D`. An entry now at position p > D − H was appended when `len ≥ D − H` (positions only decrease), so its Cell held no wire ahead of it; hence the last H entries come from H distinct Cells, each distinct from every Cell ahead. With no raw traffic to that receiver, a fresh Cell is therefore refused for capacity **only if ≥ H + 1 = 9 other Cells each hold a queued wire there**. A synchronous caller always holds none (one call at a time), so with ≤ 8 other competing Cells it is **never** refused.
- **B3 progress:** an admitted wire is dispatched within **≤ D − 1 = 63** mask-0 receives by the provider.
- Ceiling: > 8 simultaneously saturating Cells, raw-message floods and provider masked-receive policy are not bounded (section 11).

## 6. Public ABI delta

**None.** No syscall number, argument, flag, allowlist bit, completion source, record, `IPC_STATUS` byte, wire header or `libs/api` byte changes; `scripts/check-multi-source-wait-law1-digests.sh` and `check-lookupservicebound-law1-digests.sh` must still pass. Law-1 is not triggered (`phase-03-async-ipc.md:20,33`). **Behaviour deltas** (documented in Spec 20 §2.6): a Cell already holding queued operation wires at a receiver sees `Busy` at 56 instead of 64; dropping an unsettled `PendingCall` cancels and releases it; a masked receive no longer removes the wrong record.

## 7. File-owner handoff (DRAFT; not submitted)

**7.1 Requested closure** (pre-edit SHA-256 at HEAD `56381531`, first 16 hex; re-taken in full at P0). **KR** = active kernel-repair ownership: Phase 07 is `in-progress` and lists `task/syscall.rs`, `task/scheduler.rs`/`task/smp.rs` (`260927-0739-…/phase-07-warm-snapshot.md:4,17`; `plan.md:49`).

| # | Path | Purpose | SHA-256 before | Surface |
|---|---|---|---|---|
| 1 | `kernel/src/task/async_ipc.rs` | `PEER_QUEUE_HEADROOM`, `peer_queue_admits` in submit; host tests A2, A4, A6, A7 | `d04166ebd2760626` | async IPC |
| 2 | `kernel/src/task/pending_mailbox.rs` | `remove_delivered` + host test A1 | `475d7bfa2f9fa5f1` | mailbox |
| 3 | `kernel/src/task/syscall.rs` | **only** `:5452-5456`, `:5609-5613`, `:5815-5819`, `:5949-5962` → `remove_delivered` | `56bca7b804f06c83` | **KR** |
| 4 | `libs/ostd/src/ipc.rs` | `settled`, `Drop`, docs `:247-249,289-290,404-407`; unit test of the settle mapping | `a6f58e02d29fef34` | SDK |
| 5 | `cells/tests/bench/src/scenarios/async_lifecycle.rs` | legs `CANCEL-RACE`, `SYNC-TIMEOUT-LATE`, `DROP-RECLAIM` (both topologies) | `e89a8db4cc137b32` | tests |
| 6 | `cells/tests/bench/src/scenarios/local_service_lifecycle.rs` | **rewrite** `peer_pressure_leg`/`run_pressure_caller`/`run_pressure_provider` (`:1061-1164`); sync-expiry probe in the `DEADLINE` window | `3f37f80ed1b376cc` | tests |
| 7 | `cells/tests/bench/src/bench-probe.rs` | role dispatch for new provider/caller roles only | `7bf32daeeca502e4` | tests |
| 8 | `tests/integration/tests/local-service-lifecycle-x86.rs` | replace markers `:137-138`; new markers; add `!contains("denied for tid")` to test 1 (`:85-216` lacks it; async test has it `:277-280`) | `0914cb9e71e1e141` | tests |
| 9 | `docs/specs/20-unified-ipc-contract.md` | §2.6 capacity `:100`, drop/retention `:99`, fairness `:160,169,171`; §7 changelog line | `4dc8b4cd904bcd9e` | docs |
| 10 | `docs/evidence/c2c-phase03-closure-x86.{txt,log}` | new | new | evidence |
| 11 | `.agents/260927-1100-c2c-anywhere-tier-aware/reports/phase-03e-cp2-package.md` | new CP2 record | new | plan |
| 12 | `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | progress, criteria ticks, status | `691e0dda9967aa81` | plan |
| 13 | `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md` | scheduling record + Phase 03 row | `3c09006649e0fcbd` | plan |
| 14 | `.agents/cell-contract-0024-locks.md` | scheduling record beside slice d (`:136-142`) | `ba004baa4fb5a9a2` | plan |
| 15 | `.agents/plan-portfolio.md` | scheduling record | `f66fdbc57e4fe27f` | plan |
| 16 | `.agents/TODO.md` | close the three Phase 03 items | re-taken at P0 | plan |
| 17 | `CHANGELOG.md` | only if E-8 = this slice | `01838faad8b5a3a2` | docs |

17 entries (18 files with the evidence pair): 8 source/test, 9 doc/evidence/plan. **Pinned byte-identical:** `kernel/src/task.rs` (`e78bbb3413cba46b`; headroom lives in `async_ipc.rs`, and its `ipc_recv*` commits already use `delivery_id`), `kernel/src/task/scheduler.rs` (`8267292a2c5d4d3b`; `expire`/`on_owner_exits` call sites `:1584,1791-1792,2231` unchanged), every `syscall.rs` line outside row 3, `smp.rs` (`442edc8ac3838582`), `completion.rs` (`c3e3388b32ef8ab3`), `completion_wait.rs` (`670279bc3cc6807d`), `tcb.rs` (`39add25c90710faa`), `ipc_pending_selftest.rs` (`2e6ca23137b75965`), `kernel/src/cell/hotswap.rs` (`06259ec2f09b6a53`), `libs/api/**` (`syscall.rs` `d0388f9e995da1b0`, `completion.rs` `45656c0b2225a38a`), `libs/ostd/src/service.rs` (`1f1f4b8466b1cbcd`), `libs/ostd/src/executor.rs`, `cells/services/**` (httpd `reactor.rs` `460f54ca02518df3`), Spec 17 (`916f4cb1db491086`), `Cargo.lock` (`0795d4d0a65d1804`).

**7.2 Lanes and self-tests that assert current behaviour** (slice d missed `completion_selftest.rs`, amendment A1):

| Asserts today | Where | After this slice |
|---|---|---|
| A fills 64 queued wires; independent B is refused (`initial_peer_busy=true`) | `local_service_lifecycle.rs:1119-1124,1144-1147,1163`; `local-service-lifecycle-x86.rs:137-138` | **changes**: A stops at 56, B admitted first try (rows 6, 8) |
| "Dropping the handle does not release its slot" | `ipc.rs:289-290` | **changes** (row 4) |
| Spec 20 capacity row and "not fairness" sentences | Spec 20 `:100,160,169,171` | **changes** (row 9) |
| `c2c-peer-pressure-x86.txt` describes the 64-slot fill | `docs/evidence/` | historical; not edited; superseded in row 12 |
| Masked `TryRecv` ×5, `RecvScatter` mask 0 atomicity, `Recv` mask 0 VFS lease | `ipc_pending_selftest.rs:677-870`; `ipc_wire_selftest/scatter_case.rs:54,98`; `vfs_lifecycle_selftest.rs:927`; x86 marker `scripts/x86/qemu-domain-test.sh:241` | must **not** change (regression, A8) |
| Full-mailbox raw `Send`/`Post` refusal | `ipc_pending_selftest.rs:396-416` | unchanged (raw admission untouched) |
| `MULTI-CALLER a_held=64`, `SATURATION accepted=64`, `MSW-STARVATION` (24 outstanding) | `local_service_lifecycle.rs:405-409,949-953`; `async_lifecycle.rs:417` | unchanged: each request is received before the next submit, or ≤ 24 < 56 |
| No `denied for tid` | `local-service-lifecycle-x86.rs:278`; `igb-x86.rs:194` | must hold; new legs use only existing bench allowlists (`bench-probe.rs:13-30`, `main.rs:18-39`) |
| A specific denial **is** expected | `scripts/qemu-phase03-pipeline-test.sh:918` (other plan) | unaffected |

**7.3 ADR-0024 delta.** Principal/incarnation: headroom identity is the existing `(cell, generation)` in the kernel wire header (ADR-0023 §2.1). No handle, delegation, revocation or durable state. Kernel vs policy: the kernel bounds admission and fixes delivery identity; retry and idempotency stay with the caller (Spec 20 §2.6). Concurrency: all transitions under `SCHEDULER`. Remote: none. No ambient authority.

**7.4 Lock rows (read-only at HEAD `56381531`).**

| Row / owner | Relation | Asked |
|---|---|---|
| `260927-1100-c2c-anywhere-tier-aware` (`locks.md:182`) | direct | narrow for slice e only; whole-plan relation stays |
| controlling P04 "correlated operations, wait/completion, cancellation" (`:99`); P03 "SMP linearization" (`:98`) for A1/A2 | controlling | owner = the user (as for 03d) |
| `260927-0739-kernel-architecture-repair` (`:181`; Phase 07 in progress) | **KR overlap**: row 3 | **blocker until ruled** (E-3) |
| `261006-0934-httpd-single-cell-concurrency` (`:184`) | check: async IPC/completion capacity; httpd caps 48 outstanding (`reactor.rs:21`) < 56, no httpd edit | rule compatible |
| `260615-1325-vicell-app-sdk-v1` (`:153`) | check: SDK lifecycle (row 4 `Drop`) | rule compatible, or drop E-5 |
| `260727-2101-midori-lessons-cellos` (`:165`), `260801-parallel-midori-closure` (`:167`) | check: completion/peer-death; no reactor/executor edit | rule compatible |

**7.5 WIP rule.** One slice at a time under one integrator (`locks.md` procedure step 5). Re-check at slice start that no admitted slice (including x86 board-selection phase 03, portfolio `:176`) claims a row-1..8 path; stop if one does `[INFERENCE: none today beyond portfolio text]`. **Nothing here authorizes a source edit.**

## 8. Order

**CP1 approval → P0 baseline → P1 tests-first → P2 fixes → P3 lanes → CP2.** No prototype-before-ABI step is needed (no ABI).
- **P0 (no edits):** host `-p api`, `-p ostd`, `-p cellos-kernel` (`--target x86_64-unknown-linux-gnu`, 03d D1); lifecycle integration 3/3; record counts and that `PEER-PRESSURE` passes today.
- **P1:** add A1, A2, A4, A6, A7 host tests; A1 and A7 must **fail against HEAD logic** (position commit; depth-only admission), recorded in the CP2 package.
- **P2:** rows 1-4. **P3:** rows 5-8, 10. **CP2:** post-edit digests, diff stat, deviation log, evidence.

## 9. Acceptance matrix (host + x86 QEMU lifecycle ISO; falsified by any counter ≠ expected)

| # | Case | Host test | x86 lane witness | Pass criterion |
|---|---|---|---|---|
| A1 | G1 commit identity | mailbox `[X-op, S-1, S-2]`, snapshot `S-1`, remove `X`, commit | `CANCEL-RACE` (A2) with a **masked** `RecvTimeout` provider, `-smp 1` and `-smp 2` | `S-1` removed, `S-2` still queued and later delivered once; inline records keep the position rule; missing `delivery_id` → no removal; lane `delivered_twice=0`. Fails on HEAD logic. Ceiling: lane interleaving unforced |
| A2 | Cancel definiteness (b) | matrix {`cancel`, `expire`, `on_owner_exits`, `peer_died`, `cutover_peer`} × {Queued, Dispatched} + cancel before/after `dispatch` | `CANCEL-RACE`: 256 × (submit; immediate `cancel`) against a provider that logs every received sequence | `Cancelled`/`PreDispatchTimeout` ⇒ never received; received ⇒ `Indeterminate` or `Reply`; `cancelled_but_received=0 received_but_definite=0 unterminal=0 total=256`, both topologies |
| A3 | Sync timeout + late reply (a) | — | `SYNC-TIMEOUT-LATE`: `service_call_typed_bounded` (20 ticks) to a holding provider, then explicit and Send-bridge late replies, then a second call; queued variant with a sleeping provider; unbounded `service_call_typed` run by a probe inside the existing `DEADLINE` window | `first=Indeterminate late_explicit=refused late_send_raw_absent=true next_seq=2`; queued `PreDispatchTimeout queued_delivered=0`; unbounded `Indeterminate elapsed_ticks>=3000 late=refused fresh=Reply` |
| A4 | Replacement / hotswap (a) | `cutover_peer` with Queued + Dispatched; replacement-generation reply; `on_owner_exits` | existing `RESTART` marker unchanged | Queued → `PeerGone` + wire excluded; Dispatched unchanged until `peer_died` → `PeerGone`; replacement reply `Invalid`; its own token `Reply` |
| A5 | Dropped interest (a) | settle mapping unit test (`ostd`) | `DROP-RECLAIM`: drop 192 unsettled handles (pending, terminal Reply, Indeterminate), provider then late-replies to all; then 64 fresh submits | `dropped=192 fresh_accepted=64 busy=0 late_accepted=0`; no `denied for tid` |
| A6 | Retention bound (a) | 64 terminals keep `Busy`; take frees one; owner exit + task drop frees all; terminal transition allocates 0 (`alloc_probe`); reply buffer = 4096 B fixed | — | asserted counts; per-Cell worst case ≤ 32 × 64 retained ops documented in Spec 20 |
| A7 | Headroom bounds (c) | deterministic PRNG model, 100,000 steps, 2, 9 and 10 Cells, D = 64, H = 8; mutation H = 0 | — | B1 ≤ 56; B2 no fresh refusal with ≤ 8 other Cells; B3 ≤ 63 receives; H = 0 violates B2 (test bites). Fails on HEAD logic |
| A8 | Fairness lane (c) | — | rewritten `PEER-PRESSURE`: A fills while the provider sleeps, then sustains its cap for ≥ 2,000 submissions; B makes 64 bounded sync calls | `a_fill=56 a_headroom_busy=true b_first_admitted=true b_fresh_refused=0 b_done=64 duplicate=0 wrong_correlation=0`; provider `max_queue<=57` |
| A9 | Regression | api, ostd, kernel host suites; `net_rx_selftest`, `ipc_pending_selftest`, `scatter_case`, `vfs_lifecycle_selftest` | lifecycle 3/3 (all other markers byte-identical); x86 domain `vfs-lifetime self-test PASS`; x86 cross-tier markers; RV64 broker 1000/1000 + soak; `igb-x86` 2/2 if the private image is rebuilt | counts ≥ P0 + new; 0 `denied for tid`; fmt + clippy `-D warnings` (riscv64, aarch64, x86 kernel) |
| A10 | ABI | `git diff --stat libs/api` empty | — | both Law-1 digest scripts PASS |

## 10. Success-criteria map (`phase-03-async-ipc.md:39-42`)

| Criterion | Evidence today | Gap closed here |
|---|---|---|
| 1 nonparking submit / `Busy` with nothing delivered / one correlated completion | step-1/3 legs; `SATURATION busy_delivered=0`; `MULTI-CALLER`; `MSW-REPLY` | none; A8 re-asserts under headroom |
| 2 full queue, dup/late, death, cancel, deadline → terminal, no silent loss or double execution | `CANCEL-LATE-REPLY`, `DEADLINE`, `CALLER-DEATH`, `RESTART`, `MSW-*`, host tests §2 | G1 double delivery (A1), G2 (A2, A4), G3 (A3), G4 (A5, A6), G6 (A7, A8) |
| 3 masked recv, VFS grant lifetime, TIMER/NET_RX valid in QEMU; two-hart wake race | `CONCURRENT-INPUT`, `EVENT-COEXISTENCE`, `qemu-domain-test.sh:241`, two-hart models, 03d lanes | masked-receive commit (A1); NET_RX on x86 stays test-hook only (03d D-8) |
| 4 no new ABI without confirmations; no pipelining claim | 03d CP1/CP2 approvals; this slice adds none | A10 |

Remaining after CP2 (named, not gaps of the criteria): registry restart + re-lookup (Phase 02 step 3, E-6); broker `NetworkPoller` (03d D-5); x86 NET_RX producer; physical Intel; the pre-existing x86 domain FAILs `MMIO-REVOKE-USERBIT` and `X86-VMM-SMOKE` (`phase-03-async-ipc.md:186-189`).

## 11. Risks, rollback, non-goals

- **R1 KR overlap** (row 3): four commit hunks in a Phase 07 file. Mitigation: line-bounded handoff, start-of-slice re-check.
- **R2 Earlier `Busy`** for a Cell queuing > 56 wires at one receiver. `Busy` is already a defined, retried outcome; httpd caps at 48; other submitters (`c2c-render`, `ocel`, `cellbox`, init activation) hold ≤ a few `[INFERENCE]`.
- **R3 Raw floods** can still occupy the headroom; B2 is stated "with no raw traffic". Covering raw admission would touch `task.rs` (KR) and raw semantics.
- **R4 `Drop` discards** a result a caller meant to take by id after dropping: none found (`.operation()` users keep the handle: `async_lifecycle.rs:595-920`, `local_service_lifecycle.rs:676`, `parked_footprint.rs:44`).
- **R5 Receive commit** touches every `Recv`/`RecvScatter`/`RecvTimeout`; inline records keep the old rule; A9 self-tests guard it.
- **R6 Evidence ceiling:** G1 and B2 are proven by host models; QEMU interleavings are unforced (03d D-8 precedent).
- **Rollback:** revert the closure as one change; no persistent state, no ABI. **Security:** identities come from kernel records only; the scan is bounded; no new allowlist bit.
- **Non-goals:** cancel-ack or provider-side cancellation; raw-mailbox fairness; > 8 saturating Cells; provider masked-receive policy and scheduling weights; per-NodeId/remote fairness; time-based reclaim of untaken terminals; registry restart (Phase 02); broker cutover; x86 NET_RX producer; `transfer_consumption` exposure; Cap'n Proto; physical Intel/production.

## 12. Decisions for the owner

| ID | Decision | Proposal |
|---|---|---|
| E-1 | Adopt slice e as one bounded slice under controlling P04 (P03 for A1/A2) with the §7.1 closure; one scheduling record in `locks.md`, `plan-portfolio.md`, `plan.md` | adopt |
| E-2 | Dispatched cancellation: keep `Indeterminate`, no ABI vs design a provider cancel-ack (Law-1) | keep, no ABI |
| E-3 | KR handoff for `syscall.rs:5452-5456,5609-5613,5815-5819,5949-5962` only vs wait for Phase 07 (G1 stays open; criterion 2 not closable) | grant, lines only |
| E-4 | Headroom H = 8 of 64, identity `(cell, generation)`, operation submits only vs other H / per-tid / include raw admission | as proposed |
| E-5 | `PendingCall` drop cancels + takes an unsettled operation (detach via `mem::forget`) vs keep the documented leak | drop releases |
| E-6 | Registry restart + `ServiceRef` re-lookup stays in C2C Phase 02 step 3 vs added here (needs bench registry authority) | Phase 02 |
| E-7 | On CP2, mark Phase 03 done at the x86 QEMU/TCG ceiling with §10's named exclusions | yes |
| E-8 | `CHANGELOG.md` entry in this slice vs a later change (03d deferred it) | this slice |

## 13. Not checked

- No build/test/QEMU run at this HEAD; P0 counts unknown.
- Whether x86 syscall copy-out runs with interrupts enabled (G1 single-hart trigger), and whether AP harts run Cell tasks in the lifecycle ISO (03d §13).
- Which Cell quota is charged for the 4096-byte reply buffer allocated in `submit`.
- Outstanding-call depths of `c2c-render`, `ocel` and init activation under load.
- Out-of-tree consumers of the `PendingCall` drop behaviour.
