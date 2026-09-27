# 2026-09-27 — Phase 01 containment (kernel architecture repair)

## What happened
Executed phase 01 of `.agents/260927-0739-kernel-architecture-repair/` under `hc-cook`
(plan-path, interactive, gate per phase). Every grant entry point that names a private-root
task now refuses at the common syscall gate with the per-op alloc-safe sentinel (`0` for
`GrantAlloc`/`GrantRegister`, `usize::MAX` for `GrantSlice`, nonzero failures for
share/free/unregister/DMA); non-RV64 Tier-2 admission and warm snapshot are fail-closed
(`snapshot-qualified` build gate, off in every default image). No `libs/api`/`libs/types`
change. New boot fixture `S22-RV64-GRANT-GATE` drives the production `handle_syscall` path
with a real `TaskAddressSpace::Domain` task: red on the pre-gate kernel
(`h1-grant-gate-6j4WCL`), green after (`h1-grant-gate-MbNs8s`). All RV64 lanes 7/7, AArch64
denial lane 2/2, `launch-profile` 1/1, RV64 Tier-2 integration 5/5, off-feature build clean.

## Decisions
- Deny at the publishing operation, not only at share time: `GrantShare`'s target tid is only
  a domain when it is live, so the fail-closed check that matters is at `GrantSlice` (caller
  identity) and at the grant owner recorded in the table.
- The live-domain-grant preflight must run *before* `publish_prepared` takes `SCHEDULER`;
  running it inside `evaluate_domain_admission` self-deadlocks on the first domain launch
  (scheduler spinlock re-entry). Caught by the review pass, fixed before delivery.
- Tests that asserted the retired positive behaviour (`tier2-smoke` grant leg, AArch64/x86
  Tier-2 admission) now assert the refusal instead of being relabelled or skipped.
- Snapshot stays reported as unavailable with the existing SupervisorCap contract; a stale
  header is cleared by the gated image so an older image cannot replay it.

## Lessons
- A kernel `Spinlock` makes lock-order mistakes instant hangs, not rare races: check every
  new function for "which lock does my caller hold?" before writing the body.
- `gen_disk.ps1` runs under PowerShell Core on Linux; the canonical image can be rebuilt in
  WSL, but two optional cells (`doom`, `tetris-lua`) do not build here.
- `scripts/build-aarch64-test-hooks-ci.sh` is already broken (8 errors) on the unmodified
  tree and is not referenced by any CI workflow — a stale lane, not a regression.

## Next steps
Phase 02 (PTE reclaim + ordered non-RV64 switch, ASID/PCID leases) with the phase-01 gate as
the safety net; reopen non-RV64 Tier-2 admission only after the single-CPU switch proof, and
phase 03 replaces the containment refusal with the real grant lifecycle.
