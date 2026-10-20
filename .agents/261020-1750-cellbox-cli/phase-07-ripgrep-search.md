---
phase: 7
title: "Ripgrep-derived VFS search and structured matches"
status: pending
priority: P1
tier: thinking
dependencies: [3, 4]
---
# Phase 07: Ripgrep-based search

## Overview and requirements

Deliver CBX-19/20 using upstream search components, a bounded Cellos I/O path and direct records. Keep existing grep's contract distinct; renaming bounded ERE-lite matching is not this phase.

## Architecture and related files

Add search modules/dependencies under `cells/tools/cellbox`; retain reusable pure matcher code only where compatible. Adapt admitted VFS clients/read/directory-handle APIs in `libs/ostd` or the selected runtime under P01's reviewed scope. Inspect `libs/text-engine/src/grep*` for reuse boundaries, not as a substitute for the selected ripgrep functionality. Update Cargo lock/notices and command docs for pinned imports.

## Implementation steps

1. Use P01's pinned upstream matcher/searcher/ignore feature set and runtime route. Do not require mmap/PCRE2 or unconstrained parallel walks. If target dependencies fail, resolve the exact approved prerequisite rather than silently switching to custom grep.
2. Implement directory/file traversal with scoped handles, bounded chunks, explicit ignore/hidden/binary/symlink semantics and cancellation. Do not cross an unauthorized mount/handle boundary or loop through links.
3. Emit match/context/summary data from search callbacks into typed records. Define path representation, 1-based line number, absolute/submatch byte offsets and byte-versus-UTF-8 content without colon or color reparse.
4. Retain the P01-frozen flags/status surface and truthful access/partial traversal errors. Encode non-UTF-8 content explicitly at JSON boundaries; text rendering may safely escape data but cannot silently alter machine records.
5. Feed record and byte inputs only according to declared signatures; reject unsupported input types. On early `take`/consumer close, stop reading/traversing and release handles.
6. Exercise admitted VFS fixtures larger than the old 64 KiB aggregate bound, mixed ignore/hidden/binary/UTF-8 cases, inaccessible entries and symlink cycles where supported. Compare selected upstream-compatible behavior against a pinned host fixture with identical semantics.

## Success criteria

- [ ] Actual target search finds correct byte ranges/lines over multi-chunk files with bounded memory.
- [ ] Hidden/ignore/binary/link behavior matches the published surface and access denial is reported without privilege bypass.
- [ ] Match/no-match/error statuses follow documented 0/1/2 policy and partial errors are not silently successful.
- [ ] Typed output has no ANSI or lossy byte conversion; cancellation stops actual producer work.

## Verification

Use deterministic matcher/traversal/encoding/error tests for meaningful edges and a throwaway real target tree/pipeline smoke. Measure memory and time on identical fixtures; do not import upstream speed claims. Clean fixture files only after preserving required evidence.

## Assumptions

Pinned std/no_std/runtime boundaries and VFS stream traversal fit the admitted profile. Verify target compilation and actual I/O; host rg JSON smoke is only reference behavior.

## Security and risk assessment

Bound pattern/input/retained result complexity and enforce per-resource scope. Escape control characters only in display to prevent terminal injection. Rollback restores matched code/catalog/dependency/image; search itself is read-only but output may disclose file contents, so authorization is checked before traversal.

## Deviation log

None; implementation pending. Record any dependency/compatibility/scope change with reason/impact/revert and escalate a required surface reduction.
