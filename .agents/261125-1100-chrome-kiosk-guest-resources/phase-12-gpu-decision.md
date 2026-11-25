---
phase: 12
title: "GPU decision"
status: completed
priority: P3
effort: "1-2d"
dependencies: [7]
tier: thinking
checkpoint: false
---

# Phase 12: GPU decision

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

The request names GPU among the things to extend. CellOS has no graphics GPU stack: the
compositor draws into a linear framebuffer, the hypervisor's virtio-gpu is 2D, and ADR-0033
admits compute only on one Intel iGPU, with nothing built. A guest cannot be given what the
host does not have. This phase is a decision with measurements, written as an ADR; it builds
nothing unless the owner picks option B.

## Requirements

- Functional: an ADR (next free number) that compares, with the phase-02 bench after phases
  03-07:
  - **A. Software only.** Chromium keeps `--disable-gpu`; speed comes from phases 03-07.
  - **B. virgl proxy.** When CellOS itself runs under QEMU with `virtio-vga-gl`, the
    hypervisor's virtio-gpu forwards the guest's 3D command stream to the outer device and
    reads the scanout back into the launcher's Grant. Works only for VM-hosted CellOS, needs a
    3D-capable CellOS virtio-gpu driver, and still copies each frame to the CPU.
  - **C. Native graphics.** Extend the ADR-0033 Intel iGPU work from compute to display and
    rendering, then give the guest a paravirtual GPU on top. The only route that helps on a
    physical machine; by far the largest.
- Functional: for B, a one-week spike is costed, not started: what CellOS's `cells/drivers/virtio-gpu`
  lacks for 3D, and whether readback at 60 Hz leaves any gain over option A's numbers.
- Non-functional: no claim of GPU acceleration appears in any document before an option that
  provides it is built and measured.

## Architecture

None in this phase.

## Assumptions

- **Claim:** after phases 03-07 the bench's `scroll` and `paint` rows are within a factor of
  two of the plain QEMU/KVM reference with the same vCPUs. **Confidence:** low.
  **Verify:** the bench; the ADR quotes it either way.
- **Claim:** readback makes option B's gain small for a 1024x680 window. **Confidence:** low.
  **Verify:** only a spike can; the ADR states it as unknown.

## Related Files

- Create: `docs/decisions/00NN-chrome-guest-gpu-path.md`
- Modify: `docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md` (§5d pointer),
  `docs/project-roadmap.md`

## Implementation Steps

1. Bench after phase 07, against the plain QEMU/KVM reference.
2. Read `cells/drivers/virtio-gpu` and `cells/runtimes/mesa-anv` for what option B and C start from.
3. Write the ADR with the owner; record the choice.

## Success Criteria

- [x] The ADR exists, quotes measured numbers for option A, and records the owner's choice.
      (`docs/decisions/0037-chrome-guest-gpu-path.md`; choice A, owner 2026-11-25.)
- [x] ADR-0017 §5d and the guide say what the choice is. (ADR-0017 §5e, guide intro.)

## Security Considerations

Options B and C put a guest-driven 3D command stream in front of a host GPU driver; the ADR
must name the validation layer each would need before either is admitted.

## Risk Notes

Choosing B optimises the configuration CellOS runs in today and not the one it is aimed at.

## Deviation Log

- **Deviation (step 1):** no new bench round against the plain QEMU/KVM reference; the ADR
  quotes the phase-02 reference (loaded host) beside the phase 05/07 CellOS rounds (quiet
  host) and says they show magnitude, not a ratio. The owner's choice (software) did not
  depend on a closer number.
- **Deviation:** the pointer goes into ADR-0017 §5e (the 2026-11-25 section that left the GPU
  undecided), not §5d; `docs/project-roadmap.md` has no Chrome GPU entry to change.

