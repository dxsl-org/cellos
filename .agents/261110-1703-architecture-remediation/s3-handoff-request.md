# S3 handoff request — incarnation authority (phase 07)

**Date:** 2026-11-11 · **Plan:** [plan.md](plan.md) · **Prior slice:** S2 closed 2026-11-11 (commits `b78d47f31`, `d14b69266`, `7dc57d730`, `f3cd96072`). S1 is open only for the post-CI untrack and the signing-blocked release item. Neither touches S3 files.

## 1. Scope

Phase 07 builds one authority record per Cell incarnation `(cell_id, generation)`. Every capability gate and service-flag attestation reads it, and `CapRevoke` writes it. This closes review H-1: threads keep copies of revoked authority, and CSpace is a second truth that is written but never read.

Measured surface on `43438b22c`:
- The TCB has 10 `*_cap: Option<…>` fields.
- `CapSet::of_task`, `from_task` and `apply_to` have 23 call sites.
- `CellAuthority.ceiling` and `CellAuthority.current_rights` (`cell/cspace.rs:387-388`) are set in two constructors and never read.
- The `CapRevoke` handler is at `task/syscall.rs:7075`.

Owner decision D2 (Phase 00 default) stands: `CellAuthority` in `cell/cspace.rs` becomes the authoritative record.

## 2. File set (file-bounded exception)

- Modify:
  - `kernel/src/cell/{cspace,cap_registry,hotswap,service_registry}.rs`
  - `kernel/src/task/{cap,tcb,scheduler,syscall,launch}.rs`
  - `kernel/src/task/thread_cap_selftest.rs`
  - `kernel/src/resource_registry.rs`, `kernel/src/audit.rs`
  - `kernel/src/loader/governed_spawn.rs`
  - every kernel test module that builds tasks with caps; these are named in the evidence as they are touched
  - `scripts/mutable-globals.toml` and `docs/generated/mutable-globals.md`
  - `docs/security-model.md`, `docs/specs/25-cell-kernel-authority-contract.md` (status text only)
- Delete: `kernel/src/cell/registry.rs` (`CELL_REGISTRY` is unused), and the `CapSet` constructors that duplicate one another.
- Shared with every phase: this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`, `docs/code-metrics.generated.md`.
- No `libs/api` or `libs/types` change. No syscall number, trailer layout or wire byte moves. If one turns out to be needed, that part stops for CP1.

## 3. Behaviour changes the owner should expect

- **Revoke becomes Cell-wide.** `CapRevoke` on any thread removes the right from the root and from every sibling, atomically. A new thread never gains more than the record holds.
  - A consumer that relied on a thread keeping a revoked right is a bug. It goes on a list; it is not preserved.
- **Audit records** carry `(cell, generation, tid)`, and admission denials are logged against the caller.
- **The set of service-right bits carried in `CapSet` is frozen.** It is documented, and no new bits are added.

## 4. Irreversible or externally visible effects

None persistent. This is a kernel-internal, source-only change; rollback is a revert.

## 5. Conflicts

- iGPU DMA-export slice (approved, not effective): it touches `kernel/src/task/syscall.rs` and `libs/api/src/abi/syscall.rs`. Granting S3 the lane keeps it waiting again.
- S3 edits many regions of `syscall.rs` (every gate), so the two cannot run side by side.

## 6. Checkpoint

Phase 07 is a checkpoint phase. Required:
- the before-result of the revoke-on-a-worker selftest on the current tree;
- kernel host tests, plus both feature sets;
- clippy on 3 architectures;
- the full local lane set: x86 boot and integration lanes, two-node C2C, the 2-CPU x86 domain lane, lifecycle, RV64 and AArch64;
- an independent `haily-reviewer` pass on the diff.

The phase is committed as several local commits (record handle in parallel with an agreement assert, then gates, attestation, revoke, and field removal), following the owner's per-phase commit choice. Nothing is pushed.

## 7. Questions to the owner

1. Grant S3 the lane with the §2 file set?
2. Should the commits be split per step as in §6, or squashed into one commit per phase as in S2?
