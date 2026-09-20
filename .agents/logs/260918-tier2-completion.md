# 2026-09-18 — Tier 2 Native Domain Cell Runtime & Verification Complete

## Why
Tier 2 (Native Domain Cell) is CellOS's hardware MMU-isolated execution boundary for unsigned native code, FFI runtimes, and arbitrary native binaries. While the RV64 private-root substrate and negative page-fault containment test (`tier2-exploit`) existed, Tier 2 lacked:
1. Positive end-to-end execution verification proving that a benign Tier 2 cell can execute user code, allocate heap memory, invoke system calls, and terminate cleanly with code 0 under private SATP isolation.
2. Tooling integration for the VFS `/bin` cell-store (`PART_CELLSTORE_BASE_LBA = 1_062_144`), which caused post-boot shell launches of dynamic cells to report `command not found`.

## What landed
1. **Positive Tier 2 Test Cell (`cells/tests/tier2-smoke`)**:
   - Declares `tier = api::manifest::PROTECTION_CLASS_UNTRUSTED` and `#![forbid(unsafe_code)]`.
   - Admitted to Tier 2 Paged Domain under private Sv39 page table (`satp`).
   - Verifies dynamic heap allocation on its private arena (`Vec len=20, sum=1050`).
   - Verifies formatted `String` allocations.
   - Exercises scheduler yield and clean exit with code 0.

2. **Integration Test Suite Extension (`tests/integration/tests/tier2_fault_isolation.rs`)**:
   - `tier2_hardware_page_fault_terminates_cell_cleanly`: Proves negative containment (deliberate NULL pointer write triggers CPU Page Fault, kernel catches trap, terminates cell, shell remains interactive).
   - `tier2_positive_execution_runs_cleanly`: Proves positive execution (cell admits to Tier 2, passes all runtime assertions, exits code 0, shell remains interactive).
   - Both tests pass cleanly (16.19s execution time).

3. **Dual-Store Synchronization in `tools/add-cell-to-disk.py`**:
   - Updates both the early bootstrap cell table at LBA 526,336 and the standalone FAT16 cell-store at LBA 1,062,144 (`PART_CELLSTORE_BASE_LBA`).
   - Ensures dynamic cell spawns resolve cleanly across both early-boot and post-boot VFS `/bin` lookup paths.

4. **Kernel Launch Profile & Boot Ceiling**:
   - Registered `/bin/tier2-smoke` in `kernel/src/loader/launch_profile/targets.rs` and `kernel/src/loader/boot_ceiling.rs` with `CapSet::EMPTY`.

## Evidence
- `cargo test --target x86_64-unknown-linux-gnu --test tier2-fault-isolation`: 2 passed; 0 failed.
- `scripts/qemu-native-domain-test.sh --harts 1 --case switch,resume-root,sas-fastpath,user-copy,ipc-copy,admission,rollback,grant-revoke`: ALL PASS.
- `scripts/qemu-native-domain-test.sh --harts 2 --case migration,user-copy-race,ipc-copy-race`: ALL PASS.
- `bash scripts/check-baseline.sh`: PASS (0 compiler errors, 0 clippy warnings).
- `cargo fmt --all --check`: Clean (0 diffs).
- `python3 scripts/cellos-sign --check`: F1/F5 policy check PASS (90 crates, 602 files).

## Multi-Architecture Expansion (AArch64)
1. **AArch64 Linker Script & Section Layout (`kernel/linker-aarch64.ld`)**:
   - Bounded `__domain_text_start` / `__domain_text_end` for `.text.boot`, `.text.vectors`, and `.text`.
   - Consolidated `.rodata`, `.requests_*`, and `.eh_frame*` under `__domain_readonly_start` / `__domain_readonly_end`.
   - Consolidated `.data`, `.sdata`, `.got`, and `.requests` under `__domain_writable_start` / `__domain_writable_end`.
   - Eliminated orphan sections between read-only and writable segments, ensuring zero unmapped holes in domain supervisor address space.

2. **AArch64 Domain Supervisor Registry & Platform MMIO (`kernel/src/main.rs`, `kernel/src/memory/address_space.rs`)**:
   - Activated `domain_supervisor_registry` and registered static image, heap, and frame allocator bitmap on AArch64.
   - Added `SupervisorRangeKind::DeviceMmio` with `PageFlags::DEVICE` mapping attributes.
   - Registered GIC (`0x0800_0000..0x0802_0000`) and peripheral MMIO (PL011 UART, RTC, PL061: `0x0900_0000..0x0902_0000`) on QEMU virt and BCM2837 on RPi3.

3. **AArch64 TTBR0 Trap Entry/Exit Protocol (`hal/arch/arm/src/aarch64/trap.rs`, `hal/arch/arm/src/aarch64/domain.rs`)**:
   - Recorded kernel SAS root in `VI_KERNEL_TTBR0` upon early paging activation.
   - Added `ttbr0_el1` slot to `TrapFrame` at offset 280 (36 * 8 byte frame alignment).
   - In `vt_sync_el0` and `vt_irq_el0`: saved interrupted `ttbr0_el1`, installed `VI_KERNEL_TTBR0`, and restored `ttbr0_el1` before `eret`.
   - Enabled safe execution of kernel syscalls, grant allocations, memory zeroing, and IRQ handlers under `KERNEL_ROOT` while retaining full user-mode isolation in Tier 2 domains.

4. **Integration Test Suite Extension (`tests/integration/tests/aarch64-boot.rs`)**:
   - Added `aarch64_tier2_smoke_positive_execution`: validates Tier 2 admission under TTBR0 isolation, heap allocation, string formatting, yield, grant register, and clean exit on AArch64.
   - Added `aarch64_tier2_fault_isolation`: validates CPU translation fault on illegal NULL write, clean cell termination by kernel, and shell interactive recovery.
   - Both tests pass:
     - `aarch64_tier2_smoke_positive_execution`: PASS (8.29s).
     - `aarch64_tier2_fault_isolation`: PASS (7.77s).
     - All 5 RISC-V Tier 2 tests continue to pass with zero regressions (9.95s).
