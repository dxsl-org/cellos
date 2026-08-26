# Phase 03 - Verification Gates

## Context Links

- `docs/code-standards.md`
- `kernel/src/loader/elf_tests.rs`
- `kernel/src/task/manifest_v2_selftest.rs`
- `scripts/cellos-sign`

## Overview

Priority: P2. Status: completed. Effort: 3h. Tier: medium.

Prove docs consistency and source compatibility after code terminology aliases.

## Requirements

- Functional: tests cover legacy and new names.
- Non-functional: no generated docs churn outside touched files.
- Backwards compatibility: existing signed/unsigned manifest fixtures behave the same.

## Architecture / Data Flow

Source constants and macros generate ELF manifest sections; loader parses them;
tests compare layout, parse behavior, floor logic, and rejection of malformed
reserved fields. Docs grep checks verify public terminology.

## Related Code Files

Read/test only unless phase 02 requires fixture updates.

## Implementation Steps

1. Run targeted grep excluding `docs/research`, `docs/project-changelog.md`,
   `docs/project-roadmap-legacy.md`, `.agents`, and `.worktrees`.
2. Run `cargo test -p api --target x86_64-unknown-linux-gnu` for the host-testable
   manifest ABI surface.
3. Run the repository's supported manifest parser/self-test lane and
   `cargo check -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`
   if the pinned WSL/Rust environment is healthy.
4. Run `git diff --check`.
5. Run link sanity for changed docs.

## Todo List

- [x] Terminology grep.
- [x] Manifest tests.
- [x] Kernel check.
- [x] Whitespace/link sanity.

## Success Criteria

- Active docs contain no `SDK L1`.
- Active docs use `Tier 1b`/`Tier 3b` only as legacy aliases.
- Manifest size remains 16 bytes.
- No new test failure in targeted lanes.

## Risk Assessment

- Medium x Medium: WSL tooling unavailable. Mitigation: record host-gated checks
  rather than claiming them green.
- Medium x High: grep misses `.worktrees` duplicates. Mitigation: exclude
  worktrees intentionally and report scope.
- Undo: revert any test fixture changes.
- Irreversible: none.

## Security Considerations

Negative tests for malformed manifests and floor logic are load-bearing; do not
drop them during rename.

## Next Steps

All verification gates pass for the completed terminology migration; keep
Phase 04 deferred pending separate approval.

## Evidence

- Targeted terminology scan completed with exit 0. Active documentation retains
  `Tier 1b`/`Tier 3b` only as legacy/profile wording; remaining source matches
  are historical or domain-specific compatibility references.
- `cargo test -p api --target x86_64-unknown-linux-gnu` passed: 82 unit tests,
  2 contract tests, and 4 ignored doc tests.
- `cargo check -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`
  completed with exit 0. Rust emitted existing target-feature and kernel image
  warnings, but no compilation errors.
- `git diff --check` completed with exit 0.
- Manifest layout and compatibility assertions are present in the changed API
  and loader tests; no field reorder or version/size change appears in the diff.
