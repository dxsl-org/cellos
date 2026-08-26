---
phase: 2
title: "Make RISC-V Selection Descriptor-Driven"
status: completed
priority: P1
effort: "5h"
dependencies: [1]
tier: thinking
---

# Phase 2: Make RISC-V Selection Descriptor-Driven

## Overview

Select QEMU, VF2, and Pioneer board/SoC data once and remove feature-specific fallback duplication from boot/platform code.

## Requirements

- `kernel/src/board.rs` returns the selected RISC-V descriptor and matching SoC profile.
- VF2 and Pioneer fallback maps come from descriptors.
- DTB discovery remains authoritative and SG2042 remains SBI-DBCN/fail-closed.
- Preserve current board features and PLIC physical-hart policy.

## Architecture

Feature cfg chooses a descriptor once; boot and platform consume that immutable selection without their own board tables.

## Assumptions

None — current selection branches and profiles were read directly.

## Related Files

- Modify: `kernel/src/board.rs`, `kernel/src/boot.rs`, `kernel/src/platform.rs`
- Modify tests for selected descriptor/profile parity

## Implementation Steps

1. Centralize descriptor/profile pairing in `board.rs`.
2. Generalize fallback-map construction for all RV64 descriptors.
3. Remove platform feature branches in favor of the selected profile.

## Success Criteria

- [x] Default, VF2, and Pioneer RV64 checks pass.
- [x] No RV64 board fallback literal remains in kernel boot/platform.

## Security Considerations

Unsupported access policies remain fail-closed.

## Risk Notes

Wrong pairing can probe invalid MMIO. Revert centralized selection and restore prior branches if any feature lane regresses.

## Deviation Log

None.
