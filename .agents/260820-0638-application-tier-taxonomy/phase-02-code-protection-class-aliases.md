# Phase 02 - Code Protection-Class Aliases

## Context Links

- `libs/api/src/abi/manifest_flags.rs`
- `libs/api/src/abi/manifest.rs`
- `libs/api/src/abi/manifest_macro.rs`
- `libs/api/src/abi/manifest_parse.rs`
- `libs/ostd/src/runtime.rs`
- `kernel/src/task/cap.rs`
- `kernel/src/loader.rs`
- `kernel/src/task/manifest_v2_selftest.rs`
- `kernel/src/loader/elf_tests.rs`
- `libs/zig-syscall/src/manifest.zig`

## Overview

Priority: P2. Status: completed. Effort: 4h. Tier: thinking.

Rename code-facing meaning from "tier" to "protection_class" at the API
surface without changing Manifest v2 layout, bytes, constants, or loader
behavior. This is a terminology migration only.

## Key Insights

- OBSERVED: Manifest v2 field `tier` is an x86 PKU protection-key request, not
  an app execution tier.
- OBSERVED: `granted_tier()` enforces a floor; higher numeric values mean more
  isolation and less authority.
- PRIOR: `libs/zig-syscall/src/manifest.zig` may still carry a v1/8-byte
  contract; verify before editing.

## Requirements

- Functional: add `PROTECTION_CLASS_*` aliases and `protection_class()` accessors.
- Non-functional: preserve `TIER_*`, `.tier`, `tier()` and macro `tier = ...`
  forms for existing callers.
- Backwards compatibility: Manifest v2 remains exactly 16 bytes; v1 upcast stays.

## Architecture / Data Flow

ELF manifest bytes enter `CellManifest::from_bytes`, parse into the existing
field, flow through loader cap grant resolution, then map to x86 PKU key/value.
Only names and docs change; parsed data and PKU behavior must be byte-identical.

## Related Code Files

Modify: `libs/api/src/abi/manifest_flags.rs`, `libs/api/src/abi/manifest.rs`,
`libs/api/src/abi/manifest_macro.rs`, `libs/api/src/abi/manifest_parse.rs`,
`libs/ostd/src/runtime.rs`, `kernel/src/task/cap.rs`,
`kernel/src/task/manifest_v2_selftest.rs`, `kernel/src/loader/elf_tests.rs`.

Inspect/update if stale: `libs/zig-syscall/src/manifest.zig`.

## Implementation Steps

1. Add `PROTECTION_CLASS_TRUSTED_CORE`, `PROTECTION_CLASS_STANDARD`,
   `PROTECTION_CLASS_FFI`, `PROTECTION_CLASS_UNTRUSTED`, and
   `PROTECTION_CLASS_LEGACY` as aliases to current `TIER_*`.
2. Add `CellManifest::protection_class()` returning the same byte as `tier()`.
3. Add `granted_protection_class()` wrapper around existing floor logic; keep
   `granted_tier()` as deprecated compatibility alias.
4. Update comments and tests to describe protection class, not app tier.
5. Keep macro `tier =` form; optionally add `protection_class =` in a later
   additive macro arm after checking macro ambiguity.
6. Verify Zig manifest layout and document/update the v2 compatibility story.

## Todo List

- [x] Enumerate all `TIER_*`, `tier()`, `.tier`, and macro callers.
- [x] Add aliases without removing old names.
- [x] Update tests and self-test messages.
- [x] Verify Zig syscall manifest compatibility.

## Success Criteria

- Existing Rust callers compile unchanged.
- New code can use `PROTECTION_CLASS_*` and `protection_class()`.
- No manifest byte layout changes; v1 and v2 parser tests still pass.

## Risk Assessment

- High x High: ABI break at manifest boundary. Mitigation: no field reorder,
  no size/version bump, keep all old symbols.
- Medium x Medium: macro ambiguity if adding `protection_class =`. Mitigation:
  add only after compile check; otherwise defer macro arm.
- Undo: revert alias/comment changes; old symbols remain source of truth.
- Irreversible: none if layout is preserved.

## Security Considerations

Do not use this phase to implement Tier 2. `protection_class` is an intra-target
PKU/floor request, not a trust admission result.

## Next Steps

Phase 03 gates are complete; keep Phase 04 deferred pending separate approval.

## Evidence

- `git diff --stat` showed the expected 12-file implementation surface, including
  API aliases/accessors, loader/capability migration, compatibility tests, and
  Zig manifest updates.
- `cargo test -p api --target x86_64-unknown-linux-gnu` passed: 82 unit tests,
  2 contract tests, and 4 ignored doc tests.
- `cargo check -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`
  completed successfully; only pre-existing target-feature and kernel image
  warnings were emitted.
- The final diff retains legacy `TIER_*`, `tier()`, `.tier`, and macro forms,
  while adding `PROTECTION_CLASS_*`, `protection_class()`, and
  `granted_protection_class()`.
