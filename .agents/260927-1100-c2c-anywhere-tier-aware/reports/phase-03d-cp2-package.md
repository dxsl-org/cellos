# Phase 03 slice d — CP2 package (Law-1 confirmation 2 request): multi-source `WaitCompletion`

**Refreshed 2026-11-02 after the independent review (`SliceDReview`, verdict FAIL).** Every finding is addressed below (§9).

- **Baseline:** HEAD `0effc2250688cc351d64dd2733fcef66f9941939`. The delta is **uncommitted**.
- **Inputs:** [CP1](../phase-03d-multi-source-wait-cp1.md), including its §14 deviation log; the [CP1 approval](../phase-03d-multi-source-wait-cp1-approval.md); and the [P0–P2 report](phase-03d-prototype.md).
- **Not yet done:** `CHANGELOG.md`, the Spec 17/20/03b edits (closure rows 17–19, drafted in §7) and the digest script (row 16). They land only after confirmation 2.

## 0. §7.1 closure amendment (ratified by the owner together with CP2; not pre-approved)

If the owner rejects an item, its edit is reverted and the dependent evidence is re-run.

| ID | Path | Edit | Why it was outside §7.1 |
|---|---|---|---|
| A1 | `kernel/src/task/completion_selftest.rs:119-126` | One assertion only: `NET_RX\|TIMER` with a finite deadline is accepted, and without a deadline is refused. | The boot self-test pinned the pre-03d single-bit rule. Approved by Main on 2026-11-02 as an amendment. |
| A2 | `cells/tests/bench/src/bench-probe.rs` `declare_syscalls!` | `WaitCompletion` (bit 42) added to the **test cell's** compiled allowlist. | Row 13 covered role dispatch only. This is not the signed operator policy (`sign-policy.py`/`POLICY.BIN` untouched), and no canonical image is rebuilt. |
| A3 | `reports/phase-03d-cp2-package.md` (this file) | New CP2 record. | Row 23 named only the prototype report. |
| A4 | `libs/ostd/src/lib.rs` (`#![feature(linkage)]`); `libs/ostd/src/runtime.rs` (`app_entry!` ×3, `service_entry!`: `#[export_name = "__vicell_syscall_allowlist"]` on the existing allowlist static) | The SDK reads the Cell's own compiled allowlist instead of probing a syscall it may be denied (review HIGH). | `lib.rs` and `runtime.rs` were not in §7.1. `declare_syscalls!` in `libs/api/src/abi/syscall.rs` (row 2) gets the same attribute. |
| A5 | `kernel/src/task/completion.rs` (`CompletionQueue::new` pre-sizes the waiter table to `MAX_WAITERS` and becomes fallible; `queue_for` allocates the queue under cell 0; a test-only `waiter_capacity`) | Removes wait-path allocation (review MED). | `completion.rs` was pinned in CP1 §7.1. |

## 1. Exact public ABI delta (Law-1 object)

### `libs/api/src/abi/completion.rs`
- Appends `source::IPC: u32 = 1 << 2`.
- Adds `const fn source::is_wait_mask_supported(mask)`: non-empty, and only bits from `NET_RX | TIMER | IPC`.
- `is_single_supported` also accepts `IPC`; this is the rule for a record's source word.
- Docs, plus three byte-level tests:
  - `IPC == 4`;
  - bytes `12..16 = 04 00 00 00` and `16..24` = token LE;
  - a mixed source word is rejected.

### `libs/api/src/abi/syscall.rs`
- `WaitCompletion = 242` and `events` documentation only.
- **New link-level contract:** `declare_syscalls!` now exports its existing `VICELL_SYSCALLS` static under the symbol `__vicell_syscall_allowlist`. The section bytes and the mask value are unchanged; only an exported symbol name is added.
- `ostd::app_entry!`/`service_entry!` (A4) do the same.
- `ostd` provides a weak default of `u64::MAX`, which equals the loader's reading of an absent section (`kernel/src/loader/governed_spawn.rs:14-22`).
- **Owner decision:** this symbol is a new public surface that is not an opcode. It is listed here so confirmation 2 covers it.

### Byte-compatible (verified)

| Item | Status |
|---|---|
| `COMPLETION_LEN` | 24 |
| `COMPLETION_VERSION` | 1 |
| `COMPLETION_MAGIC` | unchanged |
| Field offsets | unchanged |
| `NET_RX`, `TIMER`, `UNSPECIFIED` | `1`, `2`, `0` |
| Opcode 242 | arguments, return convention and allowlist bit 42 unchanged |
| `IpcSubmit`…`IpcReply` and `IPC_STATUS` | unchanged |
| `WaitForEvent` and `events` | unchanged |
| `__ViCell_syscalls` section contents | unchanged |
| A lone `NET_RX` or `TIMER` wait | keeps the original kernel path |

**Pinned (hashes equal P0):** `kernel/src/task/syscall.rs`, `smp.rs`, `libs/ostd/src/executor.rs`, `cells/services/net/**`, `cells/services/net-broker/**`, `Cargo.lock`, `CHANGELOG.md` and Specs 17/20/03b: 10 of 10. `completion.rs` left the pinned set through A5. No loader or launch-profile file changed.

## 2. Implementation summary

### Kernel

**`completion_wait.rs`:** `source_is_valid` implements the public mask rule. A lone NET_RX or TIMER keeps the original path; every other mask uses `wait_multi`:
- NET_RX readiness is resolved outside `SCHEDULER`.
- Selection happens under `SCHEDULER`.
- The park gate is `MailboxPending`/`IpcPending`/`Parked`.
- `settle_multi` builds the record, releases the reservation and re-levels any NET_RX evidence that is not reported.

**`finish_multi`:** if copying the record out fails, the IPC notification is re-armed (`async_ipc::restore_notification`) and NET_RX is re-levelled. A failed copy loses nothing (review LOW).

**`completion.rs` (A5):** a Cell's queue and its fixed 32-entry waiter table are allocated once, on that Cell's first wait, under cell 0 (kernel). The host test shows **2 allocations at queue creation** and **0 on the wait path** afterwards: `queue_for` on an existing queue, reserve, 32 × `register_waiter`/`clear_waiter`, release, the park gate and 192 selections.

**`ipc_pending_selftest`:** keeps NET_RX readiness local and no longer touches the global NET_RX flag on production boots (review LOW).

### SDK (`libs/ostd/src/ipc.rs`)
- `wait_any(sources, ticks) -> Result<Wake, IpcError>`.
- `wait`, `PendingCall::wait_and_take` and `exchange` use `WaitCompletion(IPC | TIMER)`, or `IPC` alone for an indefinite wait, **only when the Cell's compiled allowlist holds bit 42**. Otherwise they use `IpcWait`.
- No syscall is ever probed, so there is no denial log and no misclassification (review HIGH and the LOW concurrent-first-wait case). The former D8 heuristic is removed.
- Every error of the chosen syscall propagates.

### Tests and lane
- `bench-probe` dispatches `c2c-msw-{caller,echo,slow,hold,raw}`.
- Every counter the scenario prints is now computed: `lost`, `dup`, `foreign`, `stale_records`, `other_records`, `netrx_records` (review LOW).
- `tests/integration`: `boot_x86_bios_smp`; `MSW-*` marker assertions; a `-smp 2` test; and a lane-wide assertion that no `denied for tid` line appears.

## 3. M5 (terminal→running, `WaitCompletion(IPC)` vs `IpcWait`, 200 alternating rounds; criterion p99_wc ≤ p99_iw + 10 ms)

| Run | wc p50 / p99 | IpcWait p50 / p99 | Δp99 | Pass |
|---|---|---|---|---|
| `-smp 1` (evidence) | 22.86 / 450.97 µs | 23.43 / 346.20 µs | +0.10 ms | yes |
| `-smp 2` (evidence) | 23.98 / 470.60 µs | 24.55 / 320.30 µs | +0.15 ms | yes |

The p50 values are equal. **p99 is consistently 0.06–0.45 ms higher** on the completion path across all six runs recorded in this slice. It is within the threshold, but not better.

[INFERENCE: the extra reservation, release and waiter registration take `SCHEDULER` about three more times.]

## 4. Acceptance and pass counts

| Row | Host | x86 lane (`-smp 1` and `-smp 2`) | Result |
|---|---|---|---|
| A1 reply | one-shot and ordering tests | `MSW-REPLY records=8 dup=0 foreign=0 rereport=0 wrong_seq=0` | pass |
| A2 peer death | R3 | `MSW-DEATH records=4 peer_gone=4 dup=0 unterminal=0` | pass |
| A3 deadline | R4, R7 | `MSW-DEADLINE timer_ticks=20 op_expiry_ticks=3000 terminal=Indeterminate` | pass |
| A5 stale/late | stale/late test; restore test | `MSW-STALE late_reply_refused=true stale_records=0 other_records=0 quiet_after=true` | pass |
| A6 two-hart | R1–R8 model | `-smp 2` lane, unforced (D-8 label) | pass at the stated ceiling |
| A7 mixed | R6, R8; **settlement tests** (completed reservation while IPC wins, in-flight `RetryCompleting` resolved by a second thread, untouched reservation, NET_RX from the level flag, `abandon_multi` both forms) | `MSW-MIXED` all true, `netrx_records=0` (no x86 producer) | pass at the stated ceiling |
| A8 non-starvation | model: `max_returns=105` ≤ 192, `max_overtakes=34` ≤ 63 | `b_during_flood=8`, `max_b_overtakes=0`/`11` ≤ 63, `lost=0 dup=0 foreign=0` | pass |
| A9 regression | api / ostd / kernel | lifecycle 3/3; igb 2/2; `denied for tid` 0 | pass |
| A10 memory | 0 wait-path allocations; 2 per Cell at first wait (kernel-charged); `Operation` 144 → 152 B; `Task` +1 B | n/a | pass |
| A11 ABI bytes | api tests | n/a | pass |

**Pass counts** (`--target x86_64-unknown-linux-gnu`; `RUSTFLAGS` and `CARGO_TARGET_DIR` unset):

| Suite | P0 | Now |
|---|---|---|
| `-p api` | 181 + 2 (4 doc-tests ignored) | **184 + 2** |
| `-p ostd` | 84 + 7 + 19 doc (2 ignored) | **86 + 7 + 19** |
| `-p cellos-kernel` | 377 | **392**, 0 warnings |

New kernel host tests since the first CP2 draft:
- 6 in `completion_wait::tests`: the settlement and `abandon_multi` paths, and the pre-sized waiter table.
- 1 in `async_ipc::tests`: re-arm after a failed copy.

**Lanes:**

| Lane | Result | `denied for tid` |
|---|---|---|
| `local-service-lifecycle-x86` | **3/3** (105.1 s) | 0 (asserted) |
| Independent `-smp 1` / `-smp 2` serial boots (evidence log) | all `MSW-*` legs PASS; `completion-queue` and `IPC-PENDING` self-tests PASS | 0 |
| **`igb-x86`** on a private headless image with every cell rebuilt from this tree (`scripts/assemble-x86-image.py`, out-dir and target outside the repository; ISO `d1136900199b61b5…`) | **2/2** (`igb_x86_dhcp`, `igb_x86_vtd_dhcp`) | its own assertion holds |
| **`qemu-lua-vnet`** (RV64) | **not passable at HEAD; slice-independent** (details below) | not counted |
| Cross-architecture `cargo check` (rv64, aarch64-softfloat, x86_64-none) | clean | n/a |

Why `qemu-lua-vnet` cannot pass at HEAD:
- The Lua cell was built from this tree (it carries `__vicell_syscall_allowlist`). That needed a local RISC-V newlib, built from Ubuntu's `newlib-source` into `/tmp`, because the expected `riscv-none-elf-gcc` toolchain is absent here.
- The shared `disk_v3.img` already holds 63 cell entries, so the runner's two additions overflow the 64-entry table. I ran with a private trimmed copy instead.
- Even then the RV64 init's `/bin/shell` launch is refused (`DENY launch edge … target=/bin/shell`), so no shell prompt appears and Lua never runs.
- No loader or launch-profile file is in this delta. Its "no denied syscall" check passes only vacuously, so it is not counted.

**Files:** `docs/evidence/c2c-multi-source-wait-x86.{txt,log}` (log `6d1b13c9e15cfed7…`); lifecycle ISO `c08e1321a8dd3445…`.

## 5. Post-edit SHA-256 (first 16)

| Path | SHA-256 |
|---|---|
| `libs/api/src/abi/completion.rs` | `45656c0b2225a38a` |
| `libs/api/src/abi/syscall.rs` | `d0388f9e995da1b0` |
| `kernel/src/task/completion_wait.rs` | `46d8727f0242b535` |
| `kernel/src/task/async_ipc.rs` | `96e9582799d96064` |
| `kernel/src/task/completion.rs` (A5) | `c3e3388b32ef8ab3` |
| `kernel/src/task/tcb.rs` | `39add25c90710faa` |
| `kernel/src/task.rs` | `e78bbb3413cba46b` |
| `kernel/src/task/scheduler.rs` | `8267292a2c5d4d3b` |
| `kernel/src/task/waker.rs` | `91a429c2e81f9b29` |
| `kernel/src/task/ipc_pending_selftest.rs` | `2e6ca23137b75965` |
| `kernel/src/task/completion_selftest.rs` (A1) | `8d16f45ea69d5679` |
| `libs/ostd/src/lib.rs` (A4) | `10750738a023f118` |
| `libs/ostd/src/runtime.rs` (A4) | `879b0e9c12e1eae0` |
| `libs/ostd/src/syscall.rs` | `e672f9904a653caf` |
| `libs/ostd/src/ipc.rs` | `26a353cf9f8976b3` |
| `cells/tests/bench/src/scenarios/async_lifecycle.rs` | `73ae1da015f29735` |
| `cells/tests/bench/src/bench-probe.rs` (A2) | `7bf32daeeca502e4` |
| `tests/integration/src/lib.rs` | `fa3122e1fb60348e` |
| `tests/integration/tests/local-service-lifecycle-x86.rs` | `0914cb9e71e1e141` |

`git diff --stat` over `kernel libs cells tests`: 19 files, +2279 / −79. The granted kernel-repair lines are exactly `task.rs:346` and `scheduler.rs:2308`.

## 6. Deviations (P3; P1–P2 deviations are in CP1 §14)

| # | Deviation |
|---|---|
| D8 | **Resolved by A4.** The path is chosen from the compiled allowlist; there is no probe and no heuristic. |
| D9 | `ostd::ipc::wait(timeout) -> bool` keeps its signature, because its callers are outside the closure. A wait error still reads as `false`, as it did under `IpcWait`. `wait_any` is the error-carrying API. |
| D10 | Cells on the completion path (the 4 allowlisted Cells with bit 42, permit-all Cells, bench-probe) consume one-shot notifications. Every SDK path `try_take`s before parking (the review found no in-tree loser). A hand-rolled `wait_any` loop that mixes in SDK sync calls on the same thread must also `try_take` first. Allowlisted Cells without bit 42 stay on `IpcWait`. |
| D11 | A Cell with more than 32 threads parked in SDK waits at once gets `IpcError::Recv` (queue capacity) instead of parking. |
| D12 | In the lane's mixed leg, raw messages come from a context-free source Cell, because a provider's `Send` to the caller it is serving is that operation's reply. Now also stated in the §7 spec draft. |
| D13 | Lane A8 overtakes sit far below the bound. The adversarial evidence is the host model. |
| D14 | Spec 17/20/03b edits are held until confirmation 2. |
| D15 | The scenario includes one real 3000-tick expiry per topology; the lane timeout is `SCENARIO_TIMEOUT + 120`. |
| D16 | `qemu-lua-vnet` is blocked by the pre-existing faults in §4. Needed evidence inputs were built outside the repository: a local newlib in `/tmp` and a trimmed private disk copy. |
| D17 | The `igb-x86` image is a private candidate built with the lifecycle script's production kernel profile (`RUSTFLAGS` code-model kernel, cf-protection) and the manifest's `kernel_features`. It is not the canonical image, and the canonical bytes are untouched. |

## 7. Draft Spec 17 §9 amendment (not applied)

> - 2026-11-xx — **Recorded for a changed syscall surface; no ratified wire rule moved (Law-1 2× confirmed: `phase-03d-multi-source-wait-cp1-approval.md` and the CP2 record).**
>   - **New source:** `api::completion::source::IPC = 1 << 2` (bytes `04 00 00 00`) is appended.
>   - **Mask rule:** `WaitCompletion = 242` takes a non-empty subset of `NET_RX | TIMER | IPC` (`source::is_wait_mask_supported`); `TIMER` still needs a finite timeout. Arguments, the return convention and allowlist bit 42 are unchanged.
>   - **Record:** each successful return writes one v1 24-byte record with exactly one source.
>   - **IPC records:** each terminal operation the caller owns produces at most one IPC record, scoped to `(cell, generation)` and the owner/consumer thread. The record's `result` is the operation token; the terminal kind and bytes remain behind `IpcTake`. A result already reported, or already taken, is never reported again. A record the kernel fails to copy out is re-armed and reported by the next wait.
>   - **Rotation:** ready sources rotate `NET_RX → IPC → TIMER`.
>   - **Mailbox interruption:** a queued raw message interrupts only a mask that names `NET_RX`.
>   - **Reply exception:** a `Send` from the provider currently serving one of the waiter's operations, addressed to that waiter, is not a raw message. It is that operation's reply (the Spec 20 §2.6 immediate-reply bridge), so it produces at most that operation's IPC record and no mailbox interruption.
>   - **Unchanged:** a lone `NET_RX` or `TIMER` wait behaves exactly as before, and `IpcSubmit`…`IpcReply`, `IPC_STATUS`, `WaitForEvent` and `events` are unchanged.
>   - **Allowlist symbol:** `declare_syscalls!` (and `ostd::app_entry!`/`service_entry!`) export the Cell's allowlist static as `__vicell_syscall_allowlist`; the `__ViCell_syscalls` bytes are unchanged. The SDK reads it to use `IpcWait` when the Cell lacks bit 42, so it never issues a denied syscall.
>   - No frame, byte-0 value, envelope, attested tail, buffer size or reply discipline changes.
>   - **Evidence:** `docs/evidence/c2c-multi-source-wait-x86.{txt,log}` at the x86 QEMU/TCG ceiling (NET_RX by host test only; two-CPU lane unforced).

## 8. Law-1 confirmation 2 (question form)

> *Do you approve the implemented delta of `libs/api/src/abi/completion.rs` (`45656c0b2225a38a…`) and `libs/api/src/abi/syscall.rs` (`d0388f9e995da1b0…`: documentation plus the `__vicell_syscall_allowlist` export), with the kernel, SDK, test and lane changes and the evidence in this package, closure amendments A1–A5 and deviations D9–D17? On approval, the Spec 17/20/03b edits, `CHANGELOG.md` and `scripts/check-waitcompletion-ipc-law1.sh` land in the same change and the surface is FROZEN. YES / NO.*
>
> On NO: revert the closure (no persistent state). A1–A5 are reverted on any rejection of the amendment.

## 9. Review findings → resolution

| Finding | Resolution | Evidence |
|---|---|---|
| HIGH: probe logs `denied` in Cells without bit 42 | A4: the path comes from the compiled allowlist; the probe and the TIMER retry are removed | lane and serial logs show 0 `denied for tid`; the lifecycle test asserts it; `igb-x86` 2/2 on rebuilt cells; ostd test `wait_path_follows_the_compiled_allowlist_without_probing` |
| MED: wait path allocates | A5: pre-sized waiter table, kernel-charged queue creation | M2: `wait_path_allocations=0`, `queue_creation_allocations=2`; `the_waiter_table_is_preallocated…` |
| MED: settlement untested | `settle_multi` split out; 5 settlement/abandon host tests, one using a second thread for `RetryCompleting` | kernel 392 |
| LOW: hard-coded counters | computed `lost`/`dup`/`foreign`/`stale_records`/`other_records`/`netrx_records`, asserted by the lane | evidence `.txt` |
| LOW: production self-test touches the global NET_RX flag | NET_RX readiness kept local | boot self-test PASS on both topologies |
| LOW: token lost on copy failure | `restore_notification` re-arms the token; NET_RX is re-levelled | `a_notification_whose_record_was_not_delivered_is_re_armed_once` |
| LOW: D12 missing from the spec | reply exception added to the §7 draft | §7 |
| LOW: D8 concurrent first-wait misclassification | removed together with the heuristic (A4) | n/a |
