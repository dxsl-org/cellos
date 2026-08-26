# Phase 08 — stack safety slice (guard-page enforcement + bounded thread creation)

Date: 2026-07-31 · Branch: `feat/wx-post-reloc-and-f1-signing` · Scope: safety items only.
No per-path stack sizing, no `STACK_PAGES` change, no watermark work — that half stays
blocked on phase 07, as directed.

## Files Modified

| File | Δ | What |
|------|---|------|
| `kernel/src/task/stack.rs` | +64/−49 | Guard failure fails the allocation; both error paths release their frames; `release_frames` shared with `Drop` |
| `kernel/src/task/scheduler.rs` | +62/−5 | Thread stack charged to the cell's quota; refund in `exit_task`; rustdoc on the two bounds |
| `kernel/src/task/tcb.rs` | +15 | `Task::stack_quota_charge` |
| `kernel/src/task/thread_quota_selftest.rs` | +182 (new) | Boot self-test: charged, released, enforced |
| `kernel/src/task.rs` | +1 | Module registration |
| `kernel/src/main.rs` | +5 | Self-test invocation next to the existing ones |

`git status` shows no other source file touched; `build/vicell-x86.iso`,
`build/x86-iso-root/boot/kernel.elf` and `tests/integration/.gitignore` were already
dirty at session start and are not mine.

## Item 1 — a failed guard fails the spawn

`Stack::allocate` now has exactly two outcomes: a stack whose guard frame is *verified*
absent from the page tables, or an error with every frame handed back. The
"Non-fatal: stack is still usable, just unguarded" branch is gone.

Two things beyond the literal ask:

- **The guard is verified by translation, not by the unmap's return code.**
  `paging::unmap_page` reports `Ok` for pages it never touched — on riscv64/aarch64 it
  deliberately maps the underlying `unmap` error to `Ok` (right for an already-unmapped
  page, indistinguishable from a failed one), and on x86_64 it returns `Ok` when the
  paging root is absent. Trusting it would have left the same silent hole one layer down.
  `virt_to_phys(base_addr).is_some()` after the unmap is the only answer that matches
  what the hardware will do on overflow.
- **The pre-existing `map_page` failure path leaked its frames.** It returned `Err` with
  the contiguous run already allocated and no `Stack` in existence to drop it — lost until
  reboot. Adding a second early return without fixing that would have doubled the leak, so
  both paths now call `release_frames`, which is the same normalise-then-deallocate loop
  `Drop` uses (unmap → remap kernel-RWX → deallocate, so every frame reaches the free list
  identity-mapped, which the cell loader depends on).

Guard failure returns `ViError::NotSupported`, not `OutOfMemory`: the thread-spawn syscall
maps `OutOfMemory` to `TryAgain`, and a guard that cannot be established will not become
establishable on retry.

**Every caller handles it.** All eight `Stack::new_kernel` / `new_user` sites already
propagate `Result`, and none panics: `Scheduler::spawn` and `spawn_thread` return `Err`;
`task.rs:725` (cell spawn) and `task.rs:1792` (`spawn_synthetic`) return before touching the
scheduler, so `Drop` unwinds the partially built cell; `user_hello.rs:100`
(`test-hooks` only) returns `Err`; `smp.rs:53` logs and `continue`s to the next hart. No
`.expect` remains on any stack allocation path.

Empirically, no `Stack alloc refused: guard frame … still mapped` line appears anywhere in
a full riscv64 boot, across every cell spawn — so the guard is genuinely established, not
merely reported.

## Item 2 — bounded thread creation

### Per-cell live-thread limit

`MAX_THREADS_PER_CELL = 32` already existed (landed in `7621a7f6`, which the phase file
predates), enforced in `spawn_thread` with a `log::warn!` naming the cell and a
`ThreadCapReached` audit event, and covered by `thread_cap_selftest`. I reviewed it rather
than re-deriving it, and kept the value. Reasoning for 32, now recorded in the constant's
rustdoc: no cell spawns threads today (`ostd` exposes `sys_spawn`, nothing calls it), so 32
is roughly an order of magnitude above any plausible worker pool on a 256 MiB machine,
while 32 × 65 contiguous frames ≈ 8.3 MiB stays far below the point where 65-frame runs
stop being findable. It is also the tighter of the two bounds — the default 16 MiB quota
would admit ~63 stacks on its own — which is deliberate: a cell should be refused on a
number an operator can reason about, not on whatever heap it happened to be holding.

**The slot cannot leak.** The count is recomputed from `self.tasks` on every call rather
than kept in a counter, so a thread frees its slot the instant `exit_task` removes it from
the table. There is no bookkeeping to drift.

### Quota charge

`spawn_thread` charges `kstack.allocated_bytes()` (266 240 B = 65 × 4 KiB, read back from
the `Stack` rather than recomputed from `STACK_PAGES` — a second independent use of the
same constant is exactly how the memset bug happened) to `cell_quota`, and refuses with
`OutOfMemory` → `TryAgain` if the cell cannot afford it, logging cell, bytes and bytes
in use. `charge` rolls back its own optimistic add and `kstack` drops on that path, so a
refusal needs no manual unwinding.

The charge is recorded in `Task::stack_quota_charge` and refunded in `Scheduler::exit_task`
via `core::mem::take`, which makes it exactly-once even if `exit_task` ran twice for a tid.
Refunding at reap instead would bill a cell for a thread that is already dead for as long
as the zombie sits unreaped — the slow leak the brief warned about.

Cell-owned stacks are deliberately *not* charged: they are the cost of admitting the cell,
fixed at spawn, not something the cell can ask for repeatedly at runtime. Changing that
would take 520 KiB off every cell's 16 MiB budget and is a separate decision.

### Boot self-test

`kernel/src/task/thread_quota_selftest.rs` runs in the same single-hart window as the
existing task self-tests and proves both directions on a real spawn through the real
syscall path: the charge appears at exactly the stack size, is zero again after the thread
dies **through `exit_task`** (not by lifting it out of the table — the refund is only
correct if it happens in the funnel), and a cell whose quota cannot absorb a stack is
refused with `TryAgain` leaving nothing charged. It snapshots and restores `next_task_id`
and deregisters its quota slot, so boot is unchanged whether or not it is compiled in.

Observed on serial:

```
[ WARN] [sched] cell CellId(63) cannot afford a thread stack (266240 bytes, 0 in use) — refusing spawn_thread
[ INFO] [selftest] THREAD-QUOTA: PASS (charged, released, enforced)
```

## Verification

| Command | Outcome |
|---------|---------|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | pass (mutation-checked — an injected type error was reported, so the fast exit is real) |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | pass |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | pass |
| `cargo clippy … riscv64 … -- -D warnings` | clean |
| `cargo fmt --all --check` | clean (whole workspace, not just my files) |
| `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api` | 61 + 2 passed, 0 failed (4 ignored) |
| `pwsh -NoProfile -File ./gen_disk.ps1` | image built (`tetris-c`, `tetris-lua` fail to build on this box — pre-existing) |
| `cargo build --release -p vicell-kernel --target riscv64gc…` | pass |
| `bash scripts/qemu-boot-test.sh …/release/vicell-kernel` | `PASS: shell prompt reached` |
| `cargo test --test boot -- --test-threads=1` | **54 passed, 0 failed** (356 s) |
| `cargo test --test hotswap-smoke -- --test-threads=1` | **11 passed, 0 failed** |
| `cargo test --test handoff -- --test-threads=1` | **26 passed, 0 failed** |

All three suites ran serially, each printing a real `test result:` line with a non-zero
count — no `SKIPPED`. Full serial capture also shows both task self-tests PASS and
`[vfs-test] Results: 57 PASS, 0 FAIL`.

## Status

**Status:** DONE

**Summary:** `Stack::allocate` now refuses to return a stack whose guard page it cannot
verify as unmapped, releasing every frame on the way out (and fixing a pre-existing frame
leak on the mapping-failure path); thread stacks are charged to the spawning cell's memory
quota and refunded in `exit_task`, so the existing 32-thread cap is no longer the only
thing standing between an unprivileged cell and contiguous-memory exhaustion.

**Verification:** table above — riscv64/x86_64 check, aarch64 build, clippy `-D warnings`,
`cargo fmt --all --check`, `api` 61+2, and after `gen_disk.ps1` + a release riscv64 build:
`qemu-boot-test.sh` PASS, boot **54/54**, hotswap-smoke **11/11**, handoff **26/26**, all
serial.

**Concerns/Blockers:**

- **Thread limit: 32, unchanged.** Pre-existing and kept, not re-derived. It is ~10× any
  plausible worker pool (nothing in the tree spawns threads at all today), costs at most
  8.3 MiB of contiguous frames per cell, and stays the *first* refusal a cell meets — the
  16 MiB quota alone would admit ~63 stacks — so a cell is turned away on a fixed number an
  operator can reason about rather than on its momentary heap. Its rustdoc now states that
  interaction.
- **Spawn capability: deliberately NOT required for thread creation.** A thread is the same
  principal as the cell that asked for it — same `CellId`, `CapSet`, syscall allowlist and
  PKU domain, asserted by the existing `thread_cap_selftest` — so it grants no authority the
  cell does not already hold. `SpawnCap` gates creating a *different* principal; demanding
  it for intra-cell concurrency would force every cell that wants a worker thread to be
  handed the far stronger power to spawn arbitrary cells, which is an authority escalation
  performed in the name of DoS prevention. The concern is a resource concern and is met with
  resource tools (count cap + quota charge, both now enforced and self-tested), and an
  operator who wants a specific cell to have no threads at all already has the precise knob:
  omit `Spawn` from that cell's syscall allowlist.
- **Death paths confirmed to release both the slot and the quota.** The slot is derived from
  `self.tasks`, and the refund is in `Scheduler::exit_task`, so both are released wherever
  `exit_task` runs. Every death path funnels there, and I checked each:
  clean `Syscall::Exit` (`syscall.rs:1835`); `ForceExit`/kill (`syscall.rs:1932`); RT
  cluster-policy rejection (`syscall.rs:2596`); hardware fault / cell termination
  (`task.rs:368`); orphaned-task cleanup when post-spawn context setup fails
  (`task.rs:858`); CPU-monopoly watchdog (`scheduler.rs`, `pick_next_local`); heartbeat
  liveness sweep (`scheduler.rs`, `pick_next`); hot-swap retirement (`cell/hotswap.rs:189`).
  The watchdog and heartbeat paths call `cell_quota::deregister` *before* `exit_task`, which
  zeroes the cell's counter; the later refund then saturates at 0 rather than underflowing.
  The only removal that bypasses `exit_task` is `thread_cap_selftest`'s teardown helper,
  whose synthetic cells sit above `MAX_CELLS` where `charge`/`refund` are both no-ops, so
  nothing is stranded — my own self-test uses `exit_task` precisely so it exercises the real
  funnel.
- **Cells with id ≥ 64 are uncapped**, because `cell_quota` only tracks `MAX_CELLS = 64`
  slots. Pre-existing and equally true of the heap quota; a normal boot tops out around tid
  13, so no real cell reaches it today. Worth a follow-up if the cell count ever grows.
- **x86_64 and aarch64 were not boot-tested** — only the riscv64 gate exists here. The guard
  verification is arch-uniform and the identity map is 4 KiB-granular on all three (no huge
  pages that a 4 KiB unmap could fail to split), so I expect no divergence, but the positive
  evidence is riscv64 only.
- Files touched outside the phase's Related Code Files table (`tcb.rs`, the new self-test,
  plus additive lines in `task.rs`/`main.rs`) and the reasoning for skipping a new
  `AuditEvent` variant are recorded in the phase file's § Deviation Log.
