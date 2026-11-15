# S8 handoff request: quota ownership and shared-root hardening (Phase 17)

**Date:** 2026-11-15 · **Plan:** [plan.md](plan.md) · **Prior:** Phase 16 closed 2026-11-15 ([evidence](reports/phase-16-evidence.md)), S7 closed, `main` at `65070229f2`.

Phase 17 is the first S8 phase. It is not a checkpoint phase.

## 1. Scope (measured on `65070229f2`)

| Surface | Today |
|---|---|
| Heap charging | `QuotaAlloc` charges `current_cell_id()` of the hart (`memory/heap.rs:169`). It already refunds the incarnation a block was charged to. 12 kernel files switch the current cell to 0 so that kernel bookkeeping is not billed to a cell: `ipc_wire`, `pending_mailbox`, `pipe`, `async_ipc`, `completion`, `scheduler`, `spawn`, `task.rs` and others (20 sites). |
| Quota tables | `IN_USE` and `DMA_IN_USE` are `[AtomicUsize; MAX_CELLS]`, with `MAX_CELLS` 64 or 4096 by feature. The heap quota defaults to 16 MiB per cell. There is no frame or guest-RAM budget. |
| Hypervisor and domain memory | `address_space.rs`, `stage2.rs`, `hypervisor/registry.rs` and the x86 SVM/VMX registry have no charge. Guest RAM is unbudgeted: the Chrome guest takes about 1.5 GiB. VMs are keyed by owner tid (`arch/aarch64/vm.rs:70`), not by incarnation. One owner can take all 255 AArch64 VMIDs (a Phase 16 residual). |
| Tag allocators | Three: private-root `LIVE_ASIDS` (`address_space.rs:1335`), the x86 SVM `NEXT_ASID` (never reused), and the AArch64 `VmidPool`. |
| Shared root | Stacks and grants sit at VA = PA. Free frames are kernel RWX on RV64/AArch64 (Phase 16 kept that invariant). Image bases come from the `va_alloc.rs` bump/slot allocator in a fixed order. `kaslr_cell_base` has no caller. |
| x86 SMEP/SMAP/UMIP | Cargo features exist (`kernel/Cargo.toml:181-183`). Only the IDT-protection lane builds with SMEP and UMIP. SMAP is on in no build, and the kernel has no `stac`/`clac` access window. |

## 2. Staging (proposed: two parts)

**17a, charging (lower risk):**
1. **Census.** Read only; produces `research/p17-census.md`. It lists every allocation site that relies on the ambient cell, every allocation in interrupt context, every guest-RAM and table allocation, and the three tag allocators. It also checks assumption 1 (whether any driver cell treats a stack or grant VA as a physical address).
2. **`Charge { cell, generation }` is explicit data** on heap and frame reservations, and `FrameRun` carries it. The 20 pin-to-0 sites become explicit kernel or owner charges, and ambient charging is deleted. Spec 02 §2 follows.
   - New fixture: an allocation made in interrupt context while Cell A runs is not charged to A.
3. **Guest RAM, EPT/NPT/Stage-2 and domain page tables are charged** to the owning incarnation. VMs are keyed by `(cell, generation)`. There is one tag allocator with a per-owner limit, which closes the 255-VMID residual.
   - New fixture: a VM whose guest RAM exceeds its owner's budget is refused before anything is mapped.

**17b, shared-root hardening (higher risk, each step its own lane cycle):**

4. Free frames are kernel RW+NX; boot on all three arches.
5. A VA allocator for stacks, then for grants (no more VA = PA). The driver audit from step 1 comes first.
6. SMEP, then SMAP with the `arch::user_access` window as the only place AC is set, then UMIP. The IDT-protection lane is extended: a kernel dereference of a user address outside the window must fault.
7. Slot order is randomised from kernel entropy. `kaslr_cell_base` is wired up or deleted.

The working mode is the same as S7c: a worktree, one commit per step, a rebase before the merge, nothing pushed. An independent review runs after 17a and after 17b.

## 3. File set

- **Create:** `research/p17-census.md`, `reports/phase-17-evidence.md`, the two fixtures, and a VA arena module beside `loader/va_alloc.rs` if one is needed.
- **Modify:**
  - `kernel/src/memory/**`, `kernel/src/loader/{va_alloc,elf_prepare}.rs` and the loader's frame users;
  - the 12 pin-to-0 files (`ipc/{ipc_wire,pending_mailbox,pipe}.rs`, `task/{async_ipc,completion}.rs`, `task/syscall/spawn.rs`, `sched/{scheduler,hart_local}.rs`, `task.rs` and the rest from the census);
  - `kernel/src/hypervisor/**`, `kernel/src/arch/*/{hypervisor,memory,vm}*`, `kernel/src/arch/x86_64/{boot,cpu}*` (CR4 bits), `kernel/src/task/user_copy/**`;
  - `hal/arch/x86/src/x86_64/**` (SMAP window), `hal/arch/arm/src/aarch64/stage2_regs.rs` (VMID pool owner limit);
  - `kernel/Cargo.toml`, `scripts/assemble-x86-image.py` and the x86 profile manifests (default features), `scripts/build-x86_64-idt-test-ci.sh` and its lane, `scripts/lanes.toml`, `scripts/mutable-globals.toml`, `kernel/residents.toml`;
  - `docs/specs/{02-memory,19-hardware-isolation-layers}.md`.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `docs/project-changelog.md`.

`libs/abi` and `libs/types` do not change: no syscall number or wire byte moves. A VM refused for exceeding its budget returns the existing `OutOfMemory`. There is no Law-1 checkpoint unless the census finds that the budget needs a manifest field, in which case work stops and comes back to you.

## 4. Behaviour to expect

- **Quota accounting changes by design.** Kernel bookkeeping is no longer billed to whichever cell happens to be current. Cells may see lower usage, and VM owners see guest RAM charged. Limits are re-measured and recorded, not tuned silently.
- **Hardened x86 builds** fault on a stray kernel access to user memory. A latent bug that was silent before becomes a fault, and each one found is reported.
- **Image, stack and grant addresses** change between boots once randomisation and the VA allocator land.

## 5. Risks and conflicts

- **Highest risk:** dropping VA = PA breaks any driver that leaked that assumption into DMA. The audit runs first, stacks land before grants, and each step can be reverted on its own.
- **Missing a pin-to-0 site** would mis-charge silently. The census lists each one, and the interrupt-context fixture covers the class.
- **Intel WIP plans:** the iGPU large-memory work (`AnonMemory = 440`, CP1 approved) and the XSAVE work (R5) touch the allocator and the x86 CPU setup. S8 holds those files while it runs. The `cellos-s3-int` and `cellos-s3-rfix` worktrees belong to the closed x86 breadth S3, and their files do not overlap.

## 6. Questions to the owner

1. Grant S8 Phase 17 staged as in §2, continuing without stops as S7c did. It would stop only on a behaviour difference beyond §4, or if an ABI or manifest change is needed.
2. **Guest-RAM budget.** Proposed: a separate per-cell guest-memory budget beside the heap and DMA counters, with a default of 0 and the hypervisor cell's budget set at launch to cover its largest profile (the Chrome guest, about 1.5 GiB). The alternative is to charge guest RAM to the existing 16 MiB heap quota, which every VM would exceed.
3. **Part 17b.** Run it in this phase as planned, or split it into its own phase (17b) with a separate handoff after 17a closes.
4. **x86 hardening defaults.** Turn SMEP, SMAP and UMIP on in every x86 profile once their lanes pass, with the features kept as off-switches for one cycle (proposed), or keep them opt-in.
