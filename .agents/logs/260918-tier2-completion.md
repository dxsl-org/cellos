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
