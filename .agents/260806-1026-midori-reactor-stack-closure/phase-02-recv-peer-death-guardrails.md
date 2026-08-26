---
phase: 2
title: "Recv And Peer-Death Guardrails"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
tier: thinking
---

# Phase 02: Recv And Peer-Death Guardrails

## Overview

Freeze the behaviors that generic reactor work must not break: shell input via `RecvTimeout`, peer death unblocking, and grant-copy safety assumptions.

## Requirements

- Functional: add tests/markers that catch shell input drops, dead-peer hangs, and `RecvScatter` lifecycle regressions.
- Functional: audit all unsafe VFS grant copies justified by blocking caller semantics.
- Non-functional: no executor replacement and no public ABI changes in this phase.

## Architecture

Data flow: input service `sys_try_send` enters pending mailbox or `TaskState::Recv`; shell exits through `sys_recv_timeout`; peer death flows through `exit_task`; VFS grant copies rely on caller still blocked until reply. This phase adds runtime proof around these flows before they are touched.

## Assumptions

None - all claims are from cited source and prior researcher reports.

## Related Files

- Modify: `tests/integration/tests/boot.rs`
- Modify: `tests/integration/tests/hotswap-smoke.rs`
- Modify: `kernel/src/main.rs`
- Modify: `kernel/src/task.rs`
- Modify: `kernel/src/task/scheduler.rs`
- Create: `kernel/src/task/ipc_guardrail_selftest.rs`
- Modify: `cells/tests/bench/src/bench-probe.rs`
- Modify: `cells/tests/bench/src/main.rs`
- Modify: `cells/tests/bench/src/scenarios/smp.rs`
- Read: `cells/tools/shell/src/async_utils.rs`
- Read: `cells/services/vfs/src/dispatch.rs`
- Read: `kernel/src/task/scheduler.rs`
- Read: `kernel/src/task/syscall.rs`

## Implementation Steps

1. Add a shell burst test that sends `hypha\n` after `=== ViCell shell ready ===` and asserts no input drop or hang.
2. Add a dead-peer runtime marker: kill or force-exit a service while a caller is waiting and assert the caller receives an error or timeout, not an infinite park.
3. Add a `RecvScatter` guard test proving its current known defect is still isolated and not silently converted to CQ behavior.
4. Grep unsafe VFS comments for "caller blocks" and list each grant-copy site in the test output or report.
5. Record all markers in docs/status text only if they pass on QEMU.

## Evidence

- `reports/harness/execution-evidence.json`: raw `hypha\n` burst reached the parser and returned the intentional policy denial; the boot selftest asserted `Ready`, `reply_value`, trap `a0`, and sender identity; the real QEMU lane passed with `blocked-send error + ForceExit notification`; `RecvScatter` stayed mailbox-owned; the VFS grant audit preserved the blocking-caller invariant.
- `reports/harness/verification.json`: formatting, RV64 kernel and app-bench checks/builds, test-hooks build, integration compile, both new QEMU guards, the three input regressions run sequentially, and the 120-second boot window all passed.

## Success Criteria

- [x] Shell input burst passes on RV64 and does not rely on line-oriented prompt reads.
- [x] Dead-peer marker fails loud on hang with a bounded timeout.
- [x] VFS grant-copy audit list is attached to the implementation report before Phase04 starts.

## Validation Commands

```bash
cargo fmt --all --check
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
pwsh ./gen_disk.ps1
cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test boot -- --test-threads=1 input_keyboard_e2e input_bare_cell console_long_line_with_backspace_no_stall
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
```

## Security Considerations

The VFS grant audit is a corruption boundary in SAS. A future cancellable grant operation must pin or own buffers before the blocking invariant is removed.

## Risk Notes

- High x High: tests accidentally prove only normal boot. Mitigation: use negative/death cases with bounded timeout.
- Medium x Medium: new tests are flaky under TCG. Mitigation: follow CI's serial QEMU discipline.
- Rollback: remove only added tests/markers. Irreversible part: none.

## Deviation Log

The planned additions to the already-full `ipc_pending_selftest.rs` and legacy manual
`task/tests.rs` were replaced by focused `ipc_guardrail_selftest.rs` proof code. Review
also exposed a real resume-path gap: `exit_task()` wrote trap `a0` but `Send` resumed
through `reply_value`, so the narrow production fix and stale-result reset landed in
`scheduler.rs` and `task.rs`. A dedicated app-bench role supplies deterministic QEMU
heartbeat-death and ForceExit evidence without changing public ABI or executor behavior.
