---
phase: 5
title: "Finish RPi3 HDMI Software Boundary"
status: blocked
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

Execute the already-designed safe mailbox/cache/framebuffer implementation through all host/build/QEMU-compatible checks, then stop at the physical framebuffer/coherency gate.
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
6. Mark the software ceiling reached; leave framebuffer range, VideoCore coherency, and visual stability `BLOCKED_PHYSICAL` until the connected board is available.

## Todo List

- [ ] Complete ABI, cache/pin, framebuffer authority, and BCM driver lanes.
- [ ] Pass host, target-build, policy, compatibility, and review gates.
- [ ] Hand shared syscall integration ownership to Phase 02 after landing.
- [ ] Record physical framebuffer/coherency/visual checks as blocked.

## Success Criteria

- [ ] Every non-physical criterion in the authoritative HDMI plan passes.
- [ ] Timeout, stale token, foreign owner, malformed geometry, and partial mapping fail closed.
- [ ] Generic AArch64 continues to select VirtIO and its existing behavior remains unchanged.
- [ ] No claim is made about returned framebuffer range, mailbox coherency, or visible HDMI output.

## Security Considerations

No generic cache-maintenance or arbitrary physical-map syscall. Shared USER framebuffer mapping remains trusted-SAS, not owner-only isolation.

## Risk Assessment

If software work discovers that physical values are required to choose the ABI or authority model, stop rather than encode guessed hardware behavior.

## Next Steps

Execute the existing HDMI phase through its non-physical criteria, hand off `syscall.rs`, then retain the physical deployment step unchanged.

## Deviation Log

- Full enum/allowlist inspection found that the implemented mailbox transport requires raw DMA-page copies in `cells/drivers/bcm-display/src/mailbox.rs`, but the F1 unsafe allowlist lacks a file entry. Policy requires new entries to name an actual reviewer; adding `cellos-maintainers` would falsely claim the historical group review. Phase is blocked pending named approval or an equivalent safe redesign.
