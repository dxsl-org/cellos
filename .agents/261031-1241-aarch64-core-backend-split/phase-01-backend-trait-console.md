# Phase 01 — Backend trait and console/diagnostics

**Status:** implemented 2026-11-02, uncommitted; gate PASS ([reports/phase-01-gate.md](reports/phase-01-gate.md)).

## Context Links
[plan](plan.md) · [scout](scout-report.md) · ADR-0029 §6

## Overview
Introduce the `backend` module, the `Board` alias and the `ArmPlatform` trait, and move the console/early-diagnostic selection behind it.

## Key Insights
- Console is the smallest seam that touches every variant, so it proves the dispatch mechanism before IRQ work.
- `cfi.rs` and `mte.rs` run very early. Their output functions must stay `#[inline(always)]` and allocation-free.

## Requirements
- `backend/{mod,qemu_virt,bcm2837,bcm2711}.rs`.
- Trait `ArmPlatform { fn early_putc(u8); fn console_base() -> usize; … }`.
- Remove the `board-rpi*` cfg sites from `cfi.rs`, `mte.rs`, `uart_pl011.rs`, `main.rs` (console selection only) and `console_drv.rs`.
- Record the pre-plan baseline (sizes and boot logs) in `reports/baseline-2026-11-02.md` before the first change (Q8: dated name kept).

## Architecture
Static dispatch through `Board` (see the plan's key design decisions). Only `backend/mod.rs` may contain `cfg(feature = "board-rpi*")` after this phase, for the files it covers.

## Related Code Files
`hal/arch/arm/src/aarch64/{backend/*,cfi.rs,mte.rs,uart_pl011.rs}`, `kernel/src/main.rs`, `kernel/src/task/drivers/console_drv.rs`.

## Implementation Steps
1. Capture baseline matrix
2. Add backend module and trait
3. Move console/diagnostic selection
4. Run preservation gate

## Todo List
- [x] Capture baseline matrix ([reports/baseline-2026-11-02.md](reports/baseline-2026-11-02.md))
- [x] Add backend module and trait
- [x] Move console/diagnostic selection (including `task/drivers/uart.rs`, Q1)
- [x] Run preservation gate ([reports/phase-01-gate.md](reports/phase-01-gate.md))

Deviations, detailed in the gate report §6:
- `uart_bcm_mini.rs` stays in place. Its other callers are in Phase 02 (`trap.rs`) and Phase 04 (`main.rs`) files, so the Q3 move is left to Phase 02.
- The plan's `console_base()` became `const PL011_BASE`.

## Success Criteria
Behaviour-preservation gate passes. `rg 'feature = "board-rpi' ` shows no hits in the five listed files.

## Risk Assessment
Very early output paths can break silently (no console). Mitigation: compare the QEMU `virt` and `raspi3b` first-banner bytes against the baseline. Rollback: revert.

## Security Considerations
No authority or MMIO-permission change. Console base addresses come from the existing profiles.

## Next Steps
Phase 02 and phase 03 can proceed in parallel. They own disjoint files.
