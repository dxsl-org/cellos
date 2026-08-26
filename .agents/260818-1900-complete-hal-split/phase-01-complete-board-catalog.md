---
phase: 1
title: "Complete The Typed Board Catalog"
status: completed
priority: P1
effort: "6h"
dependencies: []
tier: thinking
---

# Phase 1: Complete The Typed Board Catalog

## Overview

Give every current board selection a complete data-only package and replace stringly driver names with a checked identifier.

## Requirements

- Add descriptors for QEMU virt AArch64, VisionFive 2, Milk-V Pioneer, and Raspberry Pi 4.
- Add SoC identity and typed shared-driver identifiers to `BoardDescriptor`.
- Preserve QEMU RV64 and RPi3 data exactly.
- Check in fallback DTS assets and one reproducible build command per board README.

## Architecture

Descriptors name SoCs and requested shared drivers but contain no register-level mechanism code.

## Assumptions

- **Claim:** The four feature/default paths are the remaining current board selections.
  **Confidence:** high
  **How to verify:** grep `kernel/Cargo.toml` and AArch64/RV64 default cfg branches.

## Related Files

- Modify: `boards/src/descriptor.rs`, `boards/src/lib.rs`, catalog tests
- Create: board packages under `boards/qemu`, `boards/starfive`, `boards/milk-v`, and `boards/raspberry-pi`

## Implementation Steps

1. Introduce `SocId` and `DriverId` enums plus validation.
2. Convert existing descriptors to typed values.
3. Add four missing descriptors, DTS assets, READMEs, and catalog tests.

## Success Criteria

- [x] Board tests validate six descriptors and exact current fallback data.
- [x] Board packages contain no driver implementation.

## Security Considerations

Fail closed on missing SoC or driver selection and overlapping fallback ranges.

## Risk Notes

Incorrect fallback data can corrupt memory. Revert added descriptors/types; no runtime consumer changes occur in this phase.

## Deviation Log

None.
