---
phase: 5
title: "Finish RPi3 HDMI Software Boundary"
status: completed
priority: P1
effort: "5d"
dependencies: [1]
tier: thinking
---

# Phase 05: Finish RPi3 HDMI Software Boundary

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.
## Context Links

- `.agents/260823-rpi3-hardware-completion/plan.md`
- `.agents/260823-rpi3-hardware-completion/phase-04-hdmi-framebuffer.md`


## Overview

Completed the approved mailbox/cache/framebuffer implementation through all host/build/QEMU-compatible checks and handed the separate exact-device evidence boundary to the authoritative HDMI phase.
## Key Insights

The approved A+ design has a large software-verifiable boundary and three explicitly physical assumptions.


## Requirements

- One persistent mailbox DMA page with exact cache-sync pin tokens.
- Dedicated, capability-gated framebuffer registration with checked geometry and reserved range.
- Post-submit timeout poisons the transport and quarantines the page until reboot.
- Preserve VirtIO GPU compatibility and trusted-SAS limitations explicitly.

## Architecture

Reuse `.agents/260823-rpi3-hardware-completion/phase-04-hdmi-framebuffer.md` as the implementation authority. This roadmap does not redesign its A+ architecture.

## Assumptions

- **Claim:** ABI, pin/cache lifecycle, authority checks, and driver state transitions can be verified before physical deployment.
  **Confidence:** high
  **How to verify:** execute the existing phase's ABI, unit, target-build, policy, and generic-AArch64 compatibility matrix.

## Related Files

Use the exact Lane A–E ownership listed in the authoritative HDMI phase; do not create a second file inventory.

## Implementation Steps

1. Execute Lane A additive ABI after full enum/allowlist scan.
2. Run cache/pin and framebuffer-authority lanes in parallel with non-overlapping ownership.
3. Harden the BCM cell and persistent mailbox lifecycle.
4. Complete host tests, AArch64 builds, policy checks, generic AArch64/VirtIO compatibility, and security review.
5. Complete the shared `kernel/src/task/syscall.rs` integration and hand ownership to Phase 02.
6. Mark the software ceiling reached and hand framebuffer range, VideoCore coherency, and visual stability evidence to the authoritative HDMI phase without promoting this software phase beyond host evidence.

## Todo List

- [x] Complete ABI, cache/pin, framebuffer authority, and BCM driver lanes.
- [x] Pass host, target-build, policy, compatibility, and review gates.
- [x] Hand shared syscall integration ownership to Phase 02 after landing.
- [x] Hand the unchanged physical framebuffer/coherency/visual checks to the authoritative HDMI phase.

## Success Criteria

- [x] Every non-physical criterion in the authoritative HDMI plan passes.
- [x] Timeout, stale token, foreign owner, malformed geometry, and partial mapping fail closed.
- [x] Generic AArch64 continues to select VirtIO and its existing behavior remains unchanged.
- [x] This software phase makes no physical claim; exact-board evidence remains owned by the authoritative HDMI phase.

## Security Considerations

No generic cache-maintenance or arbitrary physical-map syscall. Shared USER framebuffer mapping remains trusted-SAS, not owner-only isolation.

## Risk Assessment

If software work discovers that physical values are required to choose the ABI or authority model, stop rather than encode guessed hardware behavior.

## Next Steps

Phase complete at its host/software evidence ceiling. Reopen only for a software regression; exact-device evidence remains owned by the authoritative HDMI phase, and production qualification remains unclaimed.

## Deviation Log

- Full enum/allowlist inspection found two bounded DMA-page copies in `cells/drivers/bcm-display/src/mailbox.rs`. `lungmat8` approved that exact unsafe island on 2026-08-28; strict F1/F5 then passed with unsafe confined to exact reviewed files. Host BCM tests, AArch64 checks, RPi3 packaging, the reviewed image build, and specialist test/review all passed before physical handoff.
