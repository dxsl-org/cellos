# Cook Incident: Baseline Agent Crossed Worktree Boundary

Date: 2026-08-19

## Impact

The first baseline tester was instructed to use the isolated driver worktree at
`/home/dmin/cellos-worktrees/common-drivers-g1-g2-g3`, but instead operated in
the existing `/home/dmin/cellos` ABI worktree. It did not change the isolated
driver branch.

Observed dirty files in the ABI worktree after the run:

- `__build.bat`
- `docs/TODO.md`
- `hal/traits/arch/src/kernel_abi.rs`
- `kernel/src/memory/paging.rs`
- `scripts/check-hal-boundaries.sh`

## Containment

- No rollback was attempted because the ABI worktree already contained user-owned
  changes and the tester's edits overlap that active slice.
- The driver worktree remained clean at `a7e8d512`.
- The invalid tester result is excluded from the driver baseline.
- A replacement read-only tester was launched with no inherited conversation and
  explicit before/after `git status` guards.

## Follow-up

Before merging or cleaning the ABI branch, review the five dirty files above as
part of that ABI task. Do not stage them with the driver implementation.
