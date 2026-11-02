# C2C Phase 03 slice d CP1: multi-source `WaitCompletion` with an IPC source

Status: **draft for Law-1 confirmation 1 and the CELL-CONTRACT-0024 lock handoff (section 7). Documentation only.** This file edits no source, lock, plan, spec or changelog, and it claims nothing is implemented. Written 2026-11-02 against HEAD `0effc2250688cc351d64dd2733fcef66f9941939`. `git status --short` shows only ` M .agents/TODO.md` and `?? .agents/archive/`; no source file is dirty. Inputs (SHA-256, first 16 hex characters): `plan.md` `a9d025aa30f6bc54`, `phase-03-async-ipc.md` `e656359d5f401f66`, `.agents/cell-contract-0024-locks.md` `98cf3042ede5b1ef`, ADR-0023 `138bed58e53bb305`, Spec 17 `9a228cfd6ce3f452`, Spec 20 `9ef8ba8d809aeea7`, K3 precedent `k3-cp1.md` `f3141cf41445921d`. Digest form: SHA-256 of the whole file, no normalization, never stored inside the hashed file (the K2/K3 form).

Claim hygiene: every `file:line` was read in this session. `[INFERENCE]` means not verified by running or by opening the exact code. `OPEN (D-n)` means a decision left to the owner (section 12). Section 13 lists what was not checked.

## 1. Scope

**In:** one new source bit `source::IPC` on the existing `WaitCompletion = 242`. A wait may name several sources (`NET_RX | IPC | TIMER`) and gets one record per return. The IPC record identifies one terminal operation of the waiter. The slice also covers the kernel wake, selection and notification logic, the SDK helper, host tests, x86 QEMU lane witnesses, docs, and a non-ABI kernel prototype that is measured before any `libs/api` edit (section 8).
**Out:** see section 11. **Serves:** Intel C2C Phase 03, success criterion 3 and the open item "multi-source completion waiting" (`phase-03-async-ipc.md:78,106,162,320`; Spec 20 `:101,171-173`).

## 2. Problem and evidence (what is polled or threaded today)

| # | Fact | Evidence |
|---|---|---|
| E1 | `WaitCompletion` accepts exactly one of `NET_RX`/`TIMER`. Multi-bit or unknown masks fail with `InvalidInput`. | `libs/api/src/abi/completion.rs:47-63`; `kernel/src/task/completion_wait.rs:77-80,175-178`; `libs/api/src/abi/syscall.rs:1417-1422` |
| E2 | An IPC terminal wakes only a task in `WaitIpc`. A task in `WaitCompletion` is not woken by its own reply, peer death, expiry or cancel. | `kernel/src/task/async_ipc.rs:160-168`, called from `:396,414,565,752,813,864` |
| E3 | `IpcWait` is level-triggered on **any** retained terminal (`has_terminal` = any). Callers that keep a result untaken therefore return at once, and the SDK yields in a loop. | `async_ipc.rs:139-143,715-737`; `libs/ostd/src/ipc.rs:260-270` (`wait_and_take`), `:379-393` (`exchange`, every sync SDK RPC) |
| E4 | Raw IPC interrupts only a `NET_RX` wait, and the wait returns 0 with no record. The net cell treats that as "retry" and parks again with a 10-tick maintenance deadline. | `kernel/src/task.rs:340-356`; `completion_wait.rs:93-106`; `cells/services/net/src/service-runtime.rs:29,869-918` |
| E5 | No x86 code produces `NET_RX`. `signal_net_rx` is called only from `vi_handle_virtio_irq` (`kernel/src/task/drivers/virtio_common.rs:155-176`), which is reached only from the RV64 PLIC (`drivers/irq_dispatch.rs:36-69`) and AArch64 virt (`hal/arch/arm/src/aarch64/backend/qemu_virt.rs:92-96`). On x86, a `NET_RX` park therefore ends only on IPC interruption or on the deadline. | grep of `signal_net_rx(` and `vi_handle_virtio_irq` callers (this session) |
| E6 | The broker runs three spinning roles: `LocalWorker`, `ReplyPump` and `NetworkPoller`. Each loops on `yield_now`. Beacon IPC uses `sys_post` plus `sys_recv_timeout` in 10-tick slices, not the operation primitive. | `cells/services/net-broker/src/local_runtime.rs:186-258,309-316`; `runtime_roles.rs:5,8,23-28`; `beacon.rs:226-285` |
| E7 | Other timer-granularity IPC polls: httpd `ipc::wait(IDLE_TICKS = 2)`, the init and bench one-tick waits, and the executor parking on `TIMER`. | `cells/services/httpd/src/reactor.rs:27,481,664`; `cells/init/src/activation.rs:494`; `libs/ostd/src/executor.rs:21-24,57-58` |
| E8 | The current "two-hart" proof is a sequential host model under one lock. It does not cover `WaitCompletion`. | `async_ipc.rs:1693-1807` |

## 3. Existing mechanics this design reuses (no change of meaning)

- **Operation identity:** each operation stores owner `(cell, generation, tid)`, consumer tid and peer `(tid, cell, generation)` (`async_ipc.rs:37-61`). Tokens come from a global monotonic counter and are never slot indexes (`:76-77,170-176`). The limit is 64 per owner task; when full, submit returns `Busy` (`:14,106-109,246-248`).
- **Exactly one terminal:** `terminal()` requires `Dispatched`, exact owner generation and peer cell generation (`:373-380`). Cancel, expiry, peer death and cutover skip operations that are already terminal (`:554,836,126,797`). `take` removes the operation (`:619-625`). Owner exit makes its operations terminal (`:665-713`).
- **`transfer_consumption` has no caller** (`:628`; the only grep hit), so in practice `consumer_tid == owner_tid`.
- **The wait reserves a slot.** Every wait reserves a queue slot, publishes its state under `SCHEDULER`, and releases the slot on every path (`completion_wait.rs:168-300`). The `TIMER` record is built after resume (`:271-290`). Queue capacity is 32 per cell and there are at most 32 waiters (`completion.rs:45-47`).
- **NET_RX is one global reservation.** A new reservation displaces the old one, which receives `RESULT_ABANDONED`. The `NET_RX_WAIT` lock is a leaf and is never taken while `SCHEDULER` is held (`waker.rs:18-30`). A level flag records a frame that arrived with no waiter (`:39-46,89-95`).

## 4. Design options

| | **(i) `source::IPC` bit on `WaitCompletion` 242 (recommended)** | (ii-a) new opcode, e.g. `WaitAny = 263` | (ii-b) reuse `IpcWait = 258` with `a2` flags |
|---|---|---|---|
| Opcode / allowlist | none new; bit 42 stays shared (`syscall.rs:1136-1143`) | new variant, `From` arm and `CASES`; must share a bit because bits 0-62 are full (`syscall.rs:508-510`) | none new |
| Record | existing 24-byte v1 record; `source = 4`, `result = token` | new record or a copy of v1 | 258 writes no record (`syscall.rs:798-801`); one would have to be added |
| Compat hazard | old kernel refuses the new mask with `InvalidInput` (fail-closed); old decoders reject source 4, but only callers that set the bit get such a record | none for old binaries | the kernel decode **ignores** `a2/a3` (`kernel/src/task/syscall.rs:10030-10032`), so the field is unvalidated and giving it meaning changes a frozen surface |
| Mechanism reuse | one reservation/park/release path, `NET_RX` lost-wake ordering, TIMER deadline sweep | duplicates the wait machinery or calls into it | mixes `NET_RX` into an IPC-only call |
| Precedent | follows `GetProcs2`/`LookupServiceBound` only when a new opcode is needed; here the existing source word was designed for this (`completion.rs:49-53`, 03b ADR `:85-90,173-175`) | append-only opcode precedent (ADR-0023 §2.3) | none |

**Recommendation: (i).** It is append-only with no new opcode. It is sound because the source word already exists for "one bit per record". The record's v1 invariant "a peer-dependent source must bind the target generation at submission" (`completion.rs:22-24`) is already met by submit (`async_ipc.rs:203-204,276-282`). [OPEN D-1]

## 5. Pinned semantics (option i)

**5.1 Mask.** A valid mask is non-zero and ⊆ `{NET_RX, TIMER, IPC}`. `TIMER` requires a finite timeout. A zero mask or unknown bits return `InvalidInput`. This is the same validation point as today (`completion_wait.rs:77-80`).

**5.2 Ownership.** The IPC source of waiter task `W` covers exactly the operations in `W.async_operations` that satisfy `can_consume(W)` (`async_ipc.rs:58-60`). The binding is `(W.cell_id, W.cell_generation)` equal to the operation's owner `(cell, generation)` (`:54-56`), together with the operation id (token). No caller-supplied id, peer or generation is accepted. Operations of other threads, Cells or generations are never reported. Because `transfer_consumption` has no caller, "consumer" equals the owner thread in v1.

**5.3 Notification model: one latched notification per operation.** Each operation gains a kernel-only `notified: bool`, set to `false` at submit.
- The terminal transition (exactly one per operation, section 3) arms the notification.
- A `WaitCompletion` with `IPC` in its mask returns **one** record for one armed, un-notified operation, then sets `notified = true`.
- The notification is a level condition until it is delivered, so a terminal reached while nobody waits is not lost. After delivery it is consumed: a retained terminal is **never reported twice**, which removes E3's spin.
- `take` removes the operation, and with it any pending notification. A result taken before any wait yields no record.
- After a notification the operation remains retained, charged to the 64 quota, and readable only by `IpcTake(token)` until `take` or owner exit.
- `IpcWait`/`IpcTake` semantics and `IPC_STATUS` bytes do not change. `notified` does not affect them.

**5.4 Interaction with `has_terminal` and pre-park publication.** `publish_wait_state_locked` (`completion_wait.rs:93-106`) gains a third outcome. When the mask contains IPC, it checks "any operation that is terminal and not yet notified" under the same `SCHEDULER` hold that writes `TaskState::WaitCompletion`, and returns *IpcPending* instead of parking. All terminal transitions happen under `SCHEDULER` (`async_ipc.rs:1-2`). `wake()` is extended to make a `WaitCompletion` task with `IPC` in its mask `Ready` (same `push_ready`/preempt path, `:160-168`). The two cases are:
- a terminal published **before** state publication is caught by the check;
- a terminal published **after** it finds the state and wakes it.

There is no third interleaving. `has_terminal` itself is unchanged and still serves `IpcWait`.

**5.5 Selection and fairness.** One record is returned per call, in this order:
- **Across sources:** a per-task 1-byte rotor (new `Task` field) rotates `NET_RX → IPC → TIMER` and starts after the last source that was delivered. A source that is ready but not chosen keeps its state.
  - IPC stays latched.
  - `NET_RX`: a reservation that completed but lost the selection is converted back to the level flag (`NET_RX_PENDING = true`) before the call returns. The cost is at worst one spurious wake, never a lost frame.
  - `TIMER` means "this call's deadline elapsed". It is not carried across calls.
- **Within IPC:** the lowest-token operation that is armed and un-notified is chosen first, which is the oldest submission (the vector is in push order and tokens are monotonic).
- **Bound:** an armed operation is reported after at most 63 older ones, and new submissions never overtake it. A ready source is chosen within ≤ 3 successful returns. An armed IPC terminal is therefore reported within **≤ 3 × 64 = 192** successful returns of a waiter that keeps IPC in its mask.
- **Peers:** a flooding peer cannot overtake an older operation of another peer.
- **Provider mailbox order is unchanged and is not a fairness claim here.**

**5.6 Bounded memory.**
- No per-wait allocation: the wait already reserves one fixed slot out of 32 (`completion.rs:173-192`). The existing `register_waiter` vector is bounded at 32 (`:356-362`) and is not changed.
- Added state: `notified` (1 byte, stored inside the existing `Operation` that submit already allocates, `async_ipc.rs:269-288`) and the 1-byte rotor per task.
- The selection scan is ≤ 64 entries under `SCHEDULER`, the same order as the `expire` sweep (`:828`).

**5.7 Stale or late replies (acceptance item a).**
- A record names a token, and only the terminal transition of that exact operation can arm it.
- Late or duplicate replies cannot reach a terminal operation (`:377`). Wrong-generation providers are refused (`:373-376`). A replacement provider cannot settle an old token (`cutover_peer` `:784-816`, evidence in the restart slice).
- Tokens are never reused (`:76-77`), so a record held by user code after `take` can only produce `IpcTake → Invalid`. It can never refer to a different operation or generation.
- The waiter check (5.2) prevents reporting any token outside the waiter's own `(cell, generation)`.

**5.8 Existing behaviour kept.**
- Raw-mailbox interruption applies **only when `NET_RX` ∈ mask**: same return 0 with no record (`task.rs:343-347` becomes a bit test). It never consumes an IPC notification.
- `NET_RX` lock order: the `NET_RX` reservation is resolved outside `SCHEDULER` first, then IPC is selected under `SCHEDULER` (`waker.rs:26-30`).
- Deadline sweep: `timed_out = (mask & TIMER) == 0` (`scheduler.rs:2303-2309`). A mask without `TIMER` behaves like today's `NET_RX` case, including the receive-context release at `:2324-2349`. A mask with `TIMER` behaves like today's `TIMER`. NET_RX-only and TIMER-only calls are byte-identical.

## 6. Public ABI delta (Spec 17 §9 amendment form)

> **2026-xx-xx — Recorded for a changed syscall surface; no ratified wire rule moved (Law-1 2× confirmed, `phase-03d-multi-source-wait-cp1-approval.md` + CP2 record).** `api::completion::source::IPC: u32 = 1 << 2` (value `4`, bytes `04 00 00 00`) is appended. Existing bits stay at `NET_RX = 1`, `TIMER = 2`, and `UNSPECIFIED = 0` still decodes only as the legacy value.
> - **Mask rule:** `WaitCompletion = 242` (unchanged `a0` = mask, `a1/a2` = timeout ticks, `a3` = record pointer; allowlist bit 42 shared with `WaitForEvent`) widens its mask from "exactly one of {NET_RX, TIMER}" to "non-zero subset of {NET_RX, TIMER, IPC}; TIMER needs a finite timeout".
> - **Return:** each successful return still writes one 24-byte v1 record (`COMPLETION_LEN = 24`, `COMPLETION_VERSION = 1`, `COMPLETION_MAGIC = "VICQ"`) with **exactly one** source bit.
> - **IPC record:** bytes `8..12` = the wait's reservation slot index (as for TIMER), `12..16` = `04 00 00 00`, `16..24` = operation token as positive little-endian `i64` (tokens ≤ `isize::MAX`, `async_ipc.rs:170-176`).
> - The terminal kind and bytes are obtainable only through `IpcTake(token)`. A record is a one-shot notification, never consumption.
> - **New `const fn source::is_wait_mask_supported(mask)`.** `is_single_supported` now also accepts `IPC`, and `ViCompletion::from_bytes` accepts a source-4 record. Multi-bit source words are still rejected.
> - **`api::syscall::events` is unchanged.** `IPC` is not an event bit, and `WaitForEvent` semantics are unchanged.
> - No frame, byte-0 value, envelope, attested tail, buffer size, `IPC_STATUS` layout or reply discipline changes.

**Byte-compatible (unchanged):**
- Opcode 242 number, arguments, return convention (1 = record, 0 = no record, errors) and allowlist bit 42.
- Record length, version, magic and field offsets.
- The `NET_RX`-only and `TIMER`-only behaviour, including mailbox interruption, displacement with `RESULT_ABANDONED`, and deadline semantics.
- `IpcSubmit`/`IpcTake`/`IpcWait`/`IpcCancel`/`IpcCurrent`/`IpcReply` 256-261 and `IPC_STATUS` (`syscall.rs:789-828`).
- `WaitForEvent` and `events`. No existing cell needs re-signing.

**Not compatible by design:**
- A mask that the old kernel refused is now accepted.
- A new binary running on an old kernel gets `InvalidInput`, which is fail-closed. Images ship kernel and userspace together, and the SDK must surface the error, not spin.
- An old decoder rejects a source-4 record, but it never asks for one.

## 7. File-owner handoff (DRAFT for the accountable owner; not submitted)

**7.1 Requested closure** (pre-edit SHA-256 at HEAD `0effc225`, first 16 hex characters; the full digests are re-taken at baseline). **KR** marks an overlap with active kernel-repair ownership: Phase 07 is in progress and lists `task/scheduler.rs`, `task/smp.rs` and `task/syscall.rs`, plus the `vi_timer_tick` hook in `task.rs` (`260927-0739-…/phase-07-warm-snapshot.md:17,163-165`; `plan.md:49,72`).

| # | Path | Purpose | SHA-256 before | Surface |
|---|---|---|---|---|
| 1 | `libs/api/src/abi/completion.rs` | `IPC`, mask fn, decode, tests | `84f4c1f45346c9c2` | **Law 1** |
| 2 | `libs/api/src/abi/syscall.rs` | doc only: `:725-749`, `:1417-1422` | `05ffaa3c85d348fa` | **Law 1** |
| 3 | `kernel/src/task/completion_wait.rs` | mask check, selection, IpcPending, record | `32cb615aae106a9d` | completion |
| 4 | `kernel/src/task/async_ipc.rs` | `notified`, `wake()` extension, selector, host tests | `2d6fe1751e36dfad` | async IPC |
| 5 | `kernel/src/task/tcb.rs` | rotor field, `WaitCompletion` doc | `bd59302493e33954` | TCB (NET-OD touched it; landed) |
| 6 | `kernel/src/task.rs` | one predicate `:343-347` (bit test) | `12583fbbfdd54dca` | **KR** (scheduler/IPC core) |
| 7 | `kernel/src/task/scheduler.rs` | one predicate `:2308` (bit test) | `a8c9365ec3975d42` | **KR** (Phase 07 quiescence, SMP) |
| 8 | `kernel/src/task/waker.rs` | `NET_RX` re-level helper | `65e4b2064b77ab00` | completion |
| 9 | `kernel/src/task/ipc_pending_selftest.rs` | boot self-test cases (A6, A7) | `abc3a4ca4ff58b37` | tests |
| 10 | `libs/ostd/src/syscall.rs` | wrapper docs and decode tests | `a1ef99c824bb8cda` | SDK |
| 11 | `libs/ostd/src/ipc.rs` | `wait_any`; D-4 cutover of `wait_and_take`/`exchange` | `3f9da54323b45c91` | SDK |
| 12 | `cells/tests/bench/src/scenarios/async_lifecycle.rs` | multi-source legs | `7fbfd03497baaa04` | tests |
| 13 | `cells/tests/bench/src/bench-probe.rs` | role dispatch, only if new roles are needed | `e012d8812adcdacb` | tests |
| 14 | `tests/integration/src/lib.rs` | `pub fn boot_x86_bios_smp` (`boot_x86_q35` is private, `:1055`) | `753c8f935d214488` | tests |
| 15 | `tests/integration/tests/local-service-lifecycle-x86.rs` | markers, `-smp 2` test | `2746030f56f5119d` | tests |
| 16 | `scripts/check-waitcompletion-ipc-law1.sh` | **new at CP2**; frozen-item check (LookupServiceBound form) | new | script |
| 17 | `docs/specs/17-ipc-wire-contract.md` | §9 log entry (section 6) | `9a228cfd6ce3f452` | docs |
| 18 | `docs/specs/20-unified-ipc-contract.md` | §2.4 `:75`, §2.6 `:101,171-173`, §3 `:186`, §4 `:195` | `9ef8ba8d809aeea7` | docs |
| 19 | `docs/specs/03b-async-reactor-adr.md` | Point 1 `:85-90`, `:173-175` | `834496c3e291df2e` | docs |
| 20 | `CHANGELOG.md` | at CP2 | `cb8899ab1f65af21` | docs |
| 21 | `docs/evidence/c2c-multi-source-wait-x86.{txt,log}` | new | new | evidence |
| 22 | `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | progress | `e656359d5f401f66` | plan |
| 23 | `.agents/260927-1100-c2c-anywhere-tier-aware/reports/phase-03d-prototype.md` | new; section 8 report | new | plan |

That is 23 entries: 16 source/test/script and 7 doc/evidence/plan (24 files, counting the evidence pair). **Pinned byte-identical:**
- `kernel/src/task/syscall.rs` (`56bca7b804f06c83`), including decode `:10456-10471`, dispatch `:9681-9685` and `IpcWait` decode `:10030-10032`;
- `completion.rs` (`7a14b43f1a0cd169`) and `smp.rs` (`442edc8ac3838582`);
- `libs/ostd/src/executor.rs` (`311871c99f44f522`);
- `cells/services/net/**` (`service-runtime.rs` `7fe126d9017fb168`) and `cells/services/net-broker/**` (`local_runtime.rs` `ce4be532f28e0f29`);
- `Cargo.lock`.

**7.2 ADR-0024 delta (procedure step 2).**
- **Principal and incarnation:** the waiter scope is the Task's `(cell, generation)` plus its own operations. There is no new identity axis, and ADR-0023 §2.1 is reused.
- **CSpace and delegation:** no handle, no delegation, no attenuation change.
- **Revocation:** the notification dies with the operation, which dies with take or owner exit.
- **Kernel versus policy:** the kernel only notifies. Retry, idempotency and the meaning of a result stay in the caller (Spec 20 §2.6).
- **Durable state:** none.
- **Concurrency:** all transitions stay under `SCHEDULER`, and the `NET_RX` leaf-lock rule is kept.
- **Remote:** none.
- **No ambient authority:** the record grants nothing; `IpcTake` remains the authority check (`async_ipc.rs:586-594`).

**7.3 Lock rows and overlaps (read-only check at HEAD `0effc225`).**

| Row / owner | Relation | Asked of the owner |
|---|---|---|
| `260927-1100-c2c-anywhere-tier-aware` (`locks.md:173`) | direct: Phase 03 async scope | narrow for slice d only; the whole-plan relation stays |
| controlling P04 "correlated operations, wait/completion" (`locks.md:99`); P03 "SMP linearization" (`:98`) for A6 | controlling phases | name the P04/P03 owner or delegate |
| `260927-0739-kernel-architecture-repair` (`locks.md:172`; plan `:49,72`; phase 07 `:17`) | **KR overlap**: rows 6-7, and the `-smp 2` lane relies on its SMP bring-up | **blocker until ruled:** a file-bounded handoff for the two predicate lines, or wait for Phase 07 (C2C `plan.md:97`; Spec 20 stop line `:224`) |
| `261006-0934-httpd-single-cell-concurrency` (`locks.md:175`) | check: "async IPC, completion-capacity"; httpd is a consumer (E7) but no httpd file is edited | rule compatible |
| `260727-2101-midori-lessons-cellos` (`:156`), `260801-parallel-midori-closure` (`:158`) | check: generic reactor, completion/peer-death; no reactor or executor change (executor pinned) | rule compatible |
| `260615-1325-vicell-app-sdk-v1` (`:144`) | check: SDK lifecycle (`ipc.rs` row 11) | rule compatible, or drop D-4 |

**7.4 WIP rule.** One implementation slice at a time under a single integrating owner (`locks.md` procedure step 5; portfolio `:24,210`). The registered x86 board-selection phases 01-02 and the aarch64 split are recorded as done on 2026-11-02 (portfolio `:171-172`), and x86 phase 03 still needs its own handoff (`:176`). Slice d starts only when no other slice holds a closure path; this is re-checked at slice start, and the slice stops if one does. [INFERENCE: no other active slice; not verified beyond the portfolio text.]

**7.5 Retained gates.**
- Law 1: confirmation 1 on this text; confirmation 2 on the exact delta of rows 1-2 plus evidence. The kernel rows are lock-gated and bound in the same packet.
- No protected identity, time, physical or production gate is touched.
- A green test does not replace any of these gates.

**7.6 Decisions requested from the owner:** D-1..D-9 (section 12) and one scheduling record across `cell-contract-0024-locks.md`, `plan-portfolio.md` and `plan.md`. **Nothing here authorizes a source edit.**

## 8. Prototype before ABI (Phase 03 rule: `phase-03-async-ipc.md:20`, steps 1-2 `:32-33`)

Order: **CP1 approval → P0 baseline → P1 prototype → P2 measurement report (go/no-go) → P3 public ABI edit → CP2.** A no-go stops the slice before any `libs/api` byte changes, and this CP1 is amended.

- **P0 baseline (no edits):**
  - run the host tests: `-p api`, `-p ostd`, kernel x86 host, the `async_ipc` tests;
  - run both `local-service-lifecycle-x86` tests;
  - record the counts and the `IpcWait`-spurious-return count of a retained-terminal fixture (E3).
- **P1 non-ABI prototype:**
  - kernel only (rows 3-9);
  - the IPC bit is a **kernel-private** constant;
  - `source_is_valid` still calls the unchanged `api` predicate, so the syscall path keeps refusing mask 4 or multi-bit masks;
  - it is reachable only from host tests and `test-hooks` boot self-tests that drive `TaskState::WaitCompletion` directly (the `ipc_pending_selftest.rs:47-51` pattern);
  - no `libs/api`, `libs/ostd` or userspace change.
- **P2 measurement and thresholds:**
  - (M1) selection scan ≤ 64 entries per wake, counted in host tests;
  - (M2) zero heap allocation on the wait path under `cellos_p01_trace` owner accounting [INFERENCE: tracer usable on x86 host];
  - (M3) the wake is in the same `SCHEDULER` hold as the terminal, so an injected terminal readies the parked waiter on that hold, with no tick lag;
  - (M4) NET_RX-only/TIMER-only self-tests and `net_rx_selftest` unchanged;
  - (M5) x86 boot self-test terminal→running latency no worse than the `IpcWait` p99 baseline + 1 tick.
  Results go to row 23.
- **P3:** rows 1-2, `ostd`, bench and lane; flip the predicate. **CP2:** post-edit digests, diff stat, evidence and the deviation log (the `law1-lookupservicebound.md` §2.1-2.2 form).

## 9. Acceptance matrix (host + x86 QEMU; every row must hold, falsified by any listed counter ≠ expected)

| # | Case | Host test | x86 lane witness (lifecycle ISO, `bench async-lifecycle`) | Pass criterion |
|---|---|---|---|---|
| A1 | Reply | 8 dispatched → replies in reverse order | 8 outstanding; loop `WaitCompletion(IPC\|TIMER)` | exactly 8 IPC records; token set = submitted set; each once; each `IpcTake` = Reply with exact bytes; `spurious=0` (no return without a new record while terminals are retained) |
| A2 | Peer death | `peer_died` with 4 dispatched | provider dies mid-flight | 4 records, all `PEER_GONE` by take; `unterminal=0 dup=0`; waiter wakes ≤ 1 tick after death on single hart |
| A3 | Deadline | `expire` before/after dispatch; wait with no terminal | real 3000-tick expiry, no shortened timeout | queued → one record + `PreDispatchTimeout`; dispatched → one record + `Indeterminate`; IPC\|TIMER with nothing ready → TIMER record, `result=0`, elapsed ∈ [timeout, timeout+2] ticks |
| A4 | Cancel / abandon | cancel queued/dispatched; take before wait; owner exit | same legs | Cancelled / Indeterminate each notified once; take-before-wait → no record for that token; owner exit → no wake of a dead tid, queue `reserved()==0` |
| A5 | Stale / late (item a) | late reply after expire; replacement provider; foreign-cell waiter | replacement provider; late old reply | late or old-generation reply refused (`Invalid`); no second record for the old token; new operation gets only its own record; no record ever names a token outside the waiter's `(cell, gen)` |
| A6 | Two-hart publication/wake | deterministic model R1-R8: pre-park terminal; parked wake; peer death vs park; expire vs reply; take vs notify; NET_RX vs IPC same instant; deadline vs IPC; raw mailbox vs NET_RX\|IPC | `-smp 2` (`[x86-smp] topology online=2`, `scripts/x86/qemu-domain-test.sh:277`), caller and provider spawned on separate harts, 1000 rounds | all R1-R8 assertions; lane `lost=0 dup=0 hang=0` under watchdog. Ceiling: interleavings are not forced, and AP dispatch of Cell tasks is [INFERENCE] (`smp.rs:1004-1009`) |
| A7 | Mixed NET_RX + IPC + TIMER | all 6 arrival orders | x86 `test-hooks` boot self-test injects NET_RX via `begin/finish_signal_net_rx_for_test` (`waker.rs:70-76`), since x86 has no producer (E5); userspace leg IPC+TIMER | each source delivered exactly once within ≤ 3 calls; NET_RX never lost (record or level flag); mailbox interruption only with NET_RX in mask; no IPC notification consumed by a 0-return |
| A8 | Sustained multi-producer non-starvation | model: 64 ops, 2 peers, continuous A, slow B, TIMER | provider A floods, B slow, ≥ 10,000 records | max overtakes of any operation ≤ 63; max returns between arm and record ≤ **192**; TIMER record ≤ 3 returns after expiry; zero missing |
| A9 | Regression | api, ostd, kernel host suites; `net_rx_selftest`; `ipc_pending_selftest` | both existing lifecycle tests; executor TIMER park proof | counts ≥ P0 baseline; NET_RX-only/TIMER-only outputs unchanged; masks 0, `1<<31`, and TIMER with timeout 0 → `InvalidInput` |
| A10 | Bounded memory | size assertions | p01 owner trace on lane run [INFERENCE] | no allocation on the wait path; `Operation` growth ≤ 8 bytes; no queue slot outstanding after any test |
| A11 | ABI bytes | api tests | n/a | `IPC == 4`, distinct single bit; IPC record bytes `12..16 = 04 00 00 00`, `16..24` = token LE; `from_bytes` accepts 4 and rejects `4\|1`; `events` unchanged; the CP2 digest script passes and fails on a one-value mutation |

## 10. Risks, rollback, security

- **R1 Scheduler overlap (KR).** Two predicate lines sit in files that the active Phase 07 owns. *Mitigation:* a file-bounded handoff limited to those lines, plus a start-of-slice re-check.
- **R2 NET_RX re-level.** A record that loses selection becomes a spurious wake. Accepted: it is never a loss.
- **R3 Receive-context release on mask timeout.** A mask without TIMER follows today's NET_RX `timed_out` path (`scheduler.rs:2324-2349`). The A9 VFS lease regressions must stay green.
- **R4 Mixed callers.** A Cell that uses both `IpcWait` and `WaitCompletion(IPC)` sees level and one-shot views of the same operations. Documented; no state is shared beyond `notified`.
- **R5 x86 evidence ceiling.** NET_RX on x86 is test-hook-only (E5). Two-hart coverage comes from the deterministic model plus an unforced `-smp 2` lane. Physical Intel is not claimed.
- **R6 Prototype no-go.** The slice stops with only kernel-private code. That code is reverted at P2 rather than kept as dead code.
- **Rollback.** Before CP2, revert the closure as one change; there is no persistent state. After CP2 the surface is frozen and removal requires the ABI process. An image pairing a new SDK with an old kernel fails closed (`InvalidInput`).
- **Security.**
  - A record reveals only the waiter's own tokens (5.2) and confers no authority; `IpcTake` re-checks `can_consume`.
  - Wake amplification is bounded: one terminal per operation and ≤ 64 per task, so a peer can cause at most one wake per submitted operation.
  - The scan is bounded at ≤ 64 under `SCHEDULER`.
  - No new allowlist bit and no change to the cross-Cell identity path.
  - A malicious cell cannot make another cell's waiter return early, because terminals arm notifications only on their owner.
  - D-6 decides whether the IPC bit additionally requires the `IpcWait` allowlist bit 1.

## 11. Non-goals

- Remote route, relay, broker-to-broker RPC, ServerEpoch (Phase 04-06).
- Cap'n Proto or any promise pipelining.
- **Broker cutover** (E6): optional follow-up slice, D-5.
- A mailbox/`Recv`-ready source.
- `WaitForEvent`/`events` changes.
- An x86 e1000/igb `NET_RX` producer.
- Executor migration.
- Per-token waiting.
- `transfer_consumption` exposure.
- Provider-side or per-NodeId fairness.
- Cancellation side-effect semantics (unchanged: dispatched → `Indeterminate`).
- Tier-2/3, physical Intel, production.

## 12. Decisions for the owner

| ID | Decision | Proposal |
|---|---|---|
| D-1 | Option (i) IPC bit on 242 vs (ii-a) new opcode 263 | (i) |
| D-2 | IPC record `result` = operation token; kind only via `IpcTake` | yes |
| D-3 | Notification model: one-shot latched per operation (5.3) vs level on any retained terminal | one-shot latched |
| D-4 | Cut `PendingCall::wait_and_take` and `exchange` over to `WaitCompletion(IPC\|TIMER)` in this slice (`IpcWait` 258 stays as frozen ABI) | yes |
| D-5 | Broker `NetworkPoller` cutover: excluded vs optional follow-up after CP2 | optional follow-up, own CP1 |
| D-6 | IPC bit needs no allowlist bit beyond 42 vs also require bit 1 | bit 42 only |
| D-7 | Kernel-repair overlap: grant a file-bounded handoff for `task.rs:343-347` and `scheduler.rs:2303-2309` now vs wait for Phase 07 closure | grant, lines only |
| D-8 | x86 evidence ceiling: test-hook NET_RX injection (A7) + `-smp 2` unforced lane + deterministic host model (A6) | accept |
| D-9 | Adopt slice d as a bounded slice under P04 (P03 for A6) with the section-7 closure; one scheduling record | adopt |

## 13. Not checked

- No build, test or QEMU run at this HEAD; P0 counts are unknown.
- Whether the x86 AP dispatches Cell tasks in the lifecycle ISO, and which existing syscall reports a task's hart.
- `cellos_p01_trace` usability on x86.
- `sys_spawn_pinned`'s third argument as hart affinity [INFERENCE].
- Whether any out-of-tree consumer decodes completion records.
- Status files of the kernel-repair phases beyond `plan.md:44-51`.
- Callers of `WaitCompletion` outside `cells/`, `libs/ostd`.

## 14. Deviation log (appended after confirmation 1; supersedes nothing above)

This log is appended after the approval record bound this file at SHA-256 `092486f136f1319f2cddf2b4c226c09d7c44d9447b951aef039c434517e21ce3`. That bound digest refers to the bytes **before** this section; the CP2 record rebinds the file. The P0–P2 evidence is in [reports/phase-03d-prototype.md](reports/phase-03d-prototype.md).

| # | Deviation | Phase | Effect |
|---|---|---|---|
| D2 | P1 delivered the selection, notification and park-gate components but did not wire a multi-source loop into the `wait_completion` syscall body. The loop moves to P3 with the predicate flip, so P3 is larger than "flip the predicate" (§8). | P1→P3 | P3 scope only; no ABI change |
| D3 | New `pub(super)` boot seam `async_ipc::selftest_accept_dispatched`. It seeds one dispatched operation without a user copy for the boot self-test, and lives in closure row 4. §7.1 did not name it. | P1 | production code path used only by the boot self-test |
| D4 | A test-only counting `#[global_allocator]` (`async_ipc::tests::alloc_probe`) applies to the whole kernel host-test binary. Every call is forwarded to `System`; it exists to measure M2. | P1 | host tests only |
| D5 | M5 (terminal→running latency against the `IpcWait` p99) could not be measured in P1 because no userspace path existed. It was carried into P3/CP2. | P2→P3 | GO recorded with M5 carried |
| D6 | A6 is covered by the deterministic host model only. The x86 `-smp 2` boot proves topology and the self-test, but `ipc_pending_selftest` runs before the secondary harts start (`ipc_pending_selftest.rs:3`), so it is not an SMP witness. | P1 | the SMP lane witness is a P3 item |
| D7 | NET_RX in the boot self-test is injected through the level flag, because x86 has no NET_RX producer (accepted under D-8). | P1 | evidence label |
| D-digest | `phase-03-async-ipc.md` gained the slice-d progress section after the approval bound it at `e656359d5f401f66378d69ae391a12d48cfdea0723963370a2c683d7b07d02c8`. It is closure row 22, so the edit is permitted, but the bound digest refers to the pre-edit bytes (post-P2 `f8e23d0556bfd4e7…`). The CP2 record rebinds it. | P2 | provenance only |
| D1 | The P0 host commands need `--target x86_64-unknown-linux-gnu` for `api`, `ostd` and `tests/integration`, because the workspace default target is `riscv64gc-unknown-none-elf` (`.cargo/config.toml:17`). | P0 | command form only |
