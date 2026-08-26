---
phase: 6
title: "Closure Verification And Rollback"
status: pending
priority: P1
effort: "1d"
dependencies: [5]
tier: fast
---

# Phase 06: Closure Verification And Rollback

## Overview

Prove the final behavior and document rollback boundaries. Hardware evidence is a post-implementation gate, not plan completion.

## Requirements

- Functional: run unit, integration, negative, QEMU RV64, and cross-arch build gates for final code.
- Non-functional: update docs/status in the same change as evidence; no false-green host-gated claim.

## Architecture

Verification data flow:
`tests -> QEMU/runtime logs -> evidence markers -> docs/status update -> rollback decision`.
Failure must fail closed: denied reads return typed errors, no silent empty data, no stale owner inheritance, no post-reuse grant writes.

## Assumptions

- Claim: RV64 QEMU test-hooks are available on the implementation host.
  Confidence: medium
  How to verify: `bash scripts/build-test-hooks-ci.sh` then integration command; if host-gated, record deferred status.

## Related Files

- Modify tests as needed: `cells/tests/vfs-test/src/dircap.rs`, `cells/tests/vfs-test/src/grant_io.rs`, `tests/integration/tests/vfs-quota.rs`
- Modify docs only after implementation evidence: `docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/specs/09-vfs.md`, `docs/specs/17-ipc-wire-contract.md`
- Read/verify: `docs/specs/18-cell-trust-tiers.md`, `docs/specs/19-hardware-isolation-layers.md`

## Implementation Steps

1. Unit tests: `Caller` generation equality; handle wrong-owner get/remove; pending wrong-owner poll; dir revoke transitivity; encode/decode discriminant stability.
2. Negative tests: cross-cell handle guessing; same `CellId` higher generation; wrong-owner close preserving entry; close/revoke during read; stale ID/epoch; sealed path; masked wrong sender; malformed reply; truncation; no `DataPtr` retry.
3. Grant/lifecycle tests: `ReadFileGrant` clamp/nonzero/deny after seal; killed caller during synchronous grant copy; if caller memory can outlive reply, require existing pin/free-refusal/quarantine/ack proof or keep path disabled.
4. Terminal tests: Exit, ForceExit, fault, watchdog, heartbeat, hot-swap, VFS death/restart, caller death, and cancellation reap all owned state; test bounded table growth and client typed error/reopen.
5. RV64 QEMU runtime: `bash scripts/build-test-hooks-ci.sh`; then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`. Preserve exact markers and tree status.
6. Compile-only kernel gates, mirroring CI: `cargo build --release -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`; repeat with `aarch64-unknown-none-softfloat` and `x86_64-unknown-none`. Record required target-specific compiler environment from `.github/workflows/ci.yml`; these do not replace runtime proof.
7. Mark Pi 3, real RV64/SMP, and any unavailable AArch64 runtime lane post-implementation hardware gates, never plan-completion conditions.
8. Rollback drill: restore the previous phase explicitly and prove no automatic per-read `GetFile` fallback exists.

## Success Criteria

- [ ] Unit and integration tests cover owner/generation, close/revoke, stale handle, sealed path, and grant bounds.
- [ ] HTTPD and net-tools HTTPD return typed errors for wrong sender, decode failure, truncation, and VFS restart instead of empty content.
- [ ] QEMU RV64 runtime markers pass on final code.
- [ ] Cross-arch build/check pass or host-gated lanes are explicitly marked deferred.
- [ ] Docs/status mention remaining boundaries: no Tier 2, no generic reactor, no async DMA, hardware evidence deferred.
- [ ] `git status --short` distinguishes implementation changes from pre-existing dirty docs.

## Security Considerations

Fail-stop/fail-closed is mandatory: malformed, unauthenticated, stale, wrong-owner, oversized, or cancelled reads must deny or return an explicit error, never fall back to `GetFile` or empty success.

## Risk Notes

- Risk Medium x High: QEMU unavailable creates false completion pressure. Mitigation: mark host-gated deferred; do not claim done.
- Risk Medium x Medium: docs drift from evidence. Mitigation: status update in same change and cite exact test markers.
- Risk Low x High: rollback leaves quarantined frames. Mitigation: leak/withhold is the safe residue; never unquarantine without ack.
- Rollback: revert final caller/API removal to Phase 02 copy-out adapter. Irreversible part: externally released ABI removal and any quarantined frame leak until ack.
- Stop condition: any terminal path fails closed only by lazy purge when immediate durable cleanup was required.

## Deviation Log

None.
