# Phase 02 Substrate Closure Audit

Date: 2026-08-19
Worktree: `/home/dmin/cellos-worktrees/common-drivers-g1-g2-g3`
Verdict: PASS — Phase 02 can close without adding new `DriverId` values or touching `libs/api` / `libs/types`.

## What this audit checked

- Driver-role teardown ownership and stale-registration cleanup.
- `RequestMmio` / `GrantDma` call paths and exit-time MMIO/BDF/IOMMU release.
- Existing descriptor and boundary coverage for duplicate driver IDs, invalid board combinations, and MMIO overlap rejection.

## Findings

1. Driver-role teardown is now single-owner and complete for the shared lifecycle slice.
   - `kernel/src/task/drivers/driver_cell.rs:143-153` centralizes input/block/NIC/GPU deregistration under `deregister_all_for`.
   - `kernel/src/task/scheduler.rs:568-577` makes scheduler exit the sole shared teardown owner after service-registry cleanup.
   - `kernel/src/task/syscall.rs:2330-2346` and `kernel/src/cell/hotswap.rs:311-324` still release MMIO, BDF, and IOMMU state on force-exit and hotswap retirement; they no longer redundantly pre-clear driver roles.

2. NIC IRQ ownership is fail-closed and stale-cache cleanup is covered.
   - `kernel/src/task/drivers/driver_cell.rs:42-59` derives NIC IRQ ownership only from a proven owned VirtIO slot.
   - `kernel/src/task/drivers/driver_cell.rs:74-83` publishes NIC owner + IRQ together.
   - `kernel/src/task/drivers/driver_cell.rs:177-210` tests exact-TID teardown and stale IRQ cache clearing.
   - `kernel/src/task/drivers/virtio_common.rs:175-185` consumes only the current proven NIC IRQ state.

3. MMIO, DMA, BDF, and IOMMU substrate already satisfy the shared-phase gate.
   - `kernel/src/resource_registry.rs:172-231` enforces bounded `RequestMmio` allowlists plus overlap rejection for both checked and unchecked claim paths.
   - `kernel/src/task/syscall.rs:3244-3299` gates `GrantDma` on alignment, overflow, BDF ownership, quota, and pin-before-map.
   - `kernel/src/resource_registry.rs:285-287` releases BDF ownership by TID; `kernel/src/task/syscall.rs:2249-2256` and `2339-2346` pair that with IOMMU cleanup on exit paths.
   - `kernel/src/task/drivers/iommu.rs:59-69` keeps teardown contract explicit: cleanup must precede frame release.

4. Existing tests/scripts already cover the remaining Phase 02 checklist items.
   - Duplicate driver IDs: `boards/src/descriptor.rs:150-153`, tested by `boards/src/descriptor_tests.rs:128-145`.
   - Typed driver selection and board catalog integrity: `boards/src/descriptor.rs:124-128`, tested by `boards/src/catalog_tests.rs:42-84`.
   - Invalid board feature combinations: `bash scripts/check-board-configs.sh` intentionally asserts conflicting board features fail closed.
   - MMIO overlap rejection: `kernel/src/resource_registry.rs:220-227` and `172-188`.

## Verification

- `cargo test -p cellos-boards --lib --target x86_64-unknown-linux-gnu`
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu`
- `bash scripts/check-hal-boundaries.sh`
- `bash scripts/check-board-configs.sh`
- `git diff --check`

Notes:

- `bash scripts/check-board-configs.sh` passed while exercising expected-negative conflicting-board compile checks.
- Kernel host unit tests for `driver_cell` remain pre-blocked by unrelated std/no_std harness issues already logged in Phase 02's deviation log, so this closure relies on target compile gates plus the in-module unit coverage added in the slice.

## Closure decision

No shared-substrate blocker remains that requires Phase 02-owned edits before Phase 03.
New controller-specific `DriverId` additions should be introduced only in the consuming phases:

- Phase 03 for RPi3 BCM I2C/SPI controller bring-up.
- Later phases only when a concrete shared driver consumer lands.
