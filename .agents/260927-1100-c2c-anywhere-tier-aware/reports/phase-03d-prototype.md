# Phase 03 slice d — P0 baseline, P1 kernel-private prototype, P2 measurement (go/no-go)

Bound to the [CP1](../phase-03d-multi-source-wait-cp1.md) §8 order and its [approval](../phase-03d-multi-source-wait-cp1-approval.md). Recorded 2026-11-02 at HEAD `0effc2250688cc351d64dd2733fcef66f9941939`; the changes are uncommitted. **P3 (public ABI) has not started**: `libs/api`, `libs/ostd` and `CHANGELOG.md` are untouched.

## Verdict: **GO for P3**, with one carried item

All P2 thresholds that P1 can measure are met. **M5 (terminal → running latency against the `IpcWait` p99) cannot be measured in P1**, because no userspace path can name the IPC bit yet. It is carried into P3 as a CP2 acceptance item rather than waived.

| Threshold (CP1 §8) | Measured | Result |
|---|---|---|
| M1 selection scan ≤ 64 records per wake | max 64 with 64 operations retained (`P03D-M1 max_scanned=64`); max 64 across the 10,000-return run | met |
| M2 no heap allocation on the wait path | 0 allocations across one publish gate plus 192 selections with 64 terminals (thread-local counting allocator); `Operation` grew 144 → 152 bytes (+8, one alignment unit); `Task` gains a 1-byte rotor | met |
| M3 the wake happens in the same `SCHEDULER` hold as the terminal | a parked IPC-mask waiter is `Ready` immediately after `reply_in_sched` and after `peer_died` returns (rows R2, R3) | met |
| M4 NET_RX-only/TIMER-only behaviour and existing suites unchanged | see §2: every pre-existing test passes before and after; the x86 boot self-tests pass on 1 and 2 CPUs | met |
| M5 x86 terminal→running latency ≤ `IpcWait` p99 + 1 tick | not measurable before P3 | **carried to P3/CP2** |
| A8 fairness model: ≤ 192 returns from arm to record; ≤ 63 overtakes | 10,000 returns with NET_RX and TIMER always ready and peer A answering newest-first: `max_returns=105`, `max_overtakes=34`, NET_RX/TIMER max gap 3, every armed terminal reported | met |
| E3 retained-terminal spurious wakes | 8 retained results, 1 pending: the `IpcWait` gate fires 1000/1000, the one-shot gate 0/1000 | improvement confirmed in model |

## 1. P0 baseline (before any source edit)

- **Closure pre-edit SHA-256**, taken in full and kept outside the repository during the run. The first 16 hex characters match CP1 §7.1. The rows touched by P1:

  | File | SHA-256 before (first 16) |
  |---|---|
  | `completion_wait.rs` | `32cb615aae106a9d` |
  | `async_ipc.rs` | `2d6fe1751e36dfad` |
  | `tcb.rs` | `bd59302493e33954` |
  | `task.rs` | `12583fbbfdd54dca` |
  | `scheduler.rs` | `a8c9365ec3975d42` |
  | `waker.rs` | `65e4b2064b77ab00` |
  | `ipc_pending_selftest.rs` | `abc3a4ca4ff58b37` |

- **Host suites.** `RUSTFLAGS` and `CARGO_TARGET_DIR` were unset.

  | Suite | Result |
  |---|---|
  | `cargo test -p api --target x86_64-unknown-linux-gnu` | 181 + 2 pass, 4 doc-tests ignored |
  | `-p ostd` | 84 + 7 + 19 doc-tests pass, 2 ignored |
  | `-p cellos-kernel --target x86_64-unknown-linux-gnu` | **377 pass**, including 10 `async_ipc` tests |

- **Lane.** The lifecycle ISO was rebuilt from HEAD (`scripts/build-x86_64-c2c-lifecycle-ci.sh`, 136 s, ISO `5bcb9b2e7abd1f5a…`). The lane is `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test local-service-lifecycle-x86`: **2/2 pass** (41.1 s).

## 2. P1 prototype (kernel-private; the syscall still refuses the IPC bit)

### What changed

Only §7.1 rows 3-9 changed (`git diff --stat`: 7 files, +820/−7):

| File | Change |
|---|---|
| `kernel/src/task/completion_wait.rs` | kernel-private `PROTO_IPC = 1 << 2`; `SELECTION_ORDER`, `INITIAL_ROTOR`, `proto_mask_is_valid`, `select_source`, `Selection`, `select_locked`; `publish_wait_state_locked` gains `IpcPending` and bit-tests NET_RX; the syscall loop handles `IpcPending` (unreachable for admitted masks) |
| `kernel/src/task/async_ipc.rs` | `Operation.notified`; `has_pending_notification` and `take_notification` (lowest token first, scope `(cell, generation)` plus consumer/owner tid); `wake()` also readies a `WaitCompletion` whose mask names IPC; boot seam `selftest_accept_dispatched`; 8 host tests |
| `kernel/src/task/tcb.rs` | `completion_rotor: u8`; `WaitCompletion` doc |
| `kernel/src/task.rs` | granted line only: `source & NET_RX != 0` |
| `kernel/src/task/scheduler.rs` | granted line only: `timed_out = source & TIMER == 0` |
| `kernel/src/task/waker.rs` | `relevel_net_rx()` |
| `kernel/src/task/ipc_pending_selftest.rs` | boot self-test `multi_source_wait_prototype` |

For every single-bit mask that `source_is_valid` admits, the two predicate edits evaluate exactly as before.

### Post-edit SHA-256 (first 16)

| File | SHA-256 after |
|---|---|
| `completion_wait.rs` | `81b79ca60c9a92e1` |
| `async_ipc.rs` | `cc4d4cdb69e082cb` |
| `tcb.rs` | `435111518525a9b9` |
| `task.rs` | `e78bbb3413cba46b` |
| `scheduler.rs` | `8267292a2c5d4d3b` |
| `waker.rs` | `91a429c2e81f9b29` |
| `ipc_pending_selftest.rs` | `5516f591a950cdce` |

### Pins

Every other closure file and pinned file has its pre-edit hash: `libs/api/**`, `libs/ostd/**`, `kernel/src/task/{syscall,completion,smp}.rs`, `executor.rs`, `net`, `net-broker`, `Cargo.lock`, the specs and `CHANGELOG.md`.

### Syscall refusal

The boot self-test asserts all three of these:
- `source_is_valid(PROTO_IPC)` is false;
- `source_is_valid(NET_RX|IPC|TIMER)` is false;
- `proto_mask_is_valid` accepts the mixed mask with a deadline and refuses it without one.

### Tests added and passing

**Kernel host: 377 → 385**, with all 8 new tests passing and 0 warnings:

| Test | Covers |
|---|---|
| `ipc_notification_is_one_shot_retained_and_cleared_by_take` | one-shot notification; duplicate reply; take before wait; `IpcWait` level view unchanged |
| `ipc_notifications_follow_submission_order_with_a_bounded_scan` | ordering + M1 |
| `source_rotation_reaches_every_ready_source_within_three_selections` | all rotor/ready combinations, 300-round rotation |
| `sustained_multi_producer_selection_meets_the_stated_bound` | A8 |
| `stale_and_late_replies_never_satisfy_another_operation_or_generation` | A5: expiry then a late reply; replacement and foreign-cell providers; owner generation change; misplaced foreign record |
| `two_hart_model_rows_for_the_ipc_source` | R1-R8, below |
| `retained_terminals_rewake_ipc_wait_but_not_a_one_shot_waiter` | E3 |
| `wait_path_selection_does_not_allocate` | M2 |

Rows R1-R8 in `two_hart_model_rows_for_the_ipc_source`:
- R1: pre-park publication gives `IpcPending`.
- R2: a parked IPC waiter is woken; a NET_RX-only waiter is not.
- R3: peer death against a parked waiter.
- R4: expire versus reply, in both orders.
- R5: take beats notification.
- R6: NET_RX and IPC at the same instant, including re-levelling NET_RX.
- R7: the deadline is not carried over.
- R8: a raw mailbox message interrupts only masks that name NET_RX and never consumes an IPC notification.

The existing `deterministic_two_hart_publication_and_wake_races` still passes.

### Other suites after P1

| Check | Result |
|---|---|
| `-p api` | 181 + 2 (unchanged) |
| `-p ostd` | 84 + 7 + 19 (unchanged) |
| `cargo check -p cellos-kernel` for rv64 (default), `aarch64-unknown-none-softfloat` and `x86_64-unknown-none` | clean, no warnings |
| Lifecycle ISO rebuilt (120 s, `ee6edbf38e457d2c…`); `local-service-lifecycle-x86` lane | **2/2 pass** (45.0 s) |
| Independent x86 boots of that ISO, `-smp 1` and `-smp 2` | `[selftest] IPC-PENDING: PASS` and `ipc-pending self-test PASS` (this includes `multi_source_wait_prototype`); `completion-queue` and `net-rx-reservation` self-tests PASS; topology `online=1` and `online=2 processor_ids=0;1`; shell prompt reached; 0 `FAIL` lines |

## 3. Deviations from the packet

1. **Explicit host target needed.** `-p api`, `-p ostd` and the integration crate need `--target x86_64-unknown-linux-gnu`, because the workspace default target is `riscv64gc-unknown-none-elf` (`.cargo/config.toml:17`). The first P0 attempts without it failed to compile; they were not test failures.
2. **P1 stops at components; the full loop moves to P3.** P1 delivers the components plus the park gate. It does **not** wire a multi-source loop into the `wait_completion` syscall body (NET_RX ownership resolved outside `SCHEDULER`, `relevel_net_rx`, writing the IPC record). That wiring moves to P3 together with the predicate flip, so P3 is larger than "flip the predicate" (CP1 §8).
3. **New boot seam outside the CP1 text.** `async_ipc::selftest_accept_dispatched` is a new `pub(super)` seam that seeds one dispatched operation without a user copy. It lives in a closure file but is not named in CP1.
4. **Test-only global allocator.** `async_ipc::tests::alloc_probe` installs a counting `#[global_allocator]` for the kernel host-test binary only. It forwards every call to `System`.
5. **M5 carried to P3** (see the verdict).
6. **No x86 SMP evidence for the new path yet.** A6 is satisfied only by the deterministic host model. The x86 `-smp 2` boot proves topology plus the self-test, but `ipc_pending_selftest` runs before secondary harts start (`ipc_pending_selftest.rs:3`). It is therefore not an SMP witness; the unforced `-smp 2` userspace lane is a P3 item.
7. **No x86 NET_RX producer, by design (D-8).** NET_RX in the boot self-test is injected through the level flag.

## 4. P3 entry list (not started)

- `libs/api` rows 1-2 (Law 1): `source::IPC`, `is_wait_mask_supported`, decode.
- Switch `source_is_valid` to the public mask rule, and wire the multi-source loop into `wait_completion`.
- `libs/ostd` `wait_any`, and the D-4 cutover of `wait`/`wait_and_take`/`exchange`.
- Bench legs A1-A8 and the `-smp 2` lane: M5 measured against the `IpcWait` p99 baseline.
- Then the docs, `CHANGELOG.md`, the digest script and the CP2 record.
