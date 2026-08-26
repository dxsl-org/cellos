---
phase: 5
title: "Parked Executor Shim"
status: completed
priority: P1
effort: "1d"
dependencies: [4]
tier: thinking
---

# Phase 05: Parked Executor Shim

## Overview

Completed on 2026-08-06. `ostd::executor::block_on()` now parks per executor through an `Arc`-backed `RawWaker`, uses a bounded TIMER wait instead of the busy-yield loop, keeps independent monotonic-ms sleep deadlines, and fails loud on authority mismatch. Shell `Recv` stayed unchanged, the NET_RX proof stayed intact, and the exact parked marker was rerun after the final fallback-only tweak.

## Closure

- Per-executor parking is bounded and no longer relies on `dummy_raw_waker`.
- Sleep deadlines remain monotonic and independent of the parked TIMER wait path.
- The broad shell/input/DHCP/TCP/VFS and peer-death lanes were run before the final fallback-only tweak.
- Final exact marker rerun: `[executor] dummy-waker=absent executor=parked source=TIMER PASS`.
- Review verdict: APPROVE.

## Requirements

- Functional: remove `dummy_raw_waker` from `libs/ostd/src/executor.rs`.
- Functional: block pending futures through `WaitCompletion` or equivalent generic wait instead of `sys_yield()` loops.
- Functional: preserve shell input and existing blocking syscalls; no Recv migration.
- Non-functional: no async VFS/DMA; no cancellable grant operation.

## Architecture

Data flow: future poll returns `Pending`, executor registers the current waiter, kernel parks the task, a completion source wakes it, executor drains completion and polls again. Existing `RecvTimeout` shell path continues separately through `TaskState::Recv`.

## Assumptions

- **Claim:** The executor can park without changing existing cell application code.
  **Confidence:** medium
  **How to verify:** Run shell, net, vfs, and app-init QEMU suites after replacing `block_on`.

## Related Files

- Modify: `libs/ostd/src/executor.rs`
- Modify: `libs/ostd/src/syscall.rs`
- Modify: `libs/ostd/src/ipc.rs`
- Modify: `cells/tools/shell/src/main.rs`
- Modify: `cells/tools/shell/src/async_utils.rs`
- Modify: `cells/services/net/src/main.rs`
- Modify: `tests/integration/tests/boot.rs`

## Implementation Steps

1. Implement a waker that records the current completion source and uses the Phase04 wait wrapper.
2. Replace the `Poll::Pending => sys_yield()` loop with a park path that cannot self-wake from a withdrawn reservation.
3. Keep `recv_async` on its current polling behavior unless Phase04 explicitly authorized a Recv-compatible wait source.
4. Run the Phase02 shell burst and dead-peer guards before stack measurement work begins.
5. Add a QEMU marker reporting `dummy_raw_waker=absent` and `executor=parked`.

## Success Criteria

- [x] `grep -RIn "dummy_raw_waker" libs/ostd/src kernel/src/task` returns no userland executor copy.
- [x] Shell burst, input, DHCP, TCP, VFS, and peer-death lanes pass on RV64.
- [x] CPU-yield loop is absent from `block_on`.

## Verification

- `cargo fmt --all --check`: PASS.
- `git diff --check`: PASS.
- RV64 `ostd`, `app-shell`, and `service-net` checks: PASS.
- Fresh QEMU parked marker: PASS.
- Exact QEMU rerun: PASS, `[executor] dummy-waker=absent executor=parked source=TIMER PASS`.
- Stale manual nightly-2025 failure note rejected; `rust-toolchain.toml` pins `nightly-2026-05-01`.

## Validation Commands

```bash
cargo fmt --all --check
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
pwsh ./gen_disk.ps1
cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test boot -- --test-threads=1 input_keyboard_e2e network_dhcp_acquires_ip network_tcp_send_recv vfs_write_echo_redirect
```

## Security Considerations

Do not make grant-backed IPC cancellable in this phase. If a future can be dropped while VFS still writes a grant, SAS corruption returns.

## Risk Notes

- High x High: executor park breaks shell input because shell no longer enters `Recv`. Mitigation: do not migrate Recv; keep Phase02 burst test mandatory.
- High x Medium: withdrawn reservation cancels the next park. Mitigation: preserve `CompletionQueue::release` no-wake contract.
- Rollback: revert `libs/ostd` and touched cell/test files; Phase04 contract may remain unused. Irreversible part: none if ABI from Phase04 has not shipped externally.

## Deviation Log

2026-08-06 — closure complete; executor source changes landed with the bounded TIMER park and unchanged shell `Recv` path.
