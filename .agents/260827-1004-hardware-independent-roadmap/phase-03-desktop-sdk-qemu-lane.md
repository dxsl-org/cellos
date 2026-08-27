---
phase: 3
title: "Define the Next QEMU Desktop and SDK Slice"
status: completed
priority: P2
effort: "0.5d"
dependencies: [1]
tier: medium
---

# Phase 03: Define the Next QEMU Desktop and SDK Slice

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `docs/project-roadmap.md`
- `.agents/260825-sdk-delivery/plan.md`
- `.agents/260827-0948-desktop-compositor-viui-managed-surface/`

## Overview

Do not schedule more implementation until one approved child names a missing observable contract and owns its files; otherwise record the current QEMU software ceiling.

## Key Insights

Window policy, ViUI, VFS, and compositor behavior already have host/QEMU evidence. Another harness without a new contract is duplicate work.

## Requirements

- Preserve the frozen display wire ABI and configure/acknowledge transaction.
- Require one proved-missing behavior and one approved owner before Build.
- Exercise actual tablet/keyboard input and scanout only for a changed contract.
- Exclude physical GPU, desktop-shell, and G2 qualification claims.

## Architecture

If authorized: `ViUI/app owner → ViSurface/Grant → display dispatcher → compositor policy → QEMU scanout`. Otherwise emit `SOFTWARE_CEILING_REACHED` and stop.

## Assumptions

- **Claim:** An in-flight managed-surface plan will define a new observable slice.
  **Confidence:** low
  **How to verify:** require a populated, approved `plan.md` with missing behavior, ownership, and acceptance evidence; the current `260827-0948` directory is not executable scope.

## Related Files

- Inspect only until approved: `libs/api/src/services/display.rs`, `libs/ostd/src/display/`, `libs/viui/`, `cells/services/compositor/`
- Modify only under approved child ownership: its exact file inventory and focused QEMU scenario
- Emit: current or extended software-ceiling record for Phase 08

## Implementation Steps

1. Inventory the verified desktop/SDK ceiling and fresh managed-surface directories.
2. If no approved child names a new behavior, emit the current ceiling to Phase 08 and stop.
3. If approved, prove the behavior is absent and assign exclusive ownership.
4. Implement and migrate only that contract; exercise it through real QEMU input and scanout.
5. Emit evidence without physical GPU or G2 qualification claims.

## Todo List

- [ ] Pin one missing observable behavior and approved owning child, or stop.
- [ ] Avoid duplicate harness/code work for already verified behavior.
- [ ] Emit the current or newly extended QEMU software ceiling.

## Success Criteria

- [ ] No implementation begins from an empty/incomplete plan directory.
- [ ] Any changed contract has one API, migrated callers, and real QEMU evidence.
- [ ] Existing background, capture, focus, cursor, and client-coordinate behavior remains intact.
- [ ] Taskbar, snapping, persistence, themes, and live resize remain separately gated.

## Security Considerations

Surface ownership, Grant replacement, serials, sender identity, clipping, and bounds remain fail closed.

## Risk Assessment

The active directory currently lacks executable scope. Treat user work as authoritative and never create a competing plan.

## Next Steps

Wait for a populated approved child. Without one, invoke Phase 08 to record the existing QEMU ceiling and retain `execution_class=scope-gated`.

## Deviation Log

- The referenced managed-surface child is populated, approved, and implementation-complete; its host/RISC-V evidence is recorded there. The normal `run.ps1` path correctly invoked `gen_disk.ps1`, but image signing stopped at F1: `hypha-llm-gateway` lacks `#![forbid(unsafe_code)]`, and BCM mailbox unsafe code lacks a reviewed `unsafe-allowlist.toml` entry. Reclassified the lane from the initial `scope-gated` assumption to `governance-gated`; no duplicate desktop implementation was started.
