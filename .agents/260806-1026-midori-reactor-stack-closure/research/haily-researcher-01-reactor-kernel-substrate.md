## Kernel-owned CQ is real, bounded, and per-cell
**Verdict:** The kernel substrate exists: completions land in kernel-owned per-cell queues, not reclaimable grants.
- `CompletionQueue` is kernel heap memory held through the task record specifically to avoid grant unregister/free races during in-flight completion writes. [kernel/src/task/completion.rs:1-9]
- Capacity is fixed at 32 slots, reservations happen at submission, and append never allocates; that is real backpressure, not a speculative design note. [kernel/src/task/completion.rs:40-45] [kernel/src/task/completion.rs:131-158]
- The queue is stored in `Task.completion` and lazily shared across all threads in one cell via `queue_for()`. [kernel/src/task/tcb.rs:374-380] [kernel/src/task/completion.rs:318-339]
- Boot self-test proves round-trip, per-cell sharing, bounded refusal, and deferred wake behavior. [kernel/src/task/completion_selftest.rs:1-14] [kernel/src/task/completion_selftest.rs:130-178] [kernel/src/task/completion_selftest.rs:186-257]
**Source:** [kernel/src/task/completion.rs](/home/dmin/cellos/kernel/src/task/completion.rs:1)

## `WaitCompletion` is still NET_RX-only
**Verdict:** The ABI surface is not a generic reactor wait yet; the syscall hard-rejects every source except `NET_RX`.
- `wait_completion()` returns `InvalidInput` unless `mask == NET_RX`. [kernel/src/task/completion_wait.rs:73-77]
- The public ABI still advertises one completion source bit today: `events::NET_RX`. [libs/api/src/abi/syscall.rs:435-455] [libs/api/src/abi/syscall.rs:833-840]
- The current roadmap and architecture docs now state the same narrowed truth: verified NET_RX substrate, deferred generic reactor / peer-death CQ / `RecvScatter` / async VFS-DMA. [docs/project-roadmap.md:138-139] [docs/system-architecture.md:1000-1002] [docs/project-changelog.md:30-42]
**Source:** [kernel/src/task/completion_wait.rs](/home/dmin/cellos/kernel/src/task/completion_wait.rs:67)

## There is a production consumer, but no production `signal_net_rx()` producer
**Verdict:** The net service does call `WaitCompletion(NET_RX)`, but no live IRQ path completes that reservation today.
- The net service declares `WaitCompletion` and parks on `sys_wait_completion(NET_RX, timeout_ticks)` in its main loop. [cells/services/net/src/main.rs:17-33] [cells/services/net/src/main.rs:173-185]
- `signal_net_rx()` exists and the NET_RX reservation self-test exercises it, but grep finds only self-test call sites outside the implementation. [kernel/src/task/waker.rs:78-105] [kernel/src/task/net_rx_selftest.rs:47-58] [kernel/src/task/net_rx_selftest.rs:84-100]
- The real VirtIO IRQ path routes through `irq_wait::signal_irq()` and returns; it does not call `signal_net_rx()`. [kernel/src/task/drivers/virtio_common.rs:100-105]
- The VirtIO net Driver Cell still uses polling `try_recv()`, while its IRQ-driven `wait_recv()` path is explicitly dormant until wired. [cells/drivers/virtio-net/src/device.rs:112-118] [cells/drivers/virtio-net/src/device.rs:150-159] [cells/drivers/virtio-net/src/dispatch.rs:69-80]
**Source:** [cells/services/net/src/main.rs](/home/dmin/cellos/cells/services/net/src/main.rs:173)

## No parked executor exists above the substrate
**Verdict:** The queue can wake a parked task, but the userland async runtime still busy-yields; there is no real reactor-integrated executor.
- `ostd::executor::block_on()` still constructs `dummy_raw_waker()` and loops `sys_yield()` on `Poll::Pending`. [libs/ostd/src/executor.rs:7-31] [libs/ostd/src/executor.rs:34-44]
- `ostd::ipc::AsyncRecv` still polls `sys_try_recv()` and returns `Pending` on empty, relying on that busy-yield executor rather than a parked wait primitive. [libs/ostd/src/ipc.rs:80-104]
- The phase file's current success criteria still require removing `dummy_raw_waker` and proving one-thread multi-source service before claiming reactor completion. [.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md:223-239]
**Source:** [libs/ostd/src/executor.rs](/home/dmin/cellos/libs/ostd/src/executor.rs:7)

## Peer-death completion is absent; death delivery still rides `Recv`
**Verdict:** Peer-death remains on the old `NotifyOnExit` + `Recv` path, and the current completion ABI has no target-generation contract.
- `exit_task` wakes watchers already parked in `TaskState::Recv` or queues `(dead_tid, exit_reason)` into `pending_deaths`; it does not append to a completion queue. [kernel/src/task/scheduler.rs:550-570]
- `Syscall::Recv` drains queued deaths first and returns the dead tid as the sender; this is still the production death-notification mechanism. [kernel/src/task/syscall.rs:1411-1437]
- `Task` stores `pending_deaths` / `pending_exit_reason` for `Recv`-resume delivery, while completion wait registration tracks only a waiter `tid`. [kernel/src/task/tcb.rs:188-202] [kernel/src/task/completion_wait.rs:26-43]
- The completion record carries only `slot` and `result`; reserved bytes are unused today, and the roadmap explicitly calls out deferred “peer-death CQ target-generation ABI”. [libs/api/src/abi/completion.rs:44-63] [docs/project-roadmap.md:138]
**Source:** [kernel/src/task/scheduler.rs](/home/dmin/cellos/kernel/src/task/scheduler.rs:550)

## `Recv` compatibility is still the hard blocker for a broader reactor
**Verdict:** Existing service behavior still depends on `TaskState::Recv`, so swapping blocking paths onto CQ parking would regress real flows.
- Shell async input intentionally uses `sys_recv_timeout` so the shell enters `TaskState::Recv`; the comments state `sys_try_send` otherwise drops input events. [cells/tools/shell/src/async_utils.rs:13-18] [cells/tools/shell/src/async_utils.rs:36-44]
- `RecvTimeout` and `TryRecv` both drain `pending_msgs` first to preserve delivery for non-Recv periods; this is mailbox compatibility glue, not a reactor migration. [kernel/src/task/syscall.rs:1686-1718] [kernel/src/task/syscall.rs:1792-1828]
- `RecvScatter` is explicitly marked as a known pre-existing defect whose real repair needs separate blocking/lifecycle work. [kernel/src/task/syscall.rs:1619-1629]
- VFS `ReadFileGrant` still relies on synchronous `ipc_call` blocking so the caller cannot free the grant before copy completion. [cells/services/vfs/src/dispatch.rs:305-310]
**Source:** [cells/tools/shell/src/async_utils.rs](/home/dmin/cellos/cells/tools/shell/src/async_utils.rs:13)

## Ranked route: ABI-free stop vs ABI-changing reactor work
**Verdict:** Rank 1 is the ABI-free stop: keep Phase 07 closed only as a NET_RX kernel substrate; rank 2 is the ABI-changing reactor route if and only if generic async remains a near-term product goal.
- **1. Recommended: ABI-free, narrow closure.** Current code and docs already agree on the honest boundary: kernel-owned CQ + `WaitCompletion(NET_RX)` + self-tests are real, while generic reactor, parked executor, peer-death CQ, and `RecvScatter` readiness are not. That route preserves the working `Recv`/`NotifyOnExit` contract and avoids destabilizing shell/input/VFS semantics. [docs/project-changelog.md:30-42] [docs/project-roadmap.md:138-139] [kernel/src/task/syscall.rs:1411-1437] [cells/tools/shell/src/async_utils.rs:36-44]
- **2. Higher-risk alternative: ABI-changing reactor.** To make CQ the general wait substrate, the tree must first lift the `NET_RX` mask gate, add more than `slot/result` semantics to completions or waiter registration for target-gone/generation safety, and replace executor busy-yield with a real park/wake contract. That is ABI work, not a bugfix. [kernel/src/task/completion_wait.rs:73-77] [libs/api/src/abi/syscall.rs:833-840] [libs/api/src/abi/completion.rs:44-63] [libs/ostd/src/executor.rs:7-31]
- **Adoption risk:** the ABI-free route is low-risk and architecture-consistent; the ABI-changing route is medium/high risk because it touches syscall/event ABI, same-cell wake semantics, and grant-safety assumptions in existing services. [docs/specs/17-ipc-wire-contract.md:79] [docs/specs/17-ipc-wire-contract.md:330-367] [cells/services/vfs/src/dispatch.rs:305-310]
**Source:** [docs/project-roadmap.md](/home/dmin/cellos/docs/project-roadmap.md:138)
