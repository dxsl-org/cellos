---
title: "Add And Consume RPi3 Descriptor"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 02 — Add And Consume RPi3 Descriptor

## Requirements

- Add a data-only RPi3 Model B descriptor and fallback DTS audit asset.
- Add VideoCore firmware identity without changing boot mechanism.
- Validate the descriptor for AArch64.
- Source RPi3 platform UART/absence data and boot fallback addresses from the descriptor.

## Related Code Files

- `boards/raspberry-pi/3-model-b/*`
- `boards/src/lib.rs`
- `boards/src/descriptor_tests.rs`
- `kernel/Cargo.toml`
- `kernel/src/board.rs`
- `kernel/src/platform.rs`
- `kernel/src/boot.rs`

## Todo List

- [x] Add identity, boot, fallback, wiring, and driver data.
- [x] Add descriptor validation tests.
- [x] Rewire platform and fallback boot consumers.

## Risk Assessment

Wrong fallback ranges can corrupt the kernel on RPi3. Tests must assert exact parity with the current constants; rollback restores the two existing literals arrays.

## Success Criteria

RPi3 platform and boot fallback compile from descriptor data with no driver or SoC-policy relocation.
