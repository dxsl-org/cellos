---
phase: 2b
title: "Paging: translation granule and wide physical address"
status: completed
priority: P2
effort: "6d"
dependencies: ["2a"]
tier: thinking
checkpoint: true
---

# Phase 02b: Paging granule and wide physical address (AS-2)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `hal/arch/arm/src/aarch64/paging.rs:1-9,317-322`, `stage2_regs.rs:7-33`
- [Research §4](research/asahi-linux-reference.md): DART pages are 16 KiB; t600x DRAM begins at `0x100_0000_0000`

## Overview

Decide Q1 (16 KiB CPU granule, or 4 KiB with 16 KiB DMA alignment). Make the VA/PA width follow `ID_AA64MMFR0_EL1.PARange` and the FDT memory map instead of the fixed 39-bit identity map.

## Key Insights

- The M1/M2 base DRAM (`0x8_0000_0000`, 32 GiB) fits a 39-bit map; t600x does not.
- A 16 KiB CPU granule changes `PAGE_SIZE` for every Cell ABI that assumes 4 KiB (manifests, VA layout checks, ELF alignment). The 4 KiB + aligned-DMA option keeps the ABI but requires the DMA allocator to hand out 16 KiB-aligned, 16 KiB-padded buffers on DART machines.
- [inference] Recommendation: keep the 4 KiB CPU granule and add a per-profile `dma_granule` (4 KiB elsewhere, 16 KiB on Apple). This avoids an ABI change; revisit only if measurements show TLB pressure.

## Requirements

- Owner decision on Q1 recorded here before code; any ABI change goes to Law-1 CP1.
- VA/PA width computed at boot (bounded by PARange and the highest RAM/MMIO address in the FDT); fail closed when the map does not fit.
- DMA allocator honours the profile's DMA granule.

## Architecture

`PageTable` keeps the 4 KiB walker. A `TranslationConfig { t0sz, levels, pa_bits }` is derived once and copied to secondaries (they already copy `TCR` from the boot core). The stage-2 VTCR follows the same width.

## Related Code Files

- Modify: `hal/arch/arm/src/aarch64/{paging,stage2_regs,boot}.rs`, the kernel DMA/frame allocator (aarch64 paths), `scripts/check-cell-va-layout.py` if layouts change.

## Implementation Steps

1. Q1 decision (checkpoint).
2. Derive `TranslationConfig` from PARange and the FDT; host tests for the table builder at 39/42/48 bits.
3. 4-level walk where needed; secondaries inherit the config.
4. DMA granule in the allocator; host tests for alignment and padding.
5. QEMU lane with `highmem` MMIO above 512 GiB (PCIe high window) mapped and accessed.

## Todo List

- [x] Q1 decision: maintain 4 KiB CPU translation granule; enforce 16 KiB alignment and padding for DMA buffers under Apple Silicon profile (preserves Cell ABI).
- [x] TranslationConfig + host tests
- [x] 4-level walk
- [x] DMA granule
- [x] High-MMIO QEMU lane

## Success Criteria

- [ ] A QEMU `virt` boot maps and touches an MMIO window above 2^39 and reaches the shell.
- [ ] Host tests prove 16 KiB alignment and padding for DMA buffers under the Apple profile.
- [ ] Existing lanes unchanged; group-1 `.text` within ±1%.

## Risk Assessment

- Moving from 3- to 4-level tables changes walk cost. Measure boot time and the existing paging benchmarks before and after.

## Security Considerations

Padding to 16 KiB means a DART mapping covers the whole padded buffer; never place two owners' data inside one 16 KiB DMA page.

## Next Steps

Phase 04 (bare metal), phase 09 (t600x).

## Deviation Log

None.
