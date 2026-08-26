---
phase: 1
title: "Model generic x86 PC"
status: completed
priority: P1
effort: "2h"
dependencies: []
tier: thinking
---

# Phase 1: Model Generic x86 PC

## Overview

Add one integration-only generic ACPI PC descriptor and a no_std x86 SoC/platform profile for static facts.

## Requirements

- Functional: catalog a generic x86_64 PC booted by Limine under BIOS or UEFI.
- Functional: profile COM1 port/IRQ and the bounded legacy firmware window in `hal/soc/x86`.
- Non-functional: no fallback LAPIC, IOAPIC, HPET, or MCFG addresses.

## Architecture

The board descriptor selects `SocId::GenericX86Pc` and shared driver identities. `hal/soc/x86` owns immutable PC-compatible platform facts and validates its ranges. Firmware-discovered addresses remain absent from this profile.

## Assumptions

None — relevant contracts and hard-coded call sites were read directly.

## Related Files

- Create: `boards/generic/x86_64-pc/board.rs`
- Create: `boards/generic/x86_64-pc/README.md`
- Create: `hal/soc/x86/Cargo.toml`
- Create: `hal/soc/x86/src/lib.rs`
- Modify: `boards/src/lib.rs`
- Modify: `boards/src/descriptor.rs`
- Modify: `boards/src/catalog_tests.rs`
- Modify: `Cargo.toml`

## Implementation Steps

1. Extend the boot and SoC enums only as required for Limine + ACPI PC boot.
2. Add and validate the generic x86 descriptor without pretending a DTB exists.
3. Add a focused SoC profile and host tests for COM1 and firmware-window bounds.

## Success Criteria

- [x] Board and SoC host tests pass.
- [x] The descriptor contains no port/MMIO/IRQ numeric facts.
- [x] The SoC profile contains no firmware-discovered fallback device addresses.

## Security Considerations

Firmware memory admission remains bounded; no broad low-memory trust is introduced.

## Risk Notes

Changing descriptor boot semantics touches all catalog validation; existing six descriptors must remain valid.

## Deviation Log

None.
