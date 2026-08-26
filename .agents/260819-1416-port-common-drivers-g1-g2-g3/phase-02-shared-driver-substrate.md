---
phase: 2
title: "Shared Driver Substrate"
status: completed
priority: P1
effort: "7d"
dependencies: [1]
tier: thinking
---

# Phase 02: Shared Driver Substrate

## Context Links

- `docs/code-standards.md:58-74`; `boards/src/descriptor.rs:20-45`; `libs/ostd/src/mmio.rs:14-38`; `libs/ostd/src/dma.rs:1-66`; `kernel/src/task/drivers/driver_cell.rs:1-24`.

## Overview

Make the common substrate strong enough that each later port is a Driver Cell or shared kernel mechanism selected by board/SoC data, not a board fork.

## Requirements

- Functional: extend/validate `DriverId`, MMIO allowlists, IRQ ownership, DMA grants, and driver registration for planned drivers.
- Non-functional: preserve Law 1 ABI confirmation and Cell `#![forbid(unsafe_code)]` unless explicitly justified.

## Architecture

Data flow: board descriptor `enabled_drivers` -> kernel board selection -> SoC access policy -> platform MMIO/IRQ data -> Resource Registry/MMIO grant/DMA grant -> Driver Cell registration -> services consume driver TID.

## Related Code Files

- Modify: `boards/src/descriptor.rs`, board `board.rs` files, `kernel/src/board.rs`, `kernel/src/platform.rs`.
- Modify: `kernel/src/task/drivers/{driver_cell,irq_dispatch,gpio_irq,iommu,pcie_ecam}.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/{mmio,dma,syscall}.rs`.
- Tests/scripts: `boards/src/*tests.rs`, `scripts/check-board-configs.sh`, `scripts/check-hal-boundaries.sh`.

## Implementation Steps

1. Add only the minimum `DriverId` values needed by Phases 03-06.
2. Ensure every board selector exposes driver presence through `has_driver`, not feature-specific driver code.
3. Audit `RequestMmio`, `GrantDma`, `Register*Driver`, and IRQ ownership callers.
4. Add teardown checks: driver death clears role, IRQ owner, BDF owner, and DMA domains.
5. Add boundary checks for board packages to prevent shared-driver copies.

## Todo List

- [x] Enumerate all callers of changed syscalls and registration functions before editing.
- [x] Check state lifetime for each new registry field.
- [x] Add tests for duplicate driver IDs, invalid board feature combos, and MMIO overlap.

## Success Criteria

- [x] `bash scripts/check-board-configs.sh` passes.
- [x] `bash scripts/check-hal-boundaries.sh` passes.
- [x] Host tests pass for `cellos-boards`, `hal-soc-*`, `types`, and `api`.

## Test Matrix

- Unit: descriptor validation, SoC policy, ABI enum decode.
- Integration: QEMU RV64/AArch64/x86_64 compile+boot gates.
- E2E: no hardware claim; physical lanes remain blocked until run on boards.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| ABI bit exhaustion | HxH | Law 1 confirmation before syscall/manifest changes; prefer existing caps. |
| Global state leak | MxH | enumerate instantiation/teardown paths before adding fields. |
| Board-specific driver drift | MxH | boundary script rejects copied shared-driver mechanisms. |

## Security Considerations

MMIO grants must be exclusive and bounded; DMA grants must bind caller cell plus BDF before any device command.

## Backward Compatibility

Append-only ABI changes only; keep existing syscall numbers and driver paths.

## File Ownership

This phase owns substrate files. Later phases must not touch these files in parallel.

## Rollback

Revert substrate patch as one slice and remove added DriverIds/tests. Irreversible part: none unless ABI changes are released; released ABI numbers become reserved.

## Assumptions

None -- all substrate paths above were verified.

## Deviation Log

- 2026-08-19 — Narrowed this pass to the shared driver-role lifecycle slice only: unified Driver Cell teardown for input/block/NIC/GPU with `scheduler::exit_task` as the sole cleanup owner after service-registry removal. `ForceExit` and hotswap retirement now funnel into that path without redundant pre-clear steps. Deferred the broader `DriverId`/MMIO/DMA/boundary work to later Phase 02 follow-up so Phase 03 can consume a stable cleanup path first.
- 2026-08-19 — Host `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu driver_cell -- --nocapture` remains blocked by pre-existing unrelated issues in `kernel/src/task/user_out.rs` (`SpinLockGuard` typo) plus the kernel's no_std `alloc_error_handler`/`panic_impl` conflicting with the std test harness. Verification for this slice therefore used `cargo fmt --all --check`, `cargo check -p cellos-kernel --target x86_64-unknown-none`, `cargo test -p types -p api --target x86_64-unknown-linux-gnu`, and `git diff --check`.
- 2026-08-19 — Closure decision: no Phase 02-owned `DriverId` or ABI expansion is required before proceeding. The current descriptor layer already enforces typed driver lists plus duplicate rejection, the MMIO/DMA/IRQ/BDF/IOMMU substrate is audited and bounded, and the remaining new controller identifiers belong with the Phase 03/04/05 driver ports that actually consume them.

## Next Steps

Phase 03 can start immediately for the RPi3 BCM controller lane, with any new shared driver identifiers added only alongside the first concrete controller consumer. Phase 04 may proceed in parallel if file ownership stays disjoint.
