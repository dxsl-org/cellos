---
phase: 2
title: "Live POSIX Documentation Path Repair"
status: completed
dependencies: []
tier: fast
---

# Phase 02: Live POSIX Documentation Path Repair

## Context Links

- [Master plan](plan.md) · [POSIX research](research/posix-sequence.md)
- `libs/api/src/services/posix.rs:1-43`
- `docs/FAQ.md:78-83`
- `docs/guides/tier1b-c-zig.md:22-25,44-57`
- `docs/specs/05-application.md:120-161`
- `cells/tests/posix-shim-test/src/main.rs:1-9`

## Overview

Repair only live navigation and factual bounded-feature prose. This independent
lane changes documentation/comments and verification evidence, not runtime
behavior, exports, tests, or ABI.

## Key Insights

- The monolith path was replaced by the split `services/posix.rs` root and `services/posix/*` modules.
- The live application spec's 482-line count and “entropy/network to add” claims contradict current exports and smoke coverage.
- Historical changelog, legacy roadmap, dated research, and prior `.agents/` plans are records, not live navigation.

## Requirements

- Owner: Documentation Owner; the POSIX Module Owner reviews factual function claims.
- Replace live root references with `libs/api/src/services/posix.rs`; use `services/posix/sysio.rs` only where text names raw syscall/C I/O shims.
- Remove the obsolete line count; mark only currently evidenced entropy/network functions implemented.
- Preserve process, mmap, dynamic-linking, SAS, and non-Linux/non-POSIX-complete limitations.
- Do not change source behavior, module layout, re-exports, feature flags, tests, changelog history, or roadmap history.

## Architecture

There is no runtime architecture change. **02A** commits the four live-doc edits. A clean checkout of exact 02A commit/tree runs the searches; only then **02B** appends a current `[Unreleased]` verification line and hashed text report binding that commit/tree, commands, and results.

## Related Code Files

- Modify: `docs/FAQ.md`
- Modify: `docs/guides/tier1b-c-zig.md`
- Modify: `docs/specs/05-application.md`
- Modify comment only: `cells/tests/posix-shim-test/src/main.rs`
- Read/review only: `libs/api/src/services/posix.rs`, `libs/api/src/services/posix/*.rs`
- Create after clean verification: `docs/evidence/posix-live-path-repair-verification.txt`
- Modify after clean verification: current `[Unreleased]` section of `docs/project-changelog.md`
- Exclude: historical changelog entries, `docs/project-roadmap-legacy.md`, dated research, `.agents/`

## Implementation Steps

1. Confirm the four live locations and current split-module exports still match
   this lane's inventory; no Phase 01 ledger state is an entry gate.
2. Update all four live locations, keeping terminology consistent with the split module and bounded Tier-1b C shim; compare the function table with actual exports and correct only proven stale entropy/network/path/line-count text.
3. Commit the owned live-doc changes as 02A, then verify a clean checkout of that exact commit/tree.
4. Run `git grep -n 'libs/api/src/posix.rs' -- docs/FAQ.md docs/guides/tier1b-c-zig.md docs/specs/05-application.md cells/tests/posix-shim-test/src/main.rs`; expect no match.
5. Search the same files for `482 lines`, `Cần thêm`, `POSIX-complete`, and `Linux compatibility`; require stale claims absent and explicit limitations retained. Capture commands, results, tested commit/tree, and report SHA-256/size.
6. Only after success, commit 02B with the text verification report and current `[Unreleased]` binding. Any contradiction/failure is repaired and reverified within this lane; it does not block another lane.

## Todo List

- [ ] Confirm the live-reference inventory and current exports.
- [ ] Repair four live references and stale bounded-feature prose in 02A.
- [ ] Verify a clean checkout of exact 02A commit/tree.
- [ ] Confirm no live deleted path/obsolete future marker remains and limitations remain.
- [ ] Commit the hashed verification report/current changelog binding as 02B.

## Success Criteria

- [ ] Targeted deleted-path search returns no live match in the four owned files.
- [ ] Every new path resolves to the existing split module/submodule.
- [ ] Entropy/network wording matches implemented symbols and existing smoke markers without claiming broad POSIX/Linux support.
- [ ] Diff contains no runtime, ABI, test, historical-changelog, legacy-roadmap, or prior-plan change.
- [ ] Current verification report/changelog names exact tested 02A commit/tree, literal searches/results, and evidence SHA-256/size.

## Risk Assessment

- Blind global replacement could rewrite history or point low-level descriptions at the wrong module. Restrict paths to the owned files and review each use.
- Updating status prose could overclaim completeness. State only named implemented functions and retain explicit unsupported categories.
- Rollback/rework stays within 02A/02B; any failed search blocks only this lane.

## Security Considerations

Documentation must not imply that the shim supplies Linux isolation, full process semantics, or a supported API beyond the frozen Cell contract. Security-reporting text in the FAQ is unrelated and untouched.

## Assumptions

- **Claim:** The four identified files are the complete live-reference set. **Confidence:** high. **How to verify:** targeted `git grep`, classifying all other matches as historical before editing.
- **Claim:** Entropy and network symbols remain exported. **Confidence:** high. **How to verify:** inspect `services/posix.rs` module wiring and named submodule exports.

## Next Steps

This lane completes when 02A and 02B are accepted. Its failure does not block
the shell, fstat, rename, AArch64, or x86 lanes; no ABI or shell work is bundled
here.
