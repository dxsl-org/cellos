# P0 Design Note — thread runtime, TLS, user stack, futex hardening

> **Status:** **RATIFIED 2026-07-23** — user approved same day: ① the Law-1 syscall batch
> (FutexWait/FutexWake/SetTls/ThreadExit + `Futex` bit + Spawn additive `a2=tls_base`) —
> this is **confirm #1 of the Law-1 2×**, confirm #2 due at implementation; ② TLS source =
> option (b) linker symbols; ③ Exit(60) → whole-cell kill-siblings-first; ④ all std-cell
> stacks in-slot via `Stack::new_user_at`. Code post-G3.
> Scope = phase-00's four work items (A) TLS+stack, (B) lifecycle, (C) TLS-source+user-mode
> spawn, (D) futex hardening. All kernel-side, all Boundary-Law-legal mechanism.

## Grounding correction found this session (updates the plan)

**Futex is NOT reachable from userspace today.** The kernel has `Syscall::FutexWait/FutexWake`
variants (`syscall.rs:457-460`) and executor arms (`syscall.rs:1431-1447`), but:
- `map_syscall` decodes via `ViSyscall::from(syscall_id)` (`syscall.rs:3692-3693`) and
  **`ViSyscall` has no Futex entries** (grep: zero `Futex` matches under `libs/`);
- nothing anywhere constructs `Syscall::FutexWait { .. }` (grep: no construction site);
- the raw value `10` is already `ViSyscall::SpawnFromMem` (`api/abi/syscall.rs:30`), so the
  "syscalls 9/10" from the internal enum comments are **not** an ABI.

So P0's futex work = *(re)build the user ABI* + hardening, not "add a timeout to a live
syscall". This also means there are **zero existing userspace futex callers** → the ABI can be
designed clean with no compat constraint.

**Bonus defect found (must fix in the same rework):** `futex_wait` does the value check
*outside* the SCHEDULER lock (`task.rs:1493-1500` deref, then `:1502` lock+park). A
`futex_wake` between check and park is a **classic lost wakeup** — with std Mutex/Condvar built
on this, that is a hang. The rework MUST do value-check + park atomically under the lock.

## Decisions

### N1 — Futex user ABI (new ViSyscall entries — Law 1, 2× confirm)

Claim three new `ViSyscall` numbers (propose `240 FutexWait`, `241 FutexWake`, `242 SetTls`;
`243 ThreadExit` in N3). Free-number audit (review-corrected): 236 = `RegisterPciDevice`,
238 = `SpawnFromElf` — NOT free; the 239-299 gap is free, so **240-243 are confirmed free**
(re-audit the `From<usize>` table at implementation anyway):

```
FutexWait  a0=addr  a1=expected(u32)  a2=timeout (SCHEDULER ticks, 10 ms each; 0 = infinite)
           → Ok(0) woken · Err(TryAgain) value mismatch · Ok(1) timeout
FutexWake  a0=addr  a1=count (usize::MAX = wake-all)  → Ok(n_woken)
SetTls     a0=tls_base → sets CURRENT task's TLS register (see N5/N6)
```

- One shared allowlist bit `Futex` for wait+wake (declared in the manifest); `SetTls` is
  always-permitted (self-only, no authority).
- **Timeout unit = SCHEDULER ticks (10 ms), the same clock as `RecvTimeout`** — NOT MTIME.
  Review finding (m1 realized): the deadline mechanism this reuses is driven by
  `system_ticks()` (a software 10 ms tick counter, `task.rs:160-165`; `RecvTimeout` computes
  `system_ticks() + a3`, `syscall.rs:3728`) — an MTIME-unit arg fed into it would be ~10⁵× off.
  The PAL converts `Duration → ceil(ms/10)` ticks; 10 ms granularity is acceptable for
  `Condvar::wait_timeout` (document it). Unit test vs `GetTime` wall delta.
- Timeout implementation reuses the `TaskState::Recv`-style deadline: `FutexWait { addr,
  deadline: Option<u64> }` in `TaskState`, timer path wakes expired waiters (same mechanism as
  `RecvTimeout`, `syscall.rs:1234-1245`).

### N2 — Futex cell-scoping (C2): O(1) ownership check via the VA slot

- PIE cells live in a **32 MiB VA slot**: `CELL_VA_START = 0x1_0000_0000`, `CELL_VA_STRIDE =
  0x200_0000` (`va_alloc.rs:38-44`). Everything a std cell can legitimately futex on — `.data/
  .bss` statics, the cell heap (a static region inside `.bss`), and **ALL stacks** — must fall
  inside `[slot_base, slot_base + 32 MiB)`. **Review finding folded in: today the MAIN task's
  stack is identity-mapped at a low physical frame OUTSIDE the slot** (`loader/syscall.rs:575`
  → `stack.rs:86`) — a stack-local `Mutex`/`park` on the main thread would fail the check.
  **Therefore N4 relocates the main std-cell stack in-slot too** (spawn path), not just worker
  stacks. Non-std cells keep their current stacks — they have no `Futex` manifest bit.
- **Check (before ANY deref):** `addr % 4 == 0 && addr ∈ caller-cell slot range`, else
  `Err(PermissionDenied)` — this kills the cross-cell 4-byte read oracle AND the kernel-deref
  DoS in one range compare. The kernel records `cell_id → va_slot_base` at spawn (the loader
  already gets it from `alloc_cell_va`, `task.rs:542`; store it in the cell registry).
- Fixed-VA (non-PIE) cells have no slot → **futex denied** (fail-loud). std cells are always
  PIE; embedded fixed-VA cells have no std and no futex bit in their manifests.
- `futex_wake` scan adds `task.cell_id == caller.cell_id` to the match (`task.rs:1517-1527`)
  — wakes can never cross cells even if two cells race on the same numeric address.

### N3 — Thread lifecycle (C1): Exit stays cell-wide; NEW ThreadExit for workers

- **`Exit (60)` becomes whole-cell death: kill ALL sibling tids, then the cell-wide teardown.**
  Review correction: this is a semantic FIX, not "today's semantics" — current `Exit` reaps
  only the caller tid (`scheduler.rs:348-379`) while deregistering the whole cell's quota +
  caps (`syscall.rs:1514-1517`). Since `Spawn=5` is already live, a multi-tid cell where one
  tid exits today leaves **siblings running with revoked caps** — a live latent corruption
  that the refcount design also fixes. Single-threaded cells see zero behavior change.
- **New `ViSyscall::ThreadExit`** (propose `243`; always-permitted like Exit): reaps ONLY the
  calling tid. Kernel keeps a per-cell live-thread refcount; when the count hits 0 (last
  thread gone, however it went), the cell-wide teardown fires exactly once.
- **panic=abort in ANY thread → whole-cell abort**: the PAL's abort path calls `Exit`, which
  kills siblings first — no locked-futex orphans, and the supervisor (which watches the cell)
  observes a normal cell death → never-die restart works. Recovery unit = cell (spec 12).
- Join: `ViSyscall::Wait = 8` already exists (`api/abi/syscall.rs:619`, executor
  `syscall.rs:1354`, decode `:3791`) and blocks until the target tid dies, returning the exit
  code — `JoinHandle::join` maps to it with no new syscall. **Review correction: Wait is 8,
  NOT raw 3 — raw 3 decodes to `Reply` (`:607`), which would silently corrupt IPC state.**
  Wait is allowlist-gated (bit 9) → goes in the std manifest set. [verify: reap path wakes
  Wait-ers on ThreadExit deaths, `scheduler.rs:339-379`]

### N4 — User stacks (A): kernel-allocated, in-slot, guarded

- Rebuild `spawn_thread` (`scheduler.rs:256-337`) around a **new in-slot stack path**.
  **Review correction: this is NEW mechanism, not a reuse.** Today `Stack::allocate` maps
  **identity** at the physical frame (`stack.rs:86,118`) and `Drop` restores frame-identity
  by address (`stack.rs:192-195`) — it cannot place a stack at a slot VA. P0 adds
  `Stack::new_user_at(slot_va, pages)`: map allocated frames at slot VAs, guard page = the
  bottom slot-VA PTE cleared (guard *frame* stays owned, same principle as today); `Drop`
  unmaps the slot VAs AND **restores identity mapping on the frames** before freeing them —
  that is what the SAS frame-identity invariant requires (it governs *freed frames*, not
  live guard pages). Budget: this is part of why P0 is ~800-1400 LOC.
- **All std-cell stacks (main + workers) live in the slot's stack region** — reserve the top
  4 MiB of the 32 MiB slot (sub-allocator; 64 KiB usable + 1 guard page per thread → ~60
  threads/cell headroom, quota-charged). Two reasons: (1) keeps N2's futex check a single
  range compare (stack-local Mutex passes, main thread included); (2) PIE cells already run
  at non-identity slot VAs, so this is the established exception zone — no new invariant.
- Entry state: U-mode (riscv `sstatus.SPP=0`, not today's `0x120` S-mode — `scheduler.rs:292`),
  `sp = stack_top`, `a0 = arg`, `pc = entry`, TLS reg = `tls_base` from N5.

### N5 — Spawn ABI: additive third arg (Law 1, additive — discriminant stable)

`ViSyscall::Spawn (5)` today takes `{entry: a0, arg: a1}` (`syscall.rs:3738`). Extend
**additively**: `a2 = tls_base` (0 = no TLS, kernel leaves the register zero). The kernel
loads it into the arch TLS register at first entry:
- riscv64: trap-frame `tp` (x4) — user tp is already saved/restored via the trap frame; only
  the *initial* value needs setting. Kernel-side hart tp (`hart_local.rs:32-34`, sscratch
  invariant) is untouched — kernel and user tp are separate save slots. [verify trap frame
  x4 round-trip on U-mode entry]
- aarch64: new `tpidr_el0` field in `CpuContext`, save/restore in the switch path (EL0 can
  also write TPIDR_EL0 directly; kernel save/restore makes it per-thread).
- x86_64: `IA32_FS_BASE` via `wrmsr` on switch; per-task value lives in the `Task` struct
  (ViTrapFrame has no free slots — established pattern: per-task CPU state in Task + gs:16).
  Cost: 1 `wrmsr`/switch only when the value differs; probe FSGSBASE later as an optimization.

The thread's TLS *block* is allocated by the PAL **from the cell heap** (userspace, quota'd);
the kernel only ever receives the pointer. Main thread: the entry shim calls `SetTls` (N1)
after building the main TLS block — needed because the *cell* spawn path has no tls arg and
x86_64 userspace cannot set FS_BASE unprivileged.

### N6 — TLS template source (C6): option (b) — linker symbols. Loader untouched.

**Decision: (b).** The std entry shim's linker script (P1/C4 already owns a `.ld` template
that KEEPs `__ViCell_manifest`/`__ViCell_syscalls`) additionally exports:

```
__tdata_start / __tdata_end / __tbss_end   (PROVIDE, PT_LOAD-resident .tdata image)
```

The PAL computes `tdata_len`/`tbss_len` from the symbols, allocates per-thread blocks from
the heap, copies `.tdata`, zeroes `.tbss`, calls `SetTls`/passes `tls_base` to `Spawn`.
- **Why not (a) loader-PT_TLS:** (a) needs `elf.rs` PT_TLS parsing + a *new spawn-ABI channel*
  to hand base+size into `_start` (today `_start` gets no auxv — `startup.rs:24-101`) — a
  kernel + Law-1 change that buys nothing (b) doesn't already give, and violates
  minimal-kernel doctrine. The symbols resolve statically inside the cell — zero kernel work.
- Risk + mitigation: a cell whose `.ld` lacks the symbols would get silent zero-size TLS →
  the shim `assert!(__tdata_end >= __tdata_start && aligned)` at startup (fail-loud); std
  cells can only be produced through the shim's `.ld` template.
- TLS model = **initial-exec** (static TLS only; no dlopen exists) — feeds the P1 target JSON
  (`tls-model: initial-exec`, `has-thread-local: true`).

### N7 — TLS destructors

Program-structure-enforced (no OS callback): `destructors::run()` is called by (1) the PAL
`thread_start` trampoline before `ThreadExit`, and (2) the entry shim after `main`/before
`Exit`. Whole-cell abort skips destructors by design (abort semantics).

## Kernel diff inventory (all mechanism, ~800-1400 LOC incl. selftests — unchanged budget)

| Item | Files | Law-1? |
|------|-------|--------|
| ViSyscall FutexWait/FutexWake/SetTls/ThreadExit + allowlist bit + map_syscall arms | `libs/api/abi/syscall.rs`, `kernel syscall.rs` | **YES — 2× confirm** (additive numbers) |
| futex: atomic check+park, slot-range ownership, cell-scoped wake, deadline | `task.rs:1493-1539`, `tcb.rs` (FutexWait state + deadline) | no |
| Spawn a2=tls_base (additive) + U-mode entry + NEW `Stack::new_user_at` in-slot path (main + workers) + Drop restores frame identity | `syscall.rs:3738,1306-1352`, `scheduler.rs:256-337`, `stack.rs`, loader spawn path | additive only |
| per-cell thread refcount + ThreadExit reap + Exit kills siblings first | `syscall.rs:1477-1537`, `scheduler.rs` | no |
| arch TLS reg: aarch64 `tpidr_el0` in CpuContext; x86_64 FS_BASE in Task+wrmsr; riscv trap-frame tp verify | `hal/arch/*` context + switch | no |
| cell registry records `va_slot_base` | `cell/registry`, `task.rs:542` | no |
| selftests: THREAD-TLS / STACK-GUARD / LIFECYCLE / FUTEX-SCOPE | new `kernel/src/task/*_selftest.rs` (test-hooks) | no |

## Open items carried to implementation (not design-blocking)

1. [verify] The reap path (`scheduler.rs:339-379`) wakes `Wait`-ers for `ThreadExit` deaths
   (the reap contract says all death paths wake waiters — confirm ThreadExit uses it).
2. ~~riscv trap-frame tp round-trip~~ **verified in review** — `trap.S:50,152` saves/restores
   x4 per-task for U-mode; only aarch64 TPIDR_EL0 + x86_64 FS_BASE are new work.
3. Free-number re-audit for 240-243 at implementation time (239-299 gap confirmed free
   this session; the `From<usize>` table is the single source of truth).
4. Whether `futex_wake` should pend a preempt (`pend_preempt_if_needed`) like `ipc_try_send`
   does — decide with a latency measurement in the FUTEX-SCOPE selftest.
