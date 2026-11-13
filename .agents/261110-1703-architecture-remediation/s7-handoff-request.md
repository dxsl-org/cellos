# S7 handoff request: kernel `arch/` seam and boot split (Phase 14)

**Date:** 2026-11-13 · **Plan:** [plan.md](plan.md) · **Prior slice:** S6 closed 2026-11-13 (Phase 13 CP2 confirmed, `main` at `064732582`).

S7 covers Phases 14, 15 and 16. This request opens **Phase 14 only**. Phases 15 and 16 each get their own request.

## 1. Scope (measured on `064732582`)

| Surface | Today |
|---|---|
| `target_arch` in `kernel/src` | 1 837 lines in 102 files: riscv64 670, x86_64 645, aarch64 543, riscv32 90, arm 22, x86 (32-bit) 36 |
| `asm!` in `kernel/src` | 124 sites in 32 files |
| Direct HAL arch paths | `hal::aarch64::` 107 times, `hal::x86_64::` 36 times |
| `kmain` | `main.rs:175`, inside a 2 018-line file. "VT-d before any DMA driver" holds only because of where the code sits. |
| `yield_cpu` | `task.rs:1385`, inside a 3 084-line file. It has two switch bodies. |
| Page tables | `KERNEL_ROOT` appears 26 times in `memory/paging.rs` (raw PTE bits) and 8 times in `page_protect.rs`. `task/user_copy/sv39_probe.rs` carries its own walker. |
| HAL traits with no implementor | `Timer`, `InterruptController`, `BootController` (`hal/traits/{timer,interrupt,arch}`). `ViHypervisor` (`hal/traits/hypervisor`) has only stubs. |
| 32-bit code | `riscv32`/`arm`/`x86` cfgs in 20 kernel files (`boot.rs`, `dev/*`, `loader/*`, `memory/*`, `sched/*`, `task.rs`, …) |
| `kernel/src/arch/` | does not exist |

## 2. Staging (proposed)

1. **Census and guard.** This step adds no new code paths.
   - Write `research/p14-census.md`:
     - `target_arch` and `asm!` counts per directory;
     - a line-by-line diff of the two `yield_cpu` bodies;
     - a comparison of x86 `pte_bits_from_flags` (`page_protect.rs`) with the HAL x86 paging flags;
     - the board accessors and their callers;
     - the 32-bit sites;
     - the `PlatformReady` fields that `kmain`'s ordering needs.
   - Add a `target_arch` ratchet (counted outside `kernel/src/arch/`) to `scripts/check-hal-boundaries.sh`, set at today's numbers.
2. **Move only.** Create `kernel/src/arch/{mod.rs,x86_64,aarch64,riscv64,legacy}`. Move `asm!` sites and direct `hal::<arch>` uses file by file, with no logic edits. Each commit lowers the ratchet. 32-bit code goes to `arch/legacy` (owner decision 4).
3. **Split `kmain`.** Each arch gets its own `arch::boot::platform_init() -> PlatformReady`, with the IOMMU as a required field. The neutral tail stays in `kmain`. x86 goes first, and that arch's boot lane runs after each arch is done.
4. **`SwitchPlan`.**
   - Design it from the step 1 diff and add one `switch(plan)` per arch, plus an RAII interrupt-state type.
   - Keep the old path behind a test-posture A/B switch. An A/B fixture must show identical markers on all three arches before the old path is deleted, inside this phase.
5. **One page-table path.** The x86 `KERNEL_ROOT` goes through the HAL page-table trait. `leaf(root, va)` and `current_root()` move into the HAL. Sv39-specific names become arch-neutral.
6. **Cleanup.**
   - One `board::active()`.
   - Delete the HAL traits that have no implementor, and the `ViHypervisor` stubs.
   - Make x86_64 the default arch in `hal/core/Cargo.toml` and `kernel/build.rs`.
   - Correct Spec 04 §1/§7 and the board sentence in `docs/system-architecture.md`.
7. **Checkpoint.**
   - x86 domain lanes on 1, 2 and 4 CPUs, RV64 `--harts 2`, AArch64 `virt-vhe` with 2 CPUs.
   - The pr-tier suite on three arches, plus `x86-c2c` and a release-posture boot.
   - Kernel builds for all three 64-bit targets, with no new `allow(dead_code)` cfgs.
   - Independent review.

**Process.**
- One commit per step, or per arch within a step.
- Steps 2–6 change no behaviour.
- Work happens in a worktree, and is rebased on `main` before each merge.
- Nothing is pushed.

## 3. File set

- **Create:**
  - `kernel/src/arch/**`;
  - `research/p14-census.md`;
  - the A/B switch fixture (test-posture only).
- **Modify:**
  - kernel files:
    - `kernel/src/{main,board,platform,boot,task,dev,snapshot_engine}.rs`;
    - `kernel/src/{memory,sched,loader,dev,task/user_copy,cell}/**`;
    - only the cfg and `asm!` lines in other kernel files;
    - `kernel/build.rs`, `kernel/Cargo.toml`;
  - HAL files:
    - `hal/core/{Cargo.toml,src/lib.rs}`;
    - `hal/arch/{x86,arm,riscv}/**` (`SwitchPlan`, page probe, user-access window);
    - `hal/traits/Cargo.toml` and the workspace members list;
  - `boards/src/**`, only for the accessor;
  - `scripts/check-hal-boundaries.sh`;
  - `docs/specs/04-hardware.md`, `docs/system-architecture.md`.
- **Delete:**
  - `hal/traits/{timer,interrupt}`, and `BootController`, if they have no implementor;
  - the `ViHypervisor` stubs;
  - `boards` `aarch64_catalog()`, if it is unused.
- **All steps:**
  - this plan directory;
  - `.agents/plan-portfolio.md`;
  - `.agents/cell-contract-0024-locks.md`;
  - `docs/project-changelog.md`.

`libs/abi` and `libs/types` do not change, so there is no Law-1 checkpoint. If step 5 turns out to need an ABI change, I stop and ask.

## 4. Behaviour to expect

None by design. The risk is in the context switch and paging (steps 4 and 5). The A/B fixture and boot lanes on all three arches are the gate before the old path is deleted.

## 5. Conflicts

- **The other session in `/home/dmin/cellos`** has lately committed block drivers, Chrome guest and display work. Its kernel commits this month are the VMX host-state repair. Step 2 touches about 100 kernel files mechanically, so each step lands as its own commit, rebased just before merge, and none of that session's uncommitted files is touched.
- **The `feat/x86-s3-*` branches** are fully contained in `main` (0 commits ahead). There is no conflict.
- **Intel WIP lane:** this slice takes no file of an active Intel slice. If one opens on the kernel (`261104-0801` large-memory work), S7 pauses at the next commit boundary.

## 6. Questions to the owner

1. Grant S7/Phase 14 staged as in §2: step 1 now, steps 2–7 continuing without a further stop, except where a step finds a behaviour difference.
2. A/B switch: delete the old switch path within Phase 14, once the fixture shows identical markers on three arches (proposed). The alternative is to keep it one more phase.
3. 32-bit code: per decision 4, move it to `arch/legacy` and only build-check it (no lane exists for it). Removing it instead needs a new owner decision.
