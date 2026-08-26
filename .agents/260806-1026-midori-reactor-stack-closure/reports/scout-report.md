# Scout Report: Midori Reactor + Stack Closure

## Relevant Files

- `.agents/reports/HANDOFF-260731.md:160-167` orders pickup as A1, A4, A2/A3, then D8 onward; this plan preserves that by limiting itself to verification-only Midori closure and not starting a new feature program.
- `.agents/260805-1833-midori-closure-execution/plan.md:25-27` says Phase07 is NET_RX-only substrate and Phase08 is baseline-only.
- `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:20-21` records verified substrate but no generic reactor, real producer, peer-death CQ, Recv migration, or parked executor.
- `.agents/260727-2101-midori-lessons-cellos/phase-08-stack-sizing-table.md:19-21` records default 64-page stacks and blocks production shrink on parked executor/generic wait plus stronger overflow protection.
- `docs/specs/03b-async-reactor-adr.md:127-145` requires one new syscall only with the first migration, preserves Recv or migrates it in the same change, and blocks stack sizing until the caller-stack-pinned shim changes.
- `docs/code-standards.md:12-16` defines Law 1 for `libs/api/` and `libs/types`; `docs/code-standards.md:270-289` defines runtime evidence as required for done.

## Current Reactor Facts

- `kernel/src/task/completion.rs:95-158` implements a bounded kernel-owned per-cell completion queue with submit-time reservation and append-time no-allocation.
- `kernel/src/task/completion_wait.rs:73-77` hard-rejects every completion source except `NET_RX`; `libs/api/src/abi/syscall.rs:831-840` exposes only `NET_RX` as a completion event bit.
- `cells/services/net/src/main.rs:173-185` is a production consumer of `sys_wait_completion(NET_RX, timeout)`.
- `kernel/src/task/waker.rs:70-105` exposes `signal_net_rx`, but grep found non-test callers only absent; `kernel/src/task/drivers/virtio_common.rs:100-105` routes VirtIO IRQs to `irq_wait::signal_irq`, not `signal_net_rx`.
- `libs/ostd/src/executor.rs:9-31` and `libs/ostd/src/executor.rs:36-44` still use stack-pinned busy-yield futures with `dummy_raw_waker`.
- `cells/tools/shell/src/async_utils.rs:36-44` still relies on `RecvTimeout` to enter `TaskState::Recv`; `kernel/src/task/syscall.rs:1619-1629` marks `RecvScatter` lifecycle repair as separate work.

## Current Stack Facts

- `kernel/src/task.rs:41-43` keeps `STACK_PAGES = 64`; `kernel/src/task.rs:223-225` makes `stack_pages_for(_name)` return that default for all paths.
- Live spawn paths route through `stack_pages_for`: `kernel/src/task.rs:871-875`, `kernel/src/task.rs:1896-1899`, `kernel/src/task/scheduler.rs:210-213`, `kernel/src/task/scheduler.rs:357`.
- The old memset-overrun hazard is fixed: stack zeroing derives from the handed-in stack at `kernel/src/task/scheduler.rs:241-259` and `kernel/src/task/scheduler.rs:381-392`.
- `kernel/src/task/stack.rs:55-87` provisions one guard page; `kernel/src/task/stack.rs:159-182` verifies the bottom guard is unmapped, but no second guard/probe policy exists.
- Test-hooks watermarking exists at `kernel/src/task/stack.rs:225-253`, and baseline marker emission is limited to init/shell/vfs/vfs-test at `kernel/src/task.rs:227-300`.
- `docs/project-changelog.md:18-28`, `docs/project-roadmap.md:30-35`, and `docs/system-architecture.md:1001-1002` accurately state current Phase07/08 boundaries.

## Precedents

- `2c2c81e2 feat(kernel): add a per-cell completion queue for asynchronous work` touched `kernel/src/task/completion.rs`, `kernel/src/task/completion_selftest.rs`, `kernel/src/task/tcb.rs`, `kernel/src/main.rs`, and `kernel/src/task.rs`.
- `49a15348 feat(kernel): move the NET_RX wait onto the completion queue` touched `cells/services/net/src/main.rs`, `kernel/src/task/completion_wait.rs`, `kernel/src/task/waker.rs`, `libs/api/src/abi/completion.rs`, `libs/api/src/abi/syscall.rs`, and `libs/ostd/src/syscall.rs`.
- `56cba9cf feat(kernel): prepare stack sizing evidence gate` touched `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/stack.rs`, `scripts/build-test-hooks-ci.sh`, and `tests/integration/tests/vfs-quota.rs`.
- Blind spot candidate: `kernel/src/task/drivers/driver_cell.rs` and `cells/drivers/virtio-net/src/device.rs`; producer work needs NIC driver TID/IRQ ownership, not only net service changes.

## Prior Failures

- `.agents/failure-history.jsonl` and `.agents/incidents/` had no matching local entries for reactor/completion/stack keywords.
- Memory-derived PRIOR constraint, re-verified in code/docs: no ABI expansion or completion claim without runtime evidence.

## Blast Radius

- ABI-free reactor producer: `kernel/src/task/drivers/driver_cell.rs`, `kernel/src/task/drivers/irq_wait.rs`, `kernel/src/task/drivers/virtio_common.rs`, `kernel/src/task/waker.rs`, `cells/drivers/virtio-net/src/device.rs`, `cells/drivers/virtio-net/src/dispatch.rs`, `cells/services/net/src/main.rs`, integration tests.
- Law 1 gated public contract: `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/completion.rs`, `libs/api/src/abi/syscall_tests.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/executor.rs`.
- Recv compatibility: `kernel/src/task.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/scheduler.rs`, `cells/tools/shell/src/async_utils.rs`, `cells/services/vfs/src/dispatch.rs`.
- Stack sizing: `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/stack.rs`, `tests/integration/tests/vfs-quota.rs`, `scripts/build-test-hooks-ci.sh`.

## Tooling Notes

- `docs/coding.md` and `docs/engineering-standards.md` are absent in this checkout; planning uses `docs/code-standards.md`.
- Active plan sync could not run: WSL has no `node`, and `.claude/scripts/set-active-plan.cjs` is absent.
