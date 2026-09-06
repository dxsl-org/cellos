---
phase: 3
title: "Remove False grant_to API"
status: completed
priority: P2
effort: "1h"
dependencies: []
tier: fast
---

# Phase 3: Remove False grant_to API

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs. On a contract-breaking edge case, choose the smallest reversible option, log it, and stop rather than inventing capability delegation.

## Overview

Remove the unused `CapTable::grant_to` pseudo-delegation API and its dead depth bookkeeping because it creates an unusable parked file placeholder instead of sharing the source resource.

## Requirements

- Functional: delete `grant_to` without a replacement, shim, alias, transfer helper, or new capability variant.
- Functional: delete `CapEntry::grant_depth` and `MAX_GRANT_DEPTH`, which have no remaining purpose once `grant_to` is gone.
- Non-functional: retain capability allocation, ownership verification, lazy lease expiry, explicit/all-owner revocation, and file park/unpark behavior exactly as-is.
- Documentation: remove the living guide’s false “Grant Chains” claim; retain historical changelog entries as historical record.

## Architecture

`grant_to` (`kernel/src/cell/cap_registry.rs:120-167`) claims same-resource delegation but inserts `CapResource::File { file: None }` (`:156-164`). `park_file` treats `None` as a parked/in-progress resource and fails (`:219-235`), so the returned cap cannot satisfy the advertised contract. Repository-wide search finds no caller; all `CapEntry` construction is local to this file. The remaining live lifecycle is `alloc`/`alloc_with_lease` -> `verify` -> `park_file`/`unpark_file` -> `revoke` or `revoke_all_for`.

## Assumptions

None — declaration, constructors, consumers, and living documentation were searched and read directly.

## Related Files

- Modify: `kernel/src/cell/cap_registry.rs`
- Modify: `docs/hotswap-guide.md`
- Intentionally unchanged: `kernel/src/task/syscall.rs`, capability ABI/service APIs, `docs/project-changelog.md`, archived `.agents/**` plans

## Implementation Steps

1. Delete the complete `CapTable::grant_to` method and its comments, placeholder resource insertion, unused parent-owner read, and depth-decrement logic.
2. Delete `CapEntry::grant_depth`, its delegation comments, `MAX_GRANT_DEPTH`, and the field initialization in `alloc`; do not change cap ID allocation or wrap handling.
3. Confirm imports remain needed by `verify` and `park_file`; remove only imports made unused by the deleted block.
4. Remove the entire “Grant Chains” section from `docs/hotswap-guide.md:124-135`, including the nonexistent `alloc_with_grant_depth` promise. Join the surrounding sections cleanly without replacing it with future-design prose.
5. Check every live `CAP_TABLE` consumer still compiles against allocation, parking, unparking, and revocation; do not modify those callsites.
6. Clean-cutover check: `git grep -nE '\b(grant_to|grant_depth|MAX_GRANT_DEPTH)\b' -- kernel/src docs/hotswap-guide.md` must return no matches. A repository-wide search may still find dated changelog or archived-plan prose; those are intentionally unchanged and must not be treated as supported API.

## Commit Contract

1. Source commit: only `kernel/src/cell/cap_registry.rs` removal.
2. Documentation projection commit: only delete the false living-guide section. Run compile/search verification against the source commit before projecting docs; do not combine either with delegation implementation.

## Regression Commands

```bash
cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu
cargo check -p cellos-kernel -p service-hypervisor -p service-vfs --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
git grep -nE '\b(grant_to|grant_depth|MAX_GRANT_DEPTH)\b' -- kernel/src docs/hotswap-guide.md
```

The final `git grep` succeeds by producing no output (exit status 1); any live-code or living-guide match is a failed cutover. No new unit test should recreate or imply a delegation contract merely to test its deletion.

## Completion Evidence

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` — exit 0; 88 passed, 0 failed.
- `cargo check --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `cargo clippy --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` — exit 0.
- `cargo build --release -p cellos-kernel -p app-shell -p app-sys-tools -p app-bench -p supervisor -p hotswap-demo-v1 -p hotswap-demo-v2 -p service-hypervisor -p service-vfs --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `git grep -nE '\b(grant_to|grant_depth|MAX_GRANT_DEPTH)\b' -- kernel/src docs/hotswap-guide.md` — exit 1, expected no matches. The remaining `grant_depth` text is confined to a dated historical changelog entry and remains intentionally unchanged.
- Review verdict: **CORRECT / safe to ship**, confidence 0.98, zero findings; full diff and lifecycle inspection confirmed the removal leaves allocation, leases, ownership, revocation, and file park/unpark behavior intact, and the living guide is accurate.

## Success Criteria

- [x] `grant_to`, depth state, placeholder cap construction, and their advertised living documentation are absent.
- [x] No Rust caller or `CapEntry` constructor expects removed fields.
- [x] Kernel host tests and bare-metal kernel/hypervisor/VFS consumer checks pass.
- [x] `alloc`, lease expiry, ownership checks, revoke paths, and file park/unpark remain source-identical except for mechanically necessary surrounding cleanup.
- [x] No capability ABI, syscall, resource-sharing, writable-cap, VIFS, or lease-ledger feature is introduced.

## Security Considerations

An unusable cap that appears delegated is an authority-confusion hazard. Removal is fail-closed: no target receives a misleading token, and no new cross-cell authority path is created. Existing owner and lease checks must not be weakened while deleting adjacent fields.

## Risk Notes

The method is unused, but `CapEntry` is public within the crate; exhaustive bare-metal compilation catches hidden struct-literal assumptions. Do not “fix” delegation in this slice: safely cloning or sharing `dyn ViFile` needs a separately approved resource-ownership design.

## Documentation Trigger

Triggered now because `docs/hotswap-guide.md` presents `grant_to`, `grant_depth`, and a planned helper as implemented behavior. Delete that section in the separate projection commit. Leave the dated project changelog untouched so historical claims are not rewritten.

## Deviation Log

None.
