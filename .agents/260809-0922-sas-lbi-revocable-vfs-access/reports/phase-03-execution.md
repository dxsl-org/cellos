# Phase 03 Execution Evidence

## Result

Phase 03 is complete. The approved bridge is implemented without changing
`libs/api`, `libs/types`, syscall numbers, VFS wire formats, or manifests.

## Delivered semantics

- Registered VFS `GrantSlice` installs a bounded exact lease keyed by holder,
  grant owner, grant id, and request generation before exposing the address.
- Matching `Send` releases only that lease and clears matching caller context;
  wrong-target or stale-generation completion cannot revoke a newer request.
- Owner death tombstones leased page and registered grants, quarantines frames,
  and withholds reuse until exact release. VFS holder death releases its leases.
- VFS may subscribe only to the kernel-derived owning task of its current caller.
  Publication is atomic with death under `SCHEDULER -> DEATH_SUBSCRIBERS`.
- Owner death purges directory handles, file handles, and pending reads for the
  exact `Caller { cell, generation }`; worker death does not purge cell state.
- Hot-swap performs IOMMU cleanup and acknowledges ordinary DMA pins before
  grant reap, while VFS request leases retain their exact-release lifetime.

## Final verification

- `cargo fmt --all --check`: pass.
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu`: pass
  (10 types, 75 API, 2 contract tests; doc tests had no failures).
- `bash scripts/build-test-hooks-ci.sh`: pass.
- RV64 QEMU `vfs_lifetime_selftest_passes`: pass 1/1 with
  `[selftest] VFS-LIFETIME: PASS`.
- RV64 QEMU `riscv64_vfs_quota_all_pass`: pass 1/1.
- Production kernel release builds: RV64, AArch64, and x86_64 pass.
- `git diff --check`: pass.
- Standard production review: PASS. Focused security review: PASS.

AArch64 `--features test-hooks` remains unavailable because the pre-existing
test hook references a missing `qemu_exit::AArch64Semihosting` variant. This is
not a Phase 03 gate: the production AArch64 kernel compiles, and runtime proof is
the required RV64 QEMU lane. No Pi 3 or physical RV64 evidence is claimed.

## Rollback

Revert the Phase 03 kernel/VFS/test files as one slice. Do not partially retain
`GrantSlice` lease creation without matching `Send`, death, and quarantine
cleanup. The public ABI and wire remain unchanged, so rollback requires no ABI
or manifest migration.
