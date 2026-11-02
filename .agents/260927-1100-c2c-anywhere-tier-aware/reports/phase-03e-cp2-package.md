# C2C Phase 03 slice e CP2 package (closure; no Law-1 surface)

Status: **implemented, independently reviewed (PASS_WITH_RISK, findings resolved in §7) and re-verified; awaiting the owner's CP2 ruling (E-7: mark Phase 03 done).** Nothing is committed. CP1: [phase-03e-closure-cp1.md](../phase-03e-closure-cp1.md); approval: [phase-03e-closure-cp1-approval.md](../phase-03e-closure-cp1-approval.md); review: `agent://SliceEReview`. Source HEAD `5638153118921e53c510571ba5dc50c26d73b0a7` plus the uncommitted delta (15 tracked files, +1482/−154 including records; new: this package, the evidence pair, the two CP1 documents). Evidence: [c2c-phase03-closure-x86.txt](../../../docs/evidence/c2c-phase03-closure-x86.txt) (+ git-ignored `.log`).

## 1. P0 baseline (before any source edit)

Pre-edit SHA-256 (first 16) equal CP1 §7.1: `async_ipc.rs` `d04166ebd2760626`, `pending_mailbox.rs` `475d7bfa2f9fa5f1`, `syscall.rs` `56bca7b804f06c83`, `ipc.rs` `a6f58e02d29fef34`, `async_lifecycle.rs` `e89a8db4cc137b32`, `local_service_lifecycle.rs` `3f37f80ed1b376cc`, `bench-probe.rs` `7bf32daeeca502e4`, `local-service-lifecycle-x86.rs` `0914cb9e71e1e141`, Spec 20 `4dc8b4cd904bcd9e`, `phase-03-async-ipc.md` `691e0dda9967aa81`, `TODO.md` `e2f81b776d63bb39`, `CHANGELOG.md` `01838faad8b5a3a2`.

| Suite (`--target x86_64-unknown-linux-gnu`; `RUSTFLAGS`, `CARGO_TARGET_DIR` unset) | P0 | Now |
|---|---|---|
| `-p api` | 184 + 2 (4 doc-tests ignored) | 184 + 2 |
| `-p ostd` | 86 + 7 + 19 (2 ignored) | **87** + 7 + 19 |
| `-p cellos-kernel` | 392 | **400** |
| `local-service-lifecycle-x86` | 3/3 (ISO `fe96963f0d846dd1`) | 3/3 (ISO `395b1b1f426f5526`) |

## 2. Reproduction first

| Defect | Failing-before | Observed before | After |
|---|---|---|---|
| (b) masked-receive double delivery | host `masked_receive_commit_removes_the_delivered_record_after_a_concurrent_cancel` against the receive arms' position rule, extracted verbatim into `PendingMailbox::remove_delivered` | `the commit removed a record it never delivered — left: Some(12) right: Some(11)`: op 12 removed, op 11 left to be delivered again; companion inline-rule test also failed | both PASS; arms rewired. **Host-proven only** (§7 R1) |
| (a) dropped `PendingCall` slot leak | lane leg `DROP-RECLAIM` with the HEAD SDK (ISO `7f64f2d2f930464f`) | `[async-lifecycle] FAIL — admission stayed Busy: a released slot was never returned` after the first 64 drops | `DROP-RECLAIM dropped=192 fresh_accepted=64 late_accepted=0`, `-smp 1` and `-smp 2` |

## 3. Delta

| Row | File | Change |
|---|---|---|
| 1 | `kernel/src/task/async_ipc.rs` | `PEER_QUEUE_HEADROOM = 8`, `peer_queue_admits` checked in `submit`'s first hold (before the request copy and 4 KiB reply allocation) and again in the commit hold; 8 host tests; test builder `operation()`; `alloc_probe` counts freed bytes |
| 2 | `kernel/src/task/pending_mailbox.rs` | `remove_delivered(pos, sender_tid, delivery_id)`: wire by delivery id, inline by position; `debug_assert!` against a wire committed with `None` |
| 3 | `kernel/src/task/syscall.rs` (KR, E-3) | `Recv` 5452-5456, `RecvScatter` 5609-5613, `RecvTimeout` 5815-5819, `TryRecv` 5949-5962 (now one line) call `remove_delivered`; no other hunk |
| 4 | `libs/ostd/src/ipc.rs` | `PendingCall.settled`, `Drop` → `discard_call`, `take_settles`, docs; unit test |
| 5 | `cells/tests/bench/src/scenarios/async_lifecycle.rs` | legs `CANCEL-RACE`, `SYNC-TIMEOUT-LATE`, `DROP-RECLAIM`; roles race/sync-hold/drop |
| 6 | `cells/tests/bench/src/scenarios/local_service_lifecycle.rs` | `PEER-PRESSURE` rewritten; `SYNC-EXPIRY` inside the `DEADLINE` window |
| 7 | `cells/tests/bench/src/bench-probe.rs` | role dispatch only |
| 8 | `tests/integration/tests/local-service-lifecycle-x86.rs` | `PRESSURE-*`/`PEER-PRESSURE` markers replaced, `SYNC-EXPIRY`, 03e async markers, `denied for tid` check on the local test |
| 9-16 | Spec 20 v3.5 (§2.6, §7), evidence pair, this package, phase file, `plan.md`, locks, portfolio, `TODO.md` | records |
| 17 | `CHANGELOG.md` | Fixed ×2 (incl. the SDK `Drop`/`!Sync` note), Changed ×1 (E-8) |

Pinned and byte-identical (`git diff --quiet`): `task.rs`, `scheduler.rs`, `smp.rs`, `completion.rs`, `completion_wait.rs`, `tcb.rs`, `ipc_pending_selftest.rs`, `cell/hotswap.rs`, `libs/api/**` (0 diff lines), `ostd/{service,executor}.rs`, Spec 17, `Cargo.lock`. **ABI delta: none.**

## 4. Acceptance matrix (CP1 §9)

| # | Host | x86 lane | Result |
|---|---|---|---|
| A1 | `masked_receive_commit_…`, `remove_delivered_keeps_the_inline_rule_…`, `a_wire_committed_without_its_delivery_id_is_caught` (debug) | **none**: no lane can place a cancellation between a receive's two holds; `CANCEL-RACE` cannot trigger G1 | pass, **host-only** |
| A2 | `definite_not_executed_outcomes_only_for_never_snapshotted_requests` (5 producers × 2) | `CANCEL-RACE total=256 cancelled=64 indeterminate=192 cancelled_but_received=0 received_but_definite=0 unterminal=0` both topologies | pass |
| A3 | — | `SYNC-TIMEOUT-LATE queued=PreDispatchTimeout first=Indeterminate late_explicit_refused late_send_raw_absent next_reply`; `SYNC-EXPIRY=OK unbounded=Indeterminate elapsed_ticks>=3000 late_reply=refused fresh=Reply` both topologies | pass |
| A4 | `hotswap_cutover_ends_queued_work_and_the_replacement_cannot_settle_old_tokens` | `RESTART` unchanged | pass |
| A5 | `only_a_consumed_or_unknown_operation_settles_its_handle` | `DROP-RECLAIM` | pass |
| A6 | `retained_terminals_stay_charged_…`: quota 64 after setup, after all terminal transitions (0 allocations) and after owner exit; push refused when full; take frees one slot and ≥ 4 KiB; reaping the exited owner frees ≥ 64 × 4 KiB | — | pass |
| A7 | `peer_queue_headroom_refuses_a_fresh_cell_only_behind_nine_others` (exact limit: share 56, 8 headroom holders each refused a second wire, fresh refused behind exactly 9, admitted after one receive, another incarnation is a separate holder); `peer_queue_headroom_bounds_monopolization`: B3 as receives from admission to dispatch read from the dequeued record — 2/9/10 Cells `max_held` 56/55/54, receives-to-dispatch 57/59/59, fresh refusals 0; crowd of 40 Cells `max_held` 48, receives-to-dispatch 64, 12 fresh refusals, fewest other holders 28, 0 B2 violations; depth-only rule `max_held=64`, 350 refusals behind 1 holder | — | pass; bites on the old rule |
| A8 | — | `PRESSURE-FILL a_fill=56 a_headroom_busy=true b_first_admitted=true`; `PEER-PRESSURE … b_fresh_refused=0 b_done=64`; A at cap: `a_busy` 10036 / 9893 over 3772 / 1366 submissions (`-smp 1` / `-smp 2`); `b_max_call_ticks=1` | pass |
| A9 | api/ostd/kernel counts §1 | lifecycle 3/3; independent sessions local + async on `-smp 1` and `-smp 2` all PASS; `igb-x86` 2/2 on the round-1 private image (`92ff130fab481b60…`); 0 `denied for tid` | pass (igb not re-run in round 2) |
| A10 | `git diff libs/api` empty | both Law-1 digest scripts PASS | pass |

Also: `cargo fmt --all --check` clean; clippy `-D warnings` rc=0 on the riscv64 workspace (CI exclusions), `-p cellos-kernel` aarch64-softfloat and x86_64-none (only pre-existing build-script notices). Re-run after round 2.

## 5. Deviations

| # | Deviation | Effect |
|---|---|---|
| E1 | (b) is reproduced by a host test of the extracted commit rule (byte-for-byte the three arms' `pos + sender_tid` check), not by driving the syscall arms; the arms were then rewired to the fixed helper. The only arm-level guard is a debug assertion (host and debug kernels); release lanes cannot observe an arm passing `None`. | proof form |
| E2 | The (a) reproduction ISO already carried the round-1 kernel fixes; only the SDK was at HEAD. The leak is SDK-only. | provenance |
| E3 | The first (a) run failed earlier in `CANCEL-RACE` on a fixture bug: plain `Recv` returns queued death records mask-agnostically (the MSW caller's exit watch). `recv_frame_from`/`expect_from` skip records from other senders. | fixture only |
| E4 | CP1 A8 named a provider `max_queue<=57` counter and "≥ 2,000 A submissions". The provider cannot see its queue depth; B1 is proven by `a_fill=56` and A7. A pumps until B finishes (3772 / 1366 submissions); `a_busy` > 0 is the "at cap" witness. | evidence shape |
| E5 | `max_a_between_b` (111 / 153) counts A receives between consecutive B receives, including A wires admitted during B's reply turnaround; reported, not bounded. B3 is A7's receives-to-dispatch (≤ 64). | measurement label |
| E6 | `remove_delivered`'s inline branch also requires the record to have no wire. Inline records are never wires, so behaviour is unchanged. | stricter |
| E7 | A6 cannot call `submit` on host; it builds operations with the same 4 KiB reply buffer and asserts quota, allocation and freed bytes directly. Kernel cell-heap quota charging is not observable on host. | proof form |
| E8 | Lanes and serial sessions were built after `cargo fmt`; the reproduction ISOs predate it. `igb-x86` ran on the round-1 tree only. | provenance |
| E9 | CP1 line numbers for `syscall.rs` refer to pre-edit bytes; the `TryRecv` hunk is now one line. | provenance |
| E10 | CP1 §4.2 and the approval text say "one convention" / "every receive arm". Exactly: the four syscall arms commit wires by delivery id; `ipc_recv*` and the blocking-resume commit already did; inline records keep sender/position rules (blocking resume: first from that sender). Production inline records come only with sender 0. Spec 20 and `CHANGELOG.md` are worded exactly; the bound CP1 is not edited. | wording |
| E11 | B2 as stated in CP1 §5 is Cell-wide and assumes no raw records at the receiver; a synchronous thread whose sibling holds a queued request is not fresh, and an unbounded `service_call` returns `Busy` without retrying. Spec 20 §2.6, `CHANGELOG.md` and the kernel doc comment now say so. | wording |
| E12 | `PendingCall` is now `Drop` and `!Sync` (SDK-visible; recorded in `CHANGELOG.md` for out-of-tree users). A take that fails pointer validation (`InvalidOperation`) still marks the handle settled, so its drop skips the release; this leak predates the slice and is not changed. | known residual |

## 6. Post-edit SHA-256 (first 16)

`async_ipc.rs` `21ea90d9fedbf28e`, `pending_mailbox.rs` `5ab01cc30ec4488d`, `syscall.rs` `0eb2b9e50fb3d888`, `ostd/ipc.rs` `8feb88b6b56b96f2`, `async_lifecycle.rs` `dfefec4d4430d4b4`, `local_service_lifecycle.rs` `fbfd78cf67f53c5f`, `bench-probe.rs` `b5759f977a091987`, `local-service-lifecycle-x86.rs` `6647fd1a6283a337`, Spec 20 `eb564f79ff485c6d`, `CHANGELOG.md` `e281edbe37344ca9`, evidence `.txt` `91006a1b215c1fd3`, `.log` `f6c989ca910da110`, `TODO.md` `55c1ddc311f89515`, `phase-03-async-ipc.md` `a4d66f031bbadb3d`.

## 7. Independent review (PASS_WITH_RISK) → resolution

| Finding | Resolution |
|---|---|
| R1 MED `CANCEL-RACE` cannot trigger G1 | A second Cell cannot be forced into the copy-out window between the two holds; **A1 relabelled host-only** here, in Spec 20 and in the evidence. `CANCEL-RACE` stays as the A2 witness. |
| R2 MED A7 B3 check vacuous; B2 never exercised near the limit | B3 is now provider receives from admission to dispatch, read from the dequeued record. New deterministic boundary test pins the exact limit (refusal only behind 9 holders); a randomized crowd run (40 Cells) produces 12 fresh refusals, all behind ≥ 28 holders. |
| R3 MED overclaimed wording | Spec 20 §2.6 Capacity row, the CHANGELOG entry and the kernel doc comment state Cell-wide identity, the no-raw-records condition, and that unbounded `service_call` returns `Busy` (E11). |
| R4 LOW A6 lacks quota/owner-exit assertions | A6 asserts quota at each stage incl. after owner exit, and freed reply bytes on take and on reaping. |
| R5 LOW no guard for an arm passing `None` | `debug_assert!` in `remove_delivered` + `#[should_panic]` host test (debug only; E1). |
| R6 LOW "every receive arm" / "one convention" | E10; Spec 20 and CHANGELOG reworded. |
| R7 LOW `PendingCall` `Drop`/`!Sync` for out-of-tree users | CHANGELOG sub-bullet (E12). |
| R8 LOW settled-on-`InvalidOperation` leak | Pre-existing; recorded E12, unchanged. |
| R9 LOW headroom refusal after the 4 KiB allocation | Early `peer_queue_admits` in `submit`'s first hold, before the copy, token and reply allocation; the authoritative check stays at the push. |

## 8. CP2 question

> Do you accept the slice e delta (§3), its evidence (§4), the review resolutions (§7) and deviations E1–E12, and, per E-7, mark C2C Phase 03 done at the x86 QEMU/TCG ceiling with the named exclusions (registry restart + re-lookup in Phase 02 step 3, broker `NetworkPoller` cutover, x86 NET_RX producer, physical Intel)? On YES the file-bounded exception ends; nothing is committed by this record.
