# Phase 03 — Platform map, resources, driver selectors

**Status:** implemented 2026-11-02, uncommitted; gate PASS ([reports/phase-03-gate.md](reports/phase-03-gate.md)). Phase 04 not started.

## Context Links
[plan](plan.md) · [scout](scout-report.md) · ADR-0029 §6

## Overview
Make `PlatformInfo` population, kernel MMIO page mappings, resource-registry tables and the driver presence selectors backend-provided data.

## Key Insights
- `platform.rs:159-226` has three hard-coded `init` variants. They become one function that reads `Board::platform_info()`.
- `resource_registry.rs` MMIO authority tables are security-relevant. They must be moved byte-for-byte, not re-derived.

## Requirements
- `ArmPlatform::platform_info()`, `kernel_mmio_maps() -> &[MapSpec]`, `mmio_authority() -> &[ResourceEntry]`, `has_virtio`, `has_ecam` and `gpio_base`.
- Remove the `board-rpi*` cfg from `platform.rs`, `memory/paging.rs`, `resource_registry.rs`, `gpio_irq.rs`, `mmc.rs`, `pcie_ecam.rs`, `virtio_common.rs`, `task/drivers.rs`, and the framebuffer/USB parts of `syscall.rs`.
- Add a unit test asserting each backend's authority table equals the pre-plan constant (snapshot copied into the test once).

## Architecture
Static dispatch through `Board` (see the plan's key design decisions). Only `backend/mod.rs` may contain `cfg(feature = "board-rpi*")` after this phase, for the files it covers.

## Related Code Files
`kernel/src/{platform.rs,memory/paging.rs,resource_registry.rs,task/drivers.rs,task/syscall.rs}`, `kernel/src/task/drivers/{gpio_irq,mmc,pcie_ecam,virtio_common}.rs`, `backend/*`.

## Implementation Steps
1. Add data accessors to the trait
2. Move platform_info and kernel MMIO maps
3. Move resource authority tables with equality test
4. Move driver selectors and syscall branches
5. Run preservation gate

## Todo List
- [x] Add data accessors to the trait (`ArmResources`)
- [x] Move platform_info and kernel MMIO maps
- [x] Move resource authority tables with equality test (Q5: kernel-side tables selected by `SOC`, compile-time equality assertions for all three boards, virt boot self-test)
- [x] Move driver selectors and syscall branches (`pcie_ecam.rs` dropped, Q6)
- [x] Run preservation gate

Deviations are in the gate report §6. Summary:
- The accessors are `const` data; `mmio_authority()` is replaced by `SOC` and the kernel-side tables; there is no `has_ecam`.
- The equality test is a compile-time assertion rather than a host unit test.
- The virt self-test runs on every boot inside the existing `mmio-allowlist` self-test.

## Success Criteria
Gate passes. The authority-table equality tests pass for all three backends. The listed files are free of `board-rpi*`.

## Risk Assessment
Mis-moving an MMIO permission entry would widen user-accessible MMIO. The equality test catches it. Rollback: revert.

## Security Considerations
Resource-registry tables are capability-relevant (ADR-0024). Byte-identical move only. Any semantic change is out of scope.

## Next Steps
Phase 04.
