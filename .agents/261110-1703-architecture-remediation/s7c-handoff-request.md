# S7c handoff request: frame ownership and shootdown contract (Phase 16)

**Date:** 2026-11-14 · **Plan:** [plan.md](plan.md) · **Prior:** Phase 15 closed 2026-11-14 ([evidence](reports/phase-15-evidence.md)), `main` at `6da3c4b3e`.

Phase 16 is a checkpoint phase and the last one in S7.

## 1. Scope (measured on `6da3c4b3e`)

| Surface | Today |
|---|---|
| Shared-root shootdown | `memory/tlb_shootdown.rs:433` panics when a remote x86 invalidation stays unconfirmed past the wait bound. `:412` also panics when an RV64 RFENCE fails. Private roots already probe-then-defer (`deferred_release.rs`). |
| Frame owners | `OwnedFrame` is used in 3 files. Raw frame frees outside `memory/frame*`: the x86 SVM registry (10), `task/stack.rs` (8), `memory/stage2.rs` (4), the x86 EPT (3), `paging.rs` (3), `syscall/grant.rs` (3), plus loader, deferred release and the SMP bring-up. |
| Region tables | `SHM_HANDLES`, `PAGE_GRANT_TABLE`, `REG_GRANT_TABLE` (`task/syscall/mod.rs`), DMA `EXPORTS` (`dma_export.rs:243`) and domain grants. Each has its own lifetime rules. |
| Retain-until-safe | Done in several separate ways: deferred release, pin quarantine, grant withhold, the Phase 15 domain sweep, and others. Each one restates the "free frame is identity-mapped kernel RWX" invariant (RV64 / AArch64). |

## 2. Staging (proposed, same shape as S7/S7b)

1. **Census.** Read only; produces `research/p16-census.md`.
   - Tabulate every release protocol: conditions waited on, lock order, flush scope, what happens on timeout.
   - List every raw alloc/free site.
   - Measure the five region tables.
   - Check the plan's three low-confidence claims.
   - Design the `FrameRun` states and reasons from the union of those protocols.
2. **`FrameRun` and the allocator guard type**, with host tests for every transition. The rule "flush while holding `FRAME_ALLOCATOR`" must fail to compile.
3. **No panic on an unconfirmed shared-root invalidation.** The frames are quarantined, logged and retried on later epochs; they are never reused before confirmation. Per-page flush callers are batched.
   - New x86 fixture: one CPU is held with interrupts masked past the bound. A cell teardown must still complete, and its frames are quarantined and freed later. Today this panics.
4. **Migrate owners, one per commit, in risk order:** stacks → cell segments → grants/SHM → DMA exports → domain grants → hypervisor tables (EPT, Stage-2, VMCB). Each commit keeps its old debug asserts and runs the stress and grant lanes.
5. **One `Region` record behind the five tables.** SHM moves onto Grant only where its callers allow. Syscall-visible grant ids do not change.
6. **Spec 02 §3 and Spec 19.**
7. **Checkpoint:**
   - the pr-tier suite on three arches;
   - the x86 4-CPU stress and domain-grant suites (TCG and KVM);
   - RV64 `--harts 2`, AArch64 test-hooks;
   - the hypervisor smokes, `x86-c2c`;
   - an independent review.

Work happens in a worktree, one commit per step or per owner, rebased before the merge. Nothing is pushed.

## 3. File set

- **Create:** `kernel/src/memory/{frame_run,region,cell_segments}.rs` (`cell_segments` moves from `task/stack.rs`), the quarantine fixture, `research/p16-census.md`, `reports/phase-16-evidence.md`.
- **Modify:**
  - `kernel/src/memory/**`, `kernel/src/task/{stack,domain_grant}.rs`, `kernel/src/task/syscall/{mod,grant,dma_export,misc}.rs`, `kernel/src/task/user_copy/**`;
  - `kernel/src/hypervisor/**` and `kernel/src/arch/*/{hypervisor,memory}/**`;
  - `kernel/src/loader/**` (frame owners only);
  - `hal/arch/*/` (only where a page-table free hook moves);
  - lane scripts for the new fixture, `scripts/lanes.toml`, `scripts/check-hal-boundaries.sh`, `kernel/residents.toml`, `scripts/mutable-globals.toml`;
  - `docs/specs/{02-memory,19-hardware-isolation-layers}.md`.
- **Delete:** per-site quarantine lists once migrated, and the dead per-task `grant_table`.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `docs/project-changelog.md`.

`libs/abi` and `libs/types` do not change: no syscall number, grant id or wire byte moves, so there is no Law-1 checkpoint.

## 4. Behaviour to expect

- **Unconfirmed shared-root invalidation:** where the kernel panics today, it quarantines the frames, logs it loudly and keeps running. A persistently silent CPU leaks those frames (counter and log), and new allocations are refused when the quarantine is too large: the node degrades, nothing is corrupted.
- **Everything else:** the same observable behaviour, with teardown cost of O(operations) broadcasts rather than O(pages).

## 5. Risks and conflicts

- **Highest risk in the plan:** a wrong drop order frees a live frame. Owners migrate one at a time, no wait is weakened, and each owner's old asserts stay until its lane passes.
- **Other session in `/home/dmin/cellos`:** none of its recent commits touch `memory/` or the hypervisor. Each commit is rebased before merge.
- **Intel WIP plans:** the large-memory work (`AnonMemory`) and the XSAVE work would touch the same allocator and grant files. S7c holds those files while it runs.

## 6. Questions to the owner

1. Grant S7c staged as in §2, continuing without stops, as S7/S7b did. It stops only on a behaviour difference other than the panic → quarantine change, or if an ABI change is needed.
2. Quarantine bound: refuse new frame allocations once quarantined frames exceed a cap. Proposed: 1/64 of RAM, reported in the log and a counter. Alternatives are no cap (log only) or a smaller cap.
3. Hypervisor tables (EPT, Stage-2, VMCB) migrate in this phase (proposed), or are left for a later phase to reduce risk.
