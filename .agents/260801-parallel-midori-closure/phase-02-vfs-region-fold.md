---
phase: 2
title: "Fold /bin/vfs Cell-Store Region"
status: complete
priority: P1
effort: "1d"
dependencies: []
tier: thinking
---

# Phase 02: Fold /bin/vfs Cell-Store Region

## Overview

Complete the remaining Midori Phase 04 `/bin/vfs` fold steps c-e without starting spawn-broker ABI work. The goal is to remove the post-policy raw block-region grant after policy and ceiling can express bit 3.

## Requirements

- Functional: `/bin/vfs` obtains block region `0b1111` through manifest/path request intersected with boot ceiling and policy, not a post-policy raw grant.
- Non-functional: No broker, no shell deprivilege, no service ID work.

## Architecture

Prerequisites verified:
- Policy parser accepts 4 block-region bits: `kernel/src/policy.rs:68`, `kernel/src/policy.rs:69`.
- Policy signer accepts 4 block-region bits: `scripts/sign-policy.py:57`, `scripts/sign-policy.py:58`.
- Boot ceiling already grants `/bin/vfs` `0b1111`: `kernel/src/loader/boot_ceiling.rs:33`, `kernel/src/loader/boot_ceiling.rs:39`, `kernel/src/loader/boot_ceiling.rs:79`.

Remaining flow: `CapSet::with_path_caps("/bin/vfs")` requests bit 3, loader intersects request with spawner ceiling, `policy::apply` preserves bit 3 from `/POLICY.BIN`, and the old raw grant at `kernel/src/loader.rs:349` disappears.

## Assumptions

- **Claim:** Generated POLICY.BIN is actually baked into the test-hooks and boot images used by the runtime lane.
  **Confidence:** medium
  **How to verify:** inspect build scripts and `inspect_fat` output before removing raw grant.

## File Ownership

- Owns: `kernel/src/task/cap.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs` self-tests, `scripts/sign-policy.py`, relevant build scripts that bake `/POLICY.BIN`, and targeted tests.
- Does not touch: `libs/api/src/abi/syscall.rs`, `cells/tools/shell/*`, `cells/services/spawn-broker/*`.

## Implementation Steps

1. Create worktree: `git worktree add .worktrees/midori-vfs-region-fold -b codex/midori-vfs-region-fold`.
2. Change `scripts/sign-policy.py` `/bin/vfs` entry from `0b111` to `0b1111`; add host-side parse assertion.
3. Fold `/bin/vfs` bit 3 into the request path in `CapSet::with_path_caps`.
4. Update policy/kernel self-tests expecting `/bin/vfs` `0b1111`.
5. Remove loader raw grant only after self-tests and policy parsing prove bit 3 survives.
6. Add a runtime assertion or serial marker proving live VFS task has `block_regions == 0b1111`.
7. Run VFS and `/srv` suites before and after raw-grant removal.

## Success Criteria

- [x] `/bin/vfs` policy entry is `0b1111`.
- [x] `CapSet::with_path_caps("/bin/vfs")` requests cell-store bit 3.
- [x] `kernel/src/loader.rs` no longer post-policy ORs `0b1000`.
- [x] Live VFS task reports `block_regions == 0b1111`.
- [x] RedoxFS `/srv` and VFS test-hooks suites pass.

## Security Considerations

Raw post-policy grants bypass the operator-policy model. Removing it is security-positive only if policy and ceiling preserve the exact same authority first.

## Risk Notes

- Risk medium likelihood, high impact: wrong policy bake can make fleet `DenyAll`. Mitigation: host parse test before image bake; raw grant deletion last.
- Rollback: restore signer entry, remove `with_path_caps` fold, and reinstate loader raw grant.

## Deviation Log

Evidence report: [midori-vfs-region-fold-report.md](./reports/midori-vfs-region-fold-report.md).

Branch-complete in the worktree; merge gate remains outside this phase file.
