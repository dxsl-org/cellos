# Phase 03 Result: Recoverable Domain-Aware User Copy

## Status: COMPLETED

### Summary of Implementation
1. **Checked User Copy Boundary (`kernel/src/task/user_copy/`)**:
   - `UserReadSlice` / `UserWriteSlice`: Typed range validation enforcing non-null, bounded length, overflow protection, SV39 canonical user VA range limits (`0 < ptr <= ptr + len <= 0x003f_ffff_ffff`).
   - `copy_from_user` / `copy_to_user`: Two-pass atomic transaction orchestration (`execute`) ensuring destination memory is never modified on probe or copy failure.
   - `stage_domain` / `sv39_probe`: Pre-validation of domain private pages, grant mappings, and user permissions (`SV39_VALID | SV39_USER | SV39_READ/WRITE`), validating every page before touching bytes.
   - `PinnedCopy`: Acquires domain reader pin (`CopyReader`) preventing concurrent teardown/revocation during copy window.
   - `guarded_byte_copy`: Per-hart recoverable assembly landing pad (`3:`) catching page faults inside armed `GuardWindow` (`user_copy_guard_resume_pc`, `user_copy_guard_start`, `user_copy_guard_end`, `user_copy_guard_active`).

2. **Syscall ABI Audit & Cutover (`kernel/src/task/syscall.rs`, `kernel/src/task/copy_glue.rs`)**:
   - Replaced all raw user pointer dereferences and `from_raw_parts` in pointer-bearing syscall arms (`Read`, `Write`, `ReadLog`, `GetProcs`, `GetProcs2`, `MemInfo`, `PciEnumerate`, `PciBarRead`, `SpawnFromMem`, `IpcSend`, `IpcRecv`, `TryRecv`, `RecvScatter`, `Lend`, `BorrowRead`, `BorrowWrite`).
   - Unified SAS and Domain user memory copies under `TaskCopyView`.

3. **SMP Lifecycle, Scheduler & Retirement Fixes**:
   - Fixed `completion_selftest` and `thread_*_selftest` task generation/root_tid metadata alignment so all cell owners register and deregister cleanly with fail-closed sender context.
   - Relocated SMP self-tests before `spawn_trusted_init` to prevent race conditions with boot background tasks.
   - Wired native-domain pre-switch hooks (`hold_after_selection_before_switch`, `observe_heartbeat_terminal_current`) into `__switch` orchestration.
   - Registered frame allocator bitmap storage into domain supervisor registry so domain page tables map all kernel-owned frames cleanly.

### Verification Evidence
- `cargo clippy -p cellos-kernel --target riscv64gc-unknown-none-elf --features native-domains,test-hooks -- -D warnings`: Clean (0 errors, 0 warnings).
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`: Clean.
- `bash scripts/qemu-native-domain-test.sh --harts 1 --case switch,sas-fastpath,user-copy`:
  - `S22-RV64-SWITCH: PASS harts=1`
  - `S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts=1`
  - `S22-RV64-COPY: PASS harts=1`
  - `S22-RV64-QEMU-SUITE: PASS HARTS=1 CASES=switch,sas-fastpath,user-copy`
- `bash scripts/qemu-native-domain-test.sh --harts 2 --case switch,migration,user-copy-race`:
  - `S22-RV64-SWITCH: PASS harts=2`
  - `S22-RV64-MIGRATION: PASS harts=2`
  - `S22-RV64-COPY-RACE: PASS harts=2`
  - `S22-RV64-QEMU-SUITE: PASS HARTS=2 CASES=switch,migration,user-copy-race`
- Zero occurrences of `FAIL` or `PANIC` across all 1-hart and 2-hart QEMU test logs.
