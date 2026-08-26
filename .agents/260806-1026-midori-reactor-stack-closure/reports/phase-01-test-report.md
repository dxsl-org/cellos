# Test Report - 2026-08-06 - Phase 01 final cleanup verification

Mode: diff-aware - 12 changed files
Mapped: `tests/integration/tests/boot.rs` (network boot lane), `tests/integration/tests/hotswap-smoke.rs` (bounded cleanup witness)
Unmapped: `cells/drivers/virtio-net/src/main.rs`, `cells/services/net/src/handlers.rs`, `cells/services/net/src/interface.rs`, `cells/services/net/src/main.rs`, `hal/arch/riscv/src/rv64.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/task/drivers/driver_cell.rs`, `kernel/src/task/drivers/input_irq_ack.rs`, `kernel/src/task/drivers/irq_wait.rs`, `kernel/src/task/drivers/virtio_common.rs`, `kernel/src/task/scheduler.rs` -> targeted unit coverage still desirable for IRQ gating, hotswap cleanup, and net-wake internals
Ran 6/6: 6 passed, 0 failed, 0 skipped
Coverage: unavailable (`cargo llvm-cov` missing; no line/branch percentage reported)
Build/typecheck: pass

Baseline delta: 4 fail->pass on the network lane, 0 pass->fail, no new failures vs baseline
Signal: strong. The four network lanes are green, `network_dhcp_acquires_ip` passed the `[net-rx-producer] irq->completion PASS` gate, and the hotswap smoke lane passed as a bounded cleanup witness. The harness does not echo the full serial transcript on success, so the proof is from the passing assertions rather than printed boot logs.
Source/test evidence for no stale NIC cache path: `kernel/src/task/scheduler.rs` and `kernel/src/cell/hotswap.rs` deregister the NIC driver on exit, `kernel/src/task/drivers/driver_cell.rs` tracks the registered NIC IRQ/source, and `tests/integration/tests/boot.rs` waits for the producer marker before DHCP.

Host-gated limits:
- `cargo llvm-cov` is not installed, so coverage cannot be measured in this checkout
- `pwsh ./gen_disk.ps1` still reports optional `tetris-c` and `tetris-lua` build failures as warnings, but disk image generation completes
- No page-fault or restart-loop text surfaced in the captured rerun output

Cleanup:
- Restored regenerated tracked artifacts `kernel/src/embedded-test-hooks/init` and `kernel/src/embedded/init`
- Current worktree remains limited to the twelve intentional source edits above
