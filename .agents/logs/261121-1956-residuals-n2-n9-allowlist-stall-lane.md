# 2026-11-21 — Residuals N-2..N-9 closed, allowlist open items, virtio-blk stall lane

## What happened
Closed every Phase 19 `N-` residual of the 2026-11-10 architecture review:
- N-9 was already fixed by the 11-19 CI repair; only the record was updated.
- N-2..N-7 were fixed with owner decisions, using three parallel implementer agents on disjoint files.

`B-AARCH64-SEMHOSTING` was renewed under #49 (expires 2026-12-21T03:53:07Z). Every open item in `scripts/unsafe-allowlist.toml` was closed. A QEMU `blkdebug` stall lane now proves the N-3 deadline/poison path. CI on `main` is green (KVM job skipped: no runner).

## Decisions
- N-4: retire Spec 00 Law 5 and the Law 6 Vi-ownership clause (ADR-0036); enforcing them was rejected (41 mod.rs, 38 heterogeneous traits).
- N-5: delete `abi::fs::Lease` (Law 1 double confirmation) instead of inventing a lock protocol.
- N-6: retire `GetFile`/`DataPtr` behind wire tombstones (discriminants kept) instead of a Tier-1-only restriction.
- N-3: add `flush_nb` to the vendored virtio-drivers. The owner approved the TCB unsafe ceiling raise (blk.rs 5→8). The allowlist cleanup later brought the total down to 586.
- N-7: `assert_eq!` (kernel panic) at the HAL leaf. No task or error path exists there, and continuing would run under the wrong root.

## Lessons
- `cellos-sign` scans tracked files only: new untracked files make the Tier-1 total look lower until committed. Simulate with a temp `GIT_INDEX_FILE`.
- The vendored virtio-drivers test suite had never compiled (redundant dev-dep, `deny(missing_docs)` on the fake transport, UB in the sound fake).
- A failed shell write alone does not prove the driver deadline: VFS cuts the driver off at 35 s anyway. The stall lane checks the poison line and virtqueue `last-avail-idx`/`inuse`.
- The development-Silo lane needs `CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO=1` for the build script too, not only the runner.
- Docs-only pushes skip CI; regenerate `docs/generated/arch-status.md` and `docs/spec-status.generated.md` before pushing lane/spec edits.

## Next steps
- Renew `B-AARCH64-SEMHOSTING` around 2026-12-14.
- Portfolio residuals in `docs/roadmap/current-focus.md` § Architecture remediation plan closes (H-2/5/8/13/14, M-*, Phase 16–18 carries, security-audit re-triage).
