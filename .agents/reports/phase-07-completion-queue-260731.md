# Phase 07 — kernel-side completion queue (infrastructure only)

Date: 2026-07-31 · Branch: `feat/wx-post-reloc-and-f1-signing` (not rebased, not amended)
ADR: `docs/specs/03b-async-reactor-adr.md` · Phase: `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md` requirements 3 and 4

## What landed

| File | Change |
|------|--------|
| `kernel/src/task/completion.rs` | New — the queue, slot reservation, append, deferred wake (196 code lines) |
| `kernel/src/task/completion_selftest.rs` | New — boot self-test, `bool`-returning (216 code lines) |
| `kernel/src/task/tcb.rs` | +9 — `Task.completion: Option<Arc<CompletionQueue>>`, `None` in `Task::new` |
| `kernel/src/task.rs` | +12 — two `pub mod`, deferred-wake drain in `yield_cpu` |
| `kernel/src/main.rs` | +5 — self-test call beside the other task self-tests |

Nothing else was touched. No syscall added or altered, no ABI change, no waiter/syscall/driver
migrated, `libs/ostd` and `block_on` untouched, receive path / non-blocking send / task exit
untouched. `kernel/src/memory/pin.rs`, `kernel/src/task/stack.rs`, `cells/services/vfs/` and the
directory-handle work were not opened for edit.

## Design, and why each part is where it is

**Ownership.** `Arc<CompletionQueue>` held by the task record — not a value inside `Task`, and not
a grant. Storing it by value was ruled out by the one-lock append rule rather than by taste: `Task`
lives in `sched.tasks` behind `SCHEDULER`, so a queue inside it is reachable only by taking the
scheduler lock, which is exactly what append must not do. A separately owned object whose handle
the completion source keeps means append resolves no address and consults no allocator. Not being
a grant is what makes it unfreeable: there is no `reg_id`, so `GrantFree`/`GrantUnregister` have
nothing to reject — that half of requirement 3 is satisfied by construction, not by a check.

**Per cell.** `completion::queue_for(sched, tid)` looks across the cell for an existing handle
before allocating one, so threads of a cell share a queue. Done at the single creation point rather
than propagated in `spawn`/`spawn_thread`, so the property does not depend on every future spawn
path remembering it — and `spawn` stayed untouched.

**Bounded.** 32 slots, fixed arrays, no caller-supplied count anywhere. The drainable ring holds
slot *indexes* and is exactly as long as the slot array, so it cannot overflow: a slot contributes
one entry per reservation. Append therefore never allocates and never grows.

**Reservation.** `reserve()` returns `Option<SlotId>` from the submitting context; `None` is the
backpressure. `complete(slot, result)` is infallible for a slot that was reserved and not yet
completed; it returns `bool` (`#[must_use]`) so a kernel-side protocol violation — completing a
free slot, or completing twice — is reported rather than silently overwriting.

**Deferred wake.** An append raises a per-queue flag plus one global gate and returns.
`task.rs::yield_cpu` calls `completion::deliver_pending_wakes` after the two existing reaps, where
`SCHEDULER` may be taken. Same shape as `pending_grant_reap` (record the need, act later outside
the producing context), different carrier — see Concerns.

## Verification

All commands run from `/home/dmin/cellos`.

| Command | Outcome |
|---|---|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | clean |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | clean |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | clean (codegen, not just check) |
| `cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api` | 61 + 2 passed, 0 failed (baseline) |
| `pwsh -NoProfile -File ./gen_disk.ps1` | image built (`gen_disk.ps1`, not the CI ramdisk script) |
| `cargo build --release -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | clean |
| `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel` | `PASS: shell prompt reached — full boot successful` |
| `… --test boot -- --test-threads=1` | **54 passed; 0 failed** (340.41 s) — baseline |
| `… --test hotswap-smoke -- --test-threads=1` | **11 passed; 0 failed** (13.02 s) — baseline |
| `… --test handoff -- --test-threads=1` | **26 passed; 0 failed** (3.45 s) — baseline |

Suites were run serially, each as its own command. No `SKIPPED` line in any log.

`cargo check` was proved to be really compiling the new code (my standing trap: a bin crate prints
`Compiling`, never `Checking`, and a fast exit-0 looks like a skip). A deliberate
`fn _probe() -> u32 { "x" }` appended to `completion.rs` produced `error[E0308]: mismatched types`;
it was removed immediately afterwards.

`vfs-quota` and `redoxfs-srv` were **not** run: they need their own kernel builds, which overwrite
the plain kernel binary the boot suite just validated. Neither touches the task record.

### The self-test, in the boot log

```
[ INFO] [selftest] COMPLETION-QUEUE: PASS (cap 32 slots, 624 bytes per cell)
[INFO] completion-queue self-test PASS (reserve, land, bound, defer)
```

Four rows, `bool`-returning throughout — no boot-time `assert!`, so a wrong expectation logs a
decisive line instead of panicking every boot:

1. **round trip** — reserve, complete, drain returns that slot with that result, a second drain
   returns nothing, and the slot is free again.
2. **shared within cell** — two threads of one cell get the same `Arc` (`Arc::ptr_eq`); a task in
   another cell does not. This is what makes the bound per cell rather than per thread.
3. **exhaustion refuses submission** — 32 reservations succeed, the 33rd is refused, and then every
   one of the 32 promised completions still lands and drains in submission order. The refusal costs
   no in-flight operation its landing place, which is the whole point of reserving at submission.
4. **deferred wake reaches the scheduler** — a task parked in `Sleeping{until: usize::MAX}` is
   registered as waiter. After `complete()` the ready-queue count is *unchanged* (proving the append
   did not wake inline) but the wake flag is raised; after `deliver_pending_wakes` the count is
   +1 and the task is `Ready`. The row holds `SCHEDULER` for its whole length, so no timer tick can
   run the real deferred wake and make a context-less synthetic task runnable behind the test.

Synthetic tasks use tids 9301–9303 and cells 9401–9402, are inserted into the task table but never
pushed onto a ready queue (`pick_next` pops only from the ready queue, so it cannot select one), and
are removed on every path including failure. `next_task_id` is not advanced — the test inserts
directly rather than spawning.

## Concerns

**Which locks the append path actually takes, and how that was established.** Exactly one:
`CompletionQueue::ring`, a `Spinlock` local to the queue object. Established by reading, function by
function, not by assuming:

- `complete()` (`kernel/src/task/completion.rs:159`) — the guard's whole scope is one block of
  array writes and integer arithmetic. It calls nothing.
- `Spinlock::lock` (`kernel/src/sync.rs:25`) takes no other lock: it calls
  `crate::hal::ARCH.interrupts_enabled()` and `disable_interrupts()`, which on rv64
  (`hal/arch/riscv/src/rv64.rs:60,70`) are bare `sstatus` CSR reads/writes, then CAS-spins on its own
  `AtomicBool`.
- After the guard drops, two atomic stores. No lock, no allocator, no page table, no scheduler.

One thing this surfaced: **the logger is not a leaf.** The first cut reported a protocol violation
with `log::error!` from *inside* the queue guard, which would have made the append path take the
UART lock as well and quietly created a `queue.ring → UART` ordering nobody had signed up for. Both
`complete` and `drain` now compute a reason under the guard and log after it drops.

`deliver_pending_wakes` is the other half and takes `SCHEDULER` (held by its caller) plus the
per-hart ready lock via `push_ready` — the documented `SCHEDULER → ready` order — and allocates two
`Vec`s, matching what the existing sweep already does under that lock. It is called from `yield_cpu`
where neither `FRAME_ALLOCATOR` nor `KERNEL_ROOT` is held. The corrected order noted in the ADR
(`FRAME_ALLOCATOR` first, held across mapping calls that take `KERNEL_ROOT`, and neither under
`SCHEDULER`) is untouched by this change: no path here takes either.

**Per-cell memory cost.** `size_of::<CompletionQueue>()` = **624 bytes**, measured and printed by the
self-test rather than computed on paper — 32 × 16 B slot array (`Free`/`Reserved`/`Done(isize)`), a
32 × 2 B index ring, head/len, the spinlock, the waiter tid and the wake flag. That is **one 624-byte
heap allocation per cell, not per thread**, and it is **lazy**: `Task::new` sets `completion: None`,
so while nothing is migrated the real cost is **8 bytes per task and zero heap**. The first
`reserve()` for a cell is what allocates.

**Deliberate deviation on the wake carrier.** The grant reap defers through
`Scheduler::pending_grant_reap: Vec<usize>`, pushed under `SCHEDULER`. I followed the *shape* —
record the need, act later in `yield_cpu` — but not the carrier, because an append faces two
constraints the grant reap's producers do not: it may run in interrupt context, so it must not take
`SCHEDULER`, and it must not allocate, so it cannot push to a `Vec`. A fixed-size tid array was
considered and rejected: a full array is a lost wakeup, i.e. the exact failure the ADR rejects for a
full queue. The flag has no capacity to exhaust, and clearing the global gate *before* the scan means
an append racing the scan re-raises it and is caught next tick rather than swallowed.

**Consequences worth knowing before the first migration.**

- `reserve`/`complete`/`drain`/`register_waiter`/`queue_for` have **no production caller**. The
  self-test is the only exercise they get. That is the scope, but it means the first migration is
  also the first real load.
- `register_waiter` carries a contract the type cannot enforce: the registrant must be parked in a
  state whose only wake condition is this queue. Delivery refuses to disturb
  `Ready`/`Running`/`Terminated`/`Frozen`, but it cannot tell a completion park from a `Recv` park.
  I deliberately added **no new `TaskState`** — a park state that `exit_task` and `ipc_try_send` do
  not match on is precisely the silent-discard hazard this phase's boundary exists to avoid, and it
  belongs with the syscall, against a real caller.
- `Completion.result` is `isize` with negative reserved for errors. This is *not* an ABI commitment:
  the queue is kernel-internal and unreachable from a cell, so the encoding is still open for the
  first migration to pin.
- A queue kept alive only by a completion source outlives its cell and becomes unreachable from the
  wake scan. That is intended: its registered waiter is dead, and waking a tid the table has since
  reissued would make a stranger runnable. The cost is a bounded 624-byte leak until the source drops
  its handle — never a use-after-free, which is the property the ownership choice was made for.

No file conflicts with concurrent work: `git status` shows only my five files plus the pre-existing
`build/vicell-x86.iso`, `build/x86-iso-root/boot/kernel.elf` and `tests/integration/.gitignore`
from the session-start snapshot.

---

**Status:** DONE
**Summary:** Built the bounded per-cell completion queue (kernel-owned `Arc` off the task record, 32 slots, 624 B/cell, lazily allocated), slot reservation at submission, a single-leaf-lock append and a deferred wake drained in `yield_cpu`; nothing is migrated onto it and no syscall or ABI changed.
**Verification:** All three arch gates + clippy `-D warnings` + `cargo fmt --all --check` clean; `cargo test -p api` 61+2; image via `pwsh -NoProfile -File ./gen_disk.ps1`; `qemu-boot-test.sh` → `PASS: shell prompt reached`, boot log carries `[selftest] COMPLETION-QUEUE: PASS (cap 32 slots, 624 bytes per cell)`; integration suites run serially — boot **54/54**, hotswap-smoke **11/11**, handoff **26/26**, all matching baseline, no `SKIPPED`.
**Concerns/Blockers:** Append takes exactly one lock — `CompletionQueue::ring` — established by reading `complete()`, `Spinlock::lock` (`kernel/src/sync.rs:25`) and the rv64 `interrupts_enabled`/`disable_interrupts` CSR implementations (`hal/arch/riscv/src/rv64.rs:60,70`), none of which reaches another lock; doing so forced moving the violation `log::error!` out of the guard, since the logger takes the UART lock and would have made it two. Per-cell cost is 624 bytes measured at boot, one allocation per cell rather than per thread, and lazy — 8 bytes per task and zero heap while nothing is migrated. Deliberate deviations: the deferred wake uses a per-queue flag plus a global gate rather than the grant reap's `Vec<usize>` (an append may not take `SCHEDULER` and may not allocate), and no new `TaskState` was added because a park state `exit_task`/`ipc_try_send` do not match on belongs with the first migration, not ahead of it.
