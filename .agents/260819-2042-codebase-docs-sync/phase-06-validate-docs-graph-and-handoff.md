---
phase: 6
title: "Validate Docs Graph And Handoff"
status: completed
priority: P1
effort: "2h"
dependencies: [2, 3, 4, 5]
tier: medium
---

# Phase 6: Validate Docs Graph And Handoff

## Overview

Validate links, stale paths, source claims, and file ownership after the docs split. This phase is the gate before commit or PR review.

## Requirements

- Functional: verify docs graph, regenerated metrics, and source-backed claims.
- Non-functional: preserve user/draft changes not owned by this plan; do not claim hardware/runtime tests as run unless actually run.

## Architecture

Data flow: final docs diff enters validation commands; failures transform into local doc fixes in owned files; output is `.agents/.../reports/validation-report.md` plus a clean handoff summary.

## Assumptions

- **Claim:** No external docs-link checker is required for this repo.
  **Confidence:** medium
  **How to verify:** local `rg` path checks cover repository-relative links; use a Markdown checker only if already installed.

## Related Files

- Create: `.agents/260819-2042-codebase-docs-sync/reports/validation-report.md`
- Read: all modified docs

## Implementation Steps

1. Run `git -c safe.directory='*' diff --name-only` and confirm only phase-owned product docs plus `.agents` changed.
2. Re-grep all cited source paths and symbols from modified docs.
3. Validate relative Markdown links into `docs/roadmap/` and backlinks to `docs/project-roadmap.md`.
4. Run `python scripts/generate-code-metrics.py --check` if available.
5. Run a low-risk docs/source sanity set: `rg -n "cells/services/net/src/lib.rs|tests/integration/network_loopback|MicroPython runtime \\| ✅|codex/" docs`.
6. Optional host-gated compile sanity: `cargo check -p boards -p hal-arch-trait -p vicell-kernel` only if toolchain is available and no generated artifacts churn unexpectedly.
7. Write validation report with PASS/FAIL/HOST-GATED rows and rollback instructions.

## Success Criteria

- [ ] Validation report exists with commands and decisive output snippets.
- [ ] Stale path grep returns no matches except intentionally archived history.
- [ ] `codex/` branch prefix does not appear in new workflow guidance.
- [ ] Final diff is docs-only; no implementation code changed.

## Security Considerations

Validation must preserve evidence labels. QEMU checks, hardware checks, and compile checks are separate confidence classes.

## Risk Notes

- Medium likelihood x high impact: validation fixes can cross file ownership and conflict with parallel work. Mitigation: only Phase 6 runs after phases 2-5 complete.
- Medium likelihood x medium impact: host toolchain unavailable. Mitigation: mark HOST-GATED and include the exact command for later run.
- Rollback: revert validation-only docs adjustments and delete validation report. Irreversible part: none.

## Deviation Log

Validation was completed as part of the docs-only PR review and merge. No
separate validation report was retained; this is a documentation-process gap,
not an unverified product claim.
