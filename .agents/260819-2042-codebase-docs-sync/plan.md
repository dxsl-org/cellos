---
title: "Codebase Docs And Roadmap Sync"
description: "Docs-only plan to re-baseline Cellos documentation against current source and split the oversized roadmap into stable lookup files."
status: completed
priority: P2
effort: 21h
branch: docs/codebase-sync
tags: [docs, tech-debt]
blockedBy: []
blocks: []
created: 2026-08-19
completed: 2026-08-20
---

# Codebase Docs And Roadmap Sync

## Overview

Reconcile current Cellos source with docs, record verified open issues without fixing code, and split `docs/project-roadmap.md` into a short index plus independent roadmap files. `docs/coding.md` and `docs/engineering-standards.md` were requested by workflow but are absent in this checkout; follow AGENTS.md and existing docs contracts instead.

## Phases

| Phase | Name | Status | Depends |
|-------|------|--------|---------|
| 1 | [Build Authoritative Baseline](./phase-01-build-authoritative-baseline.md) | completed | none |
| 2 | [Split Roadmap Information Architecture](./phase-02-split-roadmap-information-architecture.md) | completed | 1 |
| 3 | [Sync Architecture And Codebase Docs](./phase-03-sync-architecture-and-codebase-docs.md) | completed | 1 |
| 4 | [Document Open Issues And API Drift](./phase-04-document-open-issues-and-api-drift.md) | completed | 1 |
| 5 | [Normalize Changelog And Generated Metrics](./phase-05-normalize-changelog-and-generated-metrics.md) | completed | 1 |
| 6 | [Validate Docs Graph And Handoff](./phase-06-validate-docs-graph-and-handoff.md) | completed | 2, 3, 4, 5 |

## Dependency Graph

`1 -> {2,3,4,5} -> 6`. Phases 2-5 may run in parallel only if file ownership below is respected.

## File Ownership

- Phase 2: `docs/project-roadmap.md`, `docs/roadmap/*.md`
- Phase 3: `docs/codebase-summary.md`, `docs/system-architecture.md`, `docs/code-standards.md`
- Phase 4: `docs/security-model.md`, `docs/network-api.md`, optional `docs/known-issues.md`
- Phase 5: `docs/project-changelog.md`, `docs/code-metrics.generated.md`
- Phase 6: validation reports in `.agents/260819-2042-codebase-docs-sync/`

## Source Of Truth Rules

- Source code beats docs; generated metrics beat prose counts.
- Memory and draft docs are PRIOR until verified with `rg` or `git show`.
- QEMU, hardware, and host-gated evidence stay labeled separately.
- No implementation source changes in this plan.

## Success Criteria

- `docs/project-roadmap.md` is an index under 250 lines.
- Every moved roadmap section has one new stable destination and one backlink.
- No stale paths remain for deleted/nonexistent source files found in this audit.
- Verified open issues are documented with file:line evidence and not marked shipped.
- Link/path validation and targeted docs grep checks pass.

## Cook Handoff

Run `$hc-cook .agents/260819-2042-codebase-docs-sync/plan.md` after approving the plan.

## Completion Record

Completed through PR #28, merged to `main` as `722c7e57` on 2026-08-20.
The roadmap was split, the source-backed documentation was synchronized, and
the remaining code and hardware gaps were recorded as open gates rather than
as completed work. The phase files were left at their draft `pending` state
during publication; this ledger corrects that administrative omission.

## Unresolved Questions

- Should `docs/project-changelog.md` remain a single large chronological file, or should old entries be archived after this sync?
- Should the optional open-issues register be `docs/known-issues.md` or a roadmap subfile?
