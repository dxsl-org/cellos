# 2026-09-27 — Kernel architecture repair, phase 01 + phase 02 slice 1

## What happened
Executed the first two increments of `.agents/260927-0739-kernel-architecture-repair/` under
`hc-cook` (plan-path, interactive, gate per phase), each with a red→green witness and a full
RV64 regression sweep.

Phase 01 (commit `e30a2a53d`): fail-closed containment for the three unqualified paths —
domain-backed zero-copy grants (all six entry points plus `GrantDma`, per-op alloc-safe
sentinels, live-record preflight at the publication point), non-RV64 Tier-2 admission
(`switch_ordering_qualified()` + `enable_for_boot` refusal + boot-log posture), and warm
snapshot (`snapshot-qualified` build gate, capture refuses before its first write, stale
header invalidated on a gated image). `tier2-smoke` and the AArch64/x86 Tier-2 integration
legs now assert the refusal instead of the retired positive behaviour; new boot fixture
`S22-RV64-GRANT-GATE` drives the public `handle_syscall` path with a real domain task.

Phase 02 slice 1 (commit `11f4258cd`): `AsidLease` no longer masks a monotonic counter into
the tag space. Tags come from a bounded live pool (256 slots, value `slot + 1`, tag 0 for
full-flush mode), a slot returns to the pool only after local + remote invalidation for that
value, exhaustion refuses the domain (`OutOfMemory`, no SAS fallback), and the width guard
keeps values inside the narrowest supported tag width. Witness `S22-RV64-ASID-LEASE`
(runner case `asid-lease`).

## Decisions
- Deny at the operation that publishes, not at the hint: `GrantShare`'s target tid is only a
  domain while it is live, so the check that matters is at `GrantSlice`/the recorded owner.
- The live-domain-grant preflight must run before `publish_prepared` takes `SCHEDULER`;
  inside `evaluate_domain_admission` it self-deadlocks on the first domain launch (caught by
  the review pass, fixed before delivery).
- Tests that asserted a retired positive behaviour assert the refusal instead of being
  relabelled or skipped; the positive RV64 Tier-2 legs (`posix-shim-test`, fault containment)
  stay positive.
- Phase 02's remaining slices are not half-built: the non-RV64 switch, the PTE-reclaim
  ordering and the x86 PCID runtime gate stay open with their gates closed.

## Lessons
- A kernel spinlock turns lock-order mistakes into instant hangs: check "which lock does my
  caller hold?" for every new function before writing its body.
- `debug_assert!` disappears in release, so a `const fn` read only by one becomes dead code
  under `-D warnings`; the width guard is a real branch for that reason.
- `gen_disk.ps1` runs under PowerShell Core on Linux (two optional cells, `doom` and
  `tetris-lua`, do not build here); `scripts/build-aarch64-test-hooks-ci.sh` is already broken
  on the unmodified tree and is not wired into CI.

## Next steps
Phase 02 slice 2 (retain detached table/leaf frames until the target root's invalidation
completes), then slice 3 (non-RV64 ordered switch + incoming completion + generation-tagged
ack) and slice 4 (x86 PCID/INVPCID runtime gate). Phase 03 replaces the phase-01 grant
refusal with the real lifecycle; the AArch64/x86 admission gate reopens only after slice 3 is
proven on one CPU.
