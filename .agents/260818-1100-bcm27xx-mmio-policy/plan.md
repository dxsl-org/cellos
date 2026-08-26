---
title: "BCM27xx MMIO Policy Slice"
description: "Move duplicated BCM2837 MMIO spans and grant-window sizes into the data-only SoC profile."
status: completed
priority: P2
effort: 3h
branch: fix/structure
tags: [refactor, hal, bcm27xx, mmio]
blockedBy: []
blocks: []
created: 2026-08-18
---

# BCM27xx MMIO Policy Slice

## Scope Contract

- Extend `hal/soc/bcm27xx` with exact BCM2837 peripheral/local-controller spans and GPIO/AUX grant-window sizes.
- Rewire RPi3 paging, resource allowlist, and GPIO IRQ owner lookup to consume those facts.
- Preserve exact USER-accessible peripheral flags, kernel-only local-controller flags, and grant widths.
- Exclude IRQ/timer mechanism relocation, board descriptor changes, new MMIO rights, DTB discovery, and physical runtime claims.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Extend and validate the SoC profile](./phase-01-extend-bcm-mmio-profile.md) | completed | none |
| 2 | [Consume profile facts in kernel policy](./phase-02-consume-bcm-mmio-profile.md) | completed | 1 |
| 3 | [Verify, review, and document](./phase-03-verify-review-document.md) | completed | 2 |

## Assumptions

- OBSERVED: the three consumers repeat addresses already partially represented by `BCM2837.mmio`.
- OBSERVED: paging permissions differ by span and must remain unchanged.
- OBSERVED: `6036f2dda` introduced the GPIO/AUX allowlist and GPIO owner lookup together, so they must remain exact peers.

## Compatibility Strategy

Only the source of immutable constants changes. Existing addresses, lengths, permissions, cfg gates, and driver mechanisms remain byte-for-byte equivalent.

## Deferred Work

- BCM2835/BCM2836 IRQ and timer policy extraction.
- Executable pinmux generation.
- Board feature collapse and physical RPi3 validation.

## Evidence Boundary

QEMU RV64 runtime is regression-tested. RPi3 remains compile-only for this slice.
