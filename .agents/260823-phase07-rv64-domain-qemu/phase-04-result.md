# Phase 04 Result — Bounded copied IPC

**Status:** success
**Model tier used:** thinking (session orchestration) + specialist agents
**Outcome:** Implementation and verification closed.
- Replaced borrowed/identity pointer message storage with bounded `IpcWireMessage` (maximum payload 4096 bytes) and scalar sender identity (`IpcWireHeader`).
- Integrated Phase 03 recoverable user-copy boundary into all IPC paths (`Syscall::Send`, `Syscall::TrySend`, `Syscall::Recv`, `Syscall::TryRecv`, `Syscall::RecvTimeout`, `Syscall::RecvScatter`).
- Added atomic multi-destination `copy_to_user_scatter` (`user_copy/scatter.rs`) and `write_scatter` (`copy_glue/scatter.rs`) to guarantee all destination ranges are staged and pinned before any bytes commit to user memory, preventing partial mutations on unmapped/read-only later iovecs.
- Exercised syscall-level `Syscall::RecvScatter` atomicity and pending-message retention regression test (`ipc_wire_selftest/scatter_case.rs` -> `S22-RV64-IPC-SCATTER: PASS`), confirming earlier destination buffers remain unmutated and the pending message remains queued upon a later unmapped destination fault.
- Preserved request generation, current-caller context at dequeue/commit, timeout, service-death, and VFS lease lifecycle invariants.
- Fixed UART line serialization via `LOG_LOCK` in the UART driver to prevent multi-hart character interleaving.
- Modularized codebase to strictly satisfy the <200-line repo rule: `user_copy/` (6 submodules, max 172 lines), `copy_glue/` (2 submodules, max 185 lines), and `ipc_wire_selftest/` (4 submodules, max 153 lines).
- All self-tests passed cleanly: `IPC-PENDING`, `IPC-GUARDRAILS`, `VFS-LIFETIME`, `SMP-FAULT-RETIREMENT`, and full QEMU test suites.
**Files changed:**
- `kernel/src/task/ipc_wire.rs`
- `kernel/src/task/ipc_wire_selftest/` (`mod.rs`, `copy_case.rs`, `scatter_case.rs`, `race_case.rs`)
- `kernel/src/task/user_copy/` (`mod.rs`, `copy.rs`, `guard.rs`, `range.rs`, `scatter.rs`, `sv39_probe.rs`)
- `kernel/src/task/user_copy_tests.rs`
- `kernel/src/task/copy_glue/` (`mod.rs`, `scatter.rs`)
- `kernel/src/task/syscall.rs`
- `kernel/src/task/pending_mailbox.rs`
- `kernel/src/task/tcb.rs`
- `kernel/src/task/tests.rs`
- `kernel/src/task.rs`
- `kernel/src/cell/hotswap.rs`
- `kernel/src/main.rs`
- `kernel/src/task/drivers/gpio_irq.rs`
- `kernel/src/task/drivers/uart.rs`
- `kernel/src/task/ipc_guardrail_selftest.rs`
- `kernel/src/task/ipc_pending_selftest.rs`
- `kernel/src/task/vfs_lifecycle_selftest.rs`
- `kernel/src/task/context_handoff_selftest.rs`
- `scripts/qemu-native-domain-test.sh`
**Residual risk:** none known.
**Test signal:**
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features native-domains,test-hooks`: PASS (0 errors, 0 warnings)
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`: PASS (0 errors, 0 warnings)
- `cargo clippy -p cellos-kernel --target riscv64gc-unknown-none-elf --features native-domains,test-hooks -- -D warnings`: PASS (0 warnings)
- `cargo clippy -p cellos-kernel --target riscv64gc-unknown-none-elf -- -D warnings`: PASS (0 warnings)
- 1-hart QEMU suite: `S22-RV64-QEMU-SUITE: PASS HARTS=1 CASES=switch,sas-fastpath,user-copy,ipc-copy`
- 2-hart QEMU suite: `S22-RV64-QEMU-SUITE: PASS HARTS=2 CASES=switch,sas-fastpath,migration,user-copy,user-copy-race,ipc-copy,ipc-copy-race`
- Syscall-level markers: `S22-RV64-IPC-COPY: PASS`, `S22-RV64-IPC-NO-PEER-MAP: PASS`, `S22-RV64-IPC-SCATTER: PASS`, `S22-RV64-IPC-COPY-RACE: PASS`
**Assumption-invalidated:** false
