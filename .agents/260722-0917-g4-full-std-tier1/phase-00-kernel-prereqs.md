# Phase 00 — Kernel prerequisites: thread runtime, TLS, user stack, futex hardening

## Context Links
- Plan: [plan.md](plan.md) · Roadmap: `docs/project-roadmap.md §G4` · Kernel Boundary Law: `docs/specs/15-kernel-boundary.md`
- SAS frame-identity invariant: memory `project-sas-frame-identity-invariant.md`
- Reliability / never-die: `docs/specs/12-reliability.md` (recovery unit = the cell)

## Overview
- **Priority:** P1 (blocks all of P1+). **Status:** **design RATIFIED 2026-07-23** (Law-1
  confirm #1 given for the syscall batch; confirm #2 at implementation) —
  [design-p00](design-p00-kernel-prereqs-note.md) settles N1-N7 (futex ABI rebuild + TLS source
  = linker symbols + Exit/ThreadExit lifecycle + in-slot user stacks); code post-G3.
  **Grounding correction:** futex is NOT userspace-reachable today (no ViSyscall entry, no
  construction site; raw 10 = SpawnFromMem) — P0 builds the ABI fresh, and the existing
  `futex_wait` has a check-outside-lock lost-wakeup bug that the rework must fix.
- Build the real **kernel thread runtime** `std::thread` needs. Four work items, all kernel-side and all
  Boundary-Law-legal (thread/scheduler/IPC mechanism): **(A)** per-thread TLS + user stack + guard page;
  **(B)** thread lifecycle — thread-scoped exit + per-cell thread refcount + panic=abort→whole-cell abort;
  **(C)** TLS-image source + user-mode thread rebuild; **(D)** futex hardening — cell-scoped ownership
  check + cell-scoped wake scan + timeout arg. This is the ONLY kernel work G4 permits.
- **Red-team reversal folded in:** futex is **NOT "verify-only"** — it is P0 rework (see item D / C2).

## Key Insights (verified this session + red-team, with evidence)
- **No per-thread TLS on any arch.** RISC-V `Context.tp` is always hart-wide `kernel_tp_for_cells`=0
  (`kernel/src/task/hart_local.rs:32-34`); ARM64 `CpuContext` has no TPIDR_EL0, x86_64 has no FS_BASE.
- **`Task.user_stack` declared but never populated** (`tcb.rs:150-182`); spawn allocs only kernel stacks
  (`scheduler.rs:271`). **`spawn_thread` produces kernel-stack S-mode threads** (`kernel/src/task.rs:471-484`,
  `Stack::new_kernel`, `sstatus=0x120`) — NOT the user-mode/user-stack model std needs. **Rebuilding
  spawn_thread into a user-mode path is part of P0**, not an afterthought.
- **[C6] Loader ignores PT_TLS** (`kernel/src/loader/elf.rs:49-50` handles only `Type::Load`); `_start`
  gets no load_base/phdr/auxv (`startup.rs:24-101`); cells export no `__tdata_start/__tbss_end` symbols.
  So "PAL sets up TLS from its own PT_TLS" is **uncodeable as written** — the cell has no handle to it.
  **Decide the TLS-template source in the P0 design note:** (a) loader parses PT_TLS → passes base+size
  to cell entry (new spawn-ABI field), OR (b) linker scripts export `__tdata_start/__tdata_end/__tbss_end`
  and `_start` hands them to the PAL. Chosen mechanism gets LOC + a spawn-ABI section.
- **[C1] `sys_exit` runs CELL-WIDE teardown** (`kernel/src/task/syscall.rs:1477-1537`:
  `CAP_TABLE.revoke_all_for(cell_id)`, `cell_quota::deregister`, `resource_registry::release_for`,
  `reap_grants`, `iommu::cleanup_cell`). A normal tokio **worker thread exiting on the happy path would
  self-destruct the whole cell.** Also `panic=abort` today kills only the panicking thread → its
  futex-Mutex stays locked forever → siblings deadlock in FutexWait → supervisor (watches main tid)
  never observes death → **half-dead hang**. **Recovery unit is the cell, not the thread.**
- **[C2] Futex is not cell-scoped** (defeats LBI): `futex_wait` derefs a raw userspace addr with no
  ownership check (`kernel/src/task.rs:1495-1500`); `futex_wake` scans ALL scheduler tasks matching only
  `wa_addr==addr`, no `cell_id` filter, under the global SCHEDULER lock (`task.rs:1517-1527`). In the SAS
  this is a **4-byte cross-cell equality oracle** (binary-search another cell's memory → breaks
  confidentiality), a **kernel-deref DoS** (unmapped/kernel addr faults in kernel context), and a
  **cross-cell spurious-wake** vector. G4 makes futex the backbone of every std Mutex/Condvar/RwLock/Once.
- **Guard-page mechanism exists** for kernel stacks only (`stack.rs:128-139`, unmap + TLB flush). A
  deliberately-unmapped guard page is **not a freed frame** (never returned to the allocator) → no
  conflict with the SAS frame-identity invariant. **Document this distinction.**
- **CellId(0) quota-escape fix already LANDED** (`syscall.rs:1306-1334` + `thread_cap_selftest.rs`) — that
  narrow item is verify-only. Thread inherits parent `cell_id`/`CapSet`/`syscall_allowlist`/PKU → the std
  cell's manifest allowlist MUST include Futex/Spawn/GetRandom/GetTime (feeds P1).

## Requirements
- **(A)** New thread: private TLS base (tp/TPIDR_EL0/FS_BASE) set at spawn + restored on every switch;
  private **user** stack with a guard page; `#[thread_local]` resolves per-thread; runs in U/HU-mode.
- **(B)** A **thread-scoped exit** primitive removes only the tid and runs **no** cell-wide teardown;
  kernel refcounts live threads per `cell_id`; cell-wide teardown fires only when the **last** thread
  exits; a `panic=abort` in a multi-threaded cell **aborts the whole cell** (kills all sibling tids of
  `cell_id`) so never-die observes death.
- **(C)** A chosen, implemented TLS-template source; a user-mode thread spawn path (rebuild of spawn_thread).
- **(D)** `futex_wait`/`futex_wake` validate the addr lies in the **caller cell's own** mapped heap/stack
  range (reject cross-cell/kernel/unmapped) and scope the wake scan to the caller `cell_id`; `futex_wait`
  takes a `timeout` in **MTIME ticks** (m1 — reuse `MTIME_TICKS_PER_MS`, ostd lib.rs:78); wake-all via count.
- **Non-functional:** ≤1 extra load+store per switch; guard page never enters the frame free list; futex
  ownership check is O(1) range compare using the per-slot VA base invariant + defense-in-depth.

## Architecture / data flow
```
sys_spawn_thread ─▶ alloc USER stack (Stack::new_user, guard unmapped) ─▶ alloc per-thread TLS block
                 ─▶ context.{tp|tpidr_el0|fs_base}=tls_base ─▶ enter U-mode at entry, SP=user_stack.top
                 ─▶ THREAD_REFCOUNT[cell_id] += 1
context_switch ─▶ save/restore TLS-base reg (csrrw tp | msr TPIDR_EL0 | wrmsr IA32_FS_BASE)
sys_thread_exit(tid) ─▶ remove tid ; THREAD_REFCOUNT[cell_id] -= 1
                     ─▶ if refcount==0: cell-wide teardown (revoke_all/deregister/reap_grants/iommu)
panic=abort in thread ─▶ abort_whole_cell(cell_id): kill all sibling tids ─▶ last-exit teardown ─▶ supervisor restart
futex_wait(addr,val,timeout_ticks) ─▶ assert addr ∈ caller-cell VA range ─▶ value-check ─▶ park (deadline)
futex_wake(addr,count) ─▶ scan tasks WHERE cell_id==caller ∧ wa_addr==addr ─▶ wake ≤count
TLS image: loader PT_TLS→base+size (option a) | _start reads __tdata_*/__tbss_end (option b) → PAL copies
```

## Related Code Files
- **Modify:** arch `context.rs` (x86 `fs_base`, arm `tpidr_el0`, riscv per-thread `tp`) + each
  context-switch asm; `kernel/src/task.rs:471-484` (`spawn_thread` → user-mode + user stack + TLS);
  `kernel/src/task.rs:1495-1527` (futex ownership + cell-scoped wake + timeout); `scheduler.rs:256-337`
  (spawn: user-stack + TLS init all 3 arches; thread refcount); `stack.rs` (`new_user` w/ guard);
  `tcb.rs` (populate `user_stack`; `tls_base`); `syscall.rs:1477-1537` (split cell-wide teardown behind
  last-thread refcount; add `sys_thread_exit`; `abort_whole_cell`); `loader/elf.rs:49-50` (PT_TLS, if opt a).
- **Modify (Law 1 watch):** if the futex ABI or spawn ABI adds/changes a `libs/api` syscall field →
  **2× user confirmation**. Prefer additive op args that keep discriminants stable.
- **Create:** `kernel/src/task/thread_tls_selftest.rs`, `thread_lifecycle_selftest.rs`,
  `futex_scope_selftest.rs` (all test-hooks).

## Implementation Steps
1. **(Now)** Design note: guard-page-≠-freed-frame; TLS model=initial-exec; **TLS-template source (a/b)**;
   thread-lifecycle contract (recovery unit = cell); futex cell-scoping + timeout-unit. Review in G3 window.
2. Add TLS-base to x86/arm context + switch asm; wire riscv per-thread tp.
3. `Stack::new_user` + guard; rebuild `spawn_thread` into a user-mode path; populate `Task.user_stack`.
4. Implement chosen TLS-template source; per-thread `.tdata` copy / `.tbss` zero; main-thread TLS at entry.
5. Thread refcount per cell_id; `sys_thread_exit` (no cell-wide teardown); last-thread fires teardown;
   `abort_whole_cell` on panic=abort; ensure supervisor observes cell death.
6. Futex: addr-ownership validation + cell-scoped wake scan + `timeout_ticks` (MTIME).
7. Selftests: 2 threads distinct TLS + stacks; guard fault; worker exit ≠ cell death; panic kills cell;
   cross-cell futex addr rejected; wake does not cross cells; `wait_timeout(100ms)` ≈ 100ms vs GetTime.

## Todo List
- [x] Design note (guard/TLS-model/TLS-source/lifecycle/futex-scope) — drafted 2026-07-23,
      [design-p00](design-p00-kernel-prereqs-note.md); TLS-source = **(b) linker symbols**;
      futex ABI = NEW ViSyscall 240/241 + SetTls 242 + ThreadExit 243 (Law-1 batch)
- [ ] Per-thread TLS base + switch asm (x86/arm/riscv)
- [ ] `Stack::new_user` + guard + user-mode `spawn_thread` rebuild
- [ ] TLS-template source implemented (option a loader-PT_TLS or b linker-symbols)
- [ ] Thread refcount + `sys_thread_exit` + last-thread teardown
- [ ] `panic=abort` aborts whole cell; supervisor observes death
- [ ] Futex addr-ownership check + cell-scoped wake + `timeout_ticks` (MTIME)
- [ ] Selftests pass on x86_64 + aarch64 (`THREAD-TLS/STACK-GUARD/LIFECYCLE/FUTEX-SCOPE: PASS`)
- [ ] Verify CellId(0) quota fix still green

## Success Criteria
- QEMU x86_64: a cell spawns 2 threads with distinct `#[thread_local]` + distinct stacks; stack overflow
  faults on the guard page; **a worker thread returning does NOT tear down the cell**; a `panic!` in any
  thread **kills the whole cell and the supervisor restarts it** (no half-dead hang); a futex op on
  another cell's address is **rejected** and `futex_wake` never wakes a sibling cell; `Condvar::
  wait_timeout(100ms)` unblocks in ~100ms. Oracles: `THREAD-TLS/STACK-GUARD/LIFECYCLE/FUTEX-SCOPE: PASS`.
- Verified on x86_64 (primary) + aarch64; riscv64 tracked separately.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **[C1] Worker-exit self-destructs cell / half-dead hang on thread panic** | H×H | Thread-scoped exit + per-cell refcount; panic=abort→whole-cell abort so never-die fires; recovery unit = cell (documented) |
| **[C2] Futex cross-cell oracle / kernel-deref DoS / spurious wake** | H×H | Addr-ownership range check before deref/park; cell-scoped wake scan; per-slot VA-base invariant as primary + identity check as defense-in-depth |
| **[C6] TLS image unlocatable → P0 uncodeable** | H×H | Design note picks loader-PT_TLS (a) or linker-symbols (b) BEFORE coding; add to spawn ABI + LOC |
| spawn_thread rebuild (S-mode→U-mode) larger than a field add | H×M | Treat as first-class P0 work; user-stack + mode transition + TLS are one coherent rebuild |
| Guard page mistaken for freed frame → identity-map panic | M×H | Never free a guard; only unmap; assert in `Drop for Stack`; design note + selftest |
| Futex timeout wrong tick unit (10000× error) | M×H | Pin to MTIME ticks via `MTIME_TICKS_PER_MS`; unit test wait_timeout vs GetTime (m1) |
| x86 FS_BASE via wrfsbase needs CR4.FSGSBASE | M×M | Prefer `wrmsr(IA32_FS_BASE)`; probe FSGSBASE, fall back |

## Security Considerations
- Guard page is defense-in-depth vs silent cross-cell heap corruption (no MMU wall in SAS).
- Futex ownership check is a **confidentiality control**, not just robustness — without it futex is a
  cross-cell read oracle that defeats LBI. This is load-bearing, not optional hardening.
- Whole-cell abort on panic preserves the isolation/recovery model: a fault never leaves shared locks
  held by a dead thread visible to survivors.
- Thread inherits parent CapSet/allowlist (landed fix) — no path may re-introduce `CellId(0)`.

## Next Steps
- Unblocks P1 (PAL thread/futex/TLS). Feeds target-JSON `tls-model`/`has-thread-local` and the manifest
  allowlist set. Thread-lifecycle contract feeds P3/M5 (blocking-pool bound by cell quota).
