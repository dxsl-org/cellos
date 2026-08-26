---
title: "D15-D17 Rulings Cleanup"
description: "Apply approved D15, D16, and D17 documentation/comment corrections without runtime or ABI changes."
status: completed
priority: P2
effort: 4h
branch: feat/wx-post-reloc-and-f1-signing
tags: [docs, tech-debt]
created: 2026-08-01
---

# D15-D17 Rulings Cleanup

## Scope Contract

Approved rulings: D15=A, D16=A, D17=A. Implementation must update only docket/reports/specs/living docs plus the three stale comment groups in:

- `kernel/src/task.rs`
- `cells/services/input/src/dispatcher.rs`
- `cells/services/vfs/src/page_cache.rs`

No runtime logic, syscall numbers, ABI layouts, public enums, wire formats, tests, Cargo metadata, or generated artifacts may change.

## File Ownership

- **Phase 1 owns docs only:** `.agents/reports/decision-docket-260730.md`, `.agents/reports/d15-input-delivery-contract-analysis-260801.md`, `.agents/reports/d16-cluster-status-ownership-analysis-260801.md`, `.agents/reports/d17-vifs-naming-shell-normativity-analysis-260801.md`, `docs/specs/06-graphics.md`, `docs/specs/00-fork.md`, `docs/specs/11-shell.md`, `docs/specs/00-context.md`, `docs/specs/20-unified-ipc-contract.md`, `docs/system-architecture.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/README.md`, `docs/code-standards.md`.
- **Phase 2 owns comments only:** `kernel/src/task.rs`, `cells/services/input/src/dispatcher.rs`, `cells/services/vfs/src/page_cache.rs`.
- **Phase 3 owns no edits:** validation commands and diff inspection only.
- **Phase 4 owns no edits unless review finds stale text introduced by phases 1-2:** any fix must stay within the phase 1/2 owned files and preserve the no-runtime/ABI contract.

## Phases

### Phase 1 — Docs

Status: completed.

Apply D15-D17 approved wording across docs and reports. D15: make Spec 06 focus-routing only, point queue/drop/backpressure ownership to Spec 17, remove or de-norm "zero latency", and mark keyboard focus-on-death as a separate unresolved policy. D16: make Spec 20 own the proposed cluster/remote-IPC contract, move implementation status toward generated Layer-3 status, replace `system-architecture.md` "all planned" with a stable summary/link, and correct roadmap/changelog false completion claims as transitional status only. D17: keep 00-fork as non-normative reference strategy while replacing its filesystem rows with a Spec 09 pointer, mark/remove Spec 11 from active normative spec index, correct active lowercase `viFS1`/`viFS2` naming rules, and preserve uppercase `VIFS1` as BootFS/initramfs.

Dependencies: none.

Risks: High likelihood/medium impact of over-editing historical changelog/research; mitigate by editing only active status/ruling surfaces and preserving historical decision records unless they are currently presented as authoritative. Rollback: revert phase 1 doc hunks only. Irreversible: none.

Success criteria: docket marks D15-D17 ruled/applied; active docs no longer present the withdrawn direct-call input path, false cluster completion/planned binary, or retired lowercase viFS naming as current architecture.

### Phase 2 — Comments

Status: completed.

Update only stale comments in `kernel/src/task.rs`, `cells/services/input/src/dispatcher.rs`, and `cells/services/vfs/src/page_cache.rs`. The code path must remain byte-for-byte semantic equivalent except comment text. D15 comments must say input `try_send` remains non-blocking but may queue into the focused cell's bounded mailbox, and dispatcher comments must not promise keyboard focus fallback/reversion that `dispatch()` does not implement. D17 page-cache comment must stop naming retired `viFS2/WAL` as the future backend and refer to VFS/MountTable/backend policy instead.

Dependencies: phase 1 wording settled so comments use the same terminology.

Risks: Low likelihood/high impact if an implementor "fixes" code to match old comments; mitigate by requiring comment-only diff in these files. Rollback: revert phase 2 hunks only. Irreversible: none.

Success criteria: `git diff --word-diff -- kernel/src/task.rs cells/services/input/src/dispatcher.rs cells/services/vfs/src/page_cache.rs` shows comment/prose-only edits; no Rust tokens, constants, function bodies, or signatures changed.

### Phase 3 — Validation

Status: completed.

Run focused validation:

1. `git diff --check`
2. `rg -n "Direct Call|on_event\\(event\\)|không qua hàng đợi|latency must be zero|viFS1 \\(Classic\\)|viFS2 \\(Modern\\)|viFS2 \\(WAL\\)|cells/apps/shell|production-ready|runs on 2-node|all 10 phases shipped" docs .agents/reports kernel/src/task.rs cells/services/input/src/dispatcher.rs cells/services/vfs/src/page_cache.rs`
3. `cargo check -p vicell-kernel -p service-input -p service-vfs`

Dependencies: phases 1-2 complete.

Risks: Medium likelihood/low impact that `cargo check` is slow or blocked by unrelated workspace state; mitigate by reporting the shortest decisive failure line and confirming whether changed files are comment/doc-only. Rollback: no edits. Irreversible: none.

Success criteria: diff check passes; stale-text grep returns only intentional historical/retirement mentions; targeted Cargo check passes or failure is proven unrelated to the touched files.

### Phase 4 — Review

Status: completed.

Perform review in code-review stance against the final diff. Check scope creep first: no runtime/ABI files beyond the three comment-only code files, no generated artifacts, no broad search-replace damage to uppercase `VIFS1`, and no duplicate ownership of cluster implementation status between Spec 20, system architecture, roadmap, and changelog.

Dependencies: phase 3 complete.

Risks: Medium likelihood/medium impact that review finds inconsistent doc ownership after phase 1; mitigate by fixing only the inconsistent text in phase-owned files and rerunning phase 3 grep/diff checks. Rollback: revert review-fix hunks. Irreversible: none.

Success criteria: reviewer verdict has no blocking findings; final diff can be summarized as D15-D17 docs/report updates plus three comment-only code cleanups, with runtime/ABI unchanged.

## Open Questions

- None for implementation. User has approved D15=A, D16=A, D17=A.
