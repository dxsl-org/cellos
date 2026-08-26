---
title: "DTB Memory Map and Runtime Gates"
description: "Replace the RV64 190 MiB fallback with a reservation-safe DTB map, then close the remaining phase 09/11 runtime evidence."
status: completed
priority: P1
effort: 2d
branch: feat/wx-post-reloc-and-f1-signing
tags: [bugfix, critical, infra]
blockedBy: []
blocks: []
created: 2026-07-31
---

# DTB Memory Map and Runtime Gates

## Overview

Continue from `HANDOFF-260731.md` section 8 in strict order: A1 first, then A4. A2 and A3
are deferred until both are verified. Current baseline is `976a6ac2`; preserve the four known
build-artifact modifications and treat temporary worktree artifacts as preflight-only unless
rebuilt from the commit being certified.

## Phases

| Phase | Name | Status |
|---|---|---|
| 1 | [Build the RV64 DTB memory map](./phase-01-rv64-dtb-memory-map.md) | completed |
| 2 | [Verify DTB capacity at runtime](./phase-02-verify-dtb-capacity.md) | completed; full serial rerun timed out |
| 3 | [Close phase 09 and 11 runtime gates](./phase-03-close-runtime-gates.md) | completed; demo packaging gaps recorded |

## Dependencies

- Phase 2 requires phase 1 because a boot-success check alone cannot distinguish the old 190 MiB map.
- Phase 3 starts only after A1 passes its 256 MiB regression and 2 GiB capacity gates.
- A2 (`OutOfMemory` ABI/logging) and A3 (`MemInfo`) remain out of scope for this plan.

## Ownership

- Phase 1 owns boot-map production code and host fixtures.
- Phase 2 owns the RV64 memory-size integration harness and evidence.
- Phase 3 owns policy/signing runtime fixtures and evidence; it must not reopen phase-11 implementation.

## Handoff

Execute with `$hc-cook .agents/260731-1804-dtb-memory-and-runtime-gates/plan.md`.

## Evidence

- [A1 DTB runtime evidence](../reports/a1-dtb-runtime-260731.md)
- [A4 runtime-gate evidence](../reports/a4-runtime-gates-260731.md)
