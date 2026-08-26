---
phase: 1
title: "Build Authoritative Baseline"
status: completed
priority: P1
effort: "4h"
dependencies: []
tier: thinking
---

# Phase 1: Build Authoritative Baseline

## Overview

Create the source-truth inventory that all docs edits must obey. This phase prevents stale roadmap text or draft docs from becoming accepted truth.

## Requirements

- Functional: inventory active crates, board/SoC/HAL ownership, runtime membership, shipped docs claims, and verified open issues.
- Non-functional: no product docs edited; all findings carry OBSERVED file:line or stay PRIOR/ASSUMED.

## Architecture

Data flow: source tree and git history enter as read-only inputs; commands transform them into `.agents/.../reports/source-baseline.md`; later phases consume that report and update only their owned docs.

Dependency graph: no blockers. Phases 2-5 must not start until the baseline names exact file ownership and verified facts.

## Assumptions

- **Claim:** Windows Git can keep using `-c safe.directory='*'` for this worktree.
  **Confidence:** high
  **How to verify:** `git -c safe.directory='*' status --short --branch`

## Related Files

- Create: `.agents/260819-2042-codebase-docs-sync/reports/source-baseline.md`
- Read: `Cargo.toml`, `boards/`, `hal/`, `kernel/`, `cells/`, `libs/`, `tests/integration/`, `docs/`

## Implementation Steps

1. Record `git status --short --branch` and protect pre-existing changes in `docs/code-standards.md`, `docs/codebase-summary.md`, and `docs/project-roadmap.md` as draft input.
2. Count workspace members from `Cargo.toml` and crate manifests; compare against `docs/codebase-summary.md:16` and `docs/codebase-summary.md:25`.
3. Re-grep HAL ABI symbols: `kernel_abi`, `vi_handle_page_fault`, `vi_timer_tick`, `vi_handle_uart_irq`, and list callers with file:line.
4. Re-grep board/SoC ownership paths: `BoardDescriptor`, `RiscvSocProfile`, `Bcm27xxSocProfile`, `X86PlatformProfile`, `ArmVirtProfile`.
5. Re-grep known issue markers for signing/policy, POSIX file ops, net-broker dispatch, net polling, stale docs paths.
6. Write source-baseline with OBSERVED, PRIOR, ASSUMED, and UNVERIFIED sections.

## Success Criteria

- [ ] Baseline report exists and cites every load-bearing claim with file:line.
- [ ] Each behavioral claim lists callers; if a caller count exceeds 10, first 10 plus total are listed.
- [ ] Draft docs are explicitly marked draft input, not truth.

## Security Considerations

This phase handles no secrets and runs read-only commands. Do not dump private key material if grep finds generated cert fixtures.

## Risk Notes

- High likelihood x medium impact: generated/vendor directories can drown useful evidence. Mitigation: prefer targeted `rg` over repo-wide dumps.
- Medium likelihood x high impact: unverified memory could reintroduce stale claims. Mitigation: memory remains PRIOR until source-grepped.
- Rollback: delete this phase report; product docs are untouched. Irreversible part: none.

## Deviation Log

Completed evidence was captured in `reports/scout-report.md` and the merged
documentation instead of a separate `reports/source-baseline.md` artifact.
