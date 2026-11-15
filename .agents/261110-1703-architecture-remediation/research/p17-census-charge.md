# Phase 17 census, part 17a: charging (S8 step 1)

Measured read-only on worktree `p17-quota` at `dfbbd2054c`. Paths are relative to `kernel/src/` unless they start with `hal/`, `libs/`, `cells/`, `scripts/` or `docs/`. **[I]** marks an inference that was not executed or observed at runtime. Everything else was read in the cited lines.

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| C1 | **The heap allocator charges the hart's dispatch identity.** `alloc_impl` reads `current_cell_id()` and charges it. The dispatcher writes that identity when it selects a task. | `memory/heap.rs:169-170`; `sched/scheduler.rs:2826-2829` |
| C2 | **Every Drop-side pin is dead code for accounting.** `dealloc` refunds the owner word stored in the block, whatever the current cell is. Seven of the production swap scopes only drop memory. | `memory/heap.rs:305-312`; §2.1 rows P4, P6, P8, P12, P20, P25 and the `release_pipe` user of P9 |
| C3 | **There are 31 production swap scopes in 11 files, not 14 or 20.** The 11 files are `ipc_wire`, `pending_mailbox`, `pipe`, `cell_quota`, `async_ipc`, `completion`, `spawn`, `task.rs`, `scheduler`, `launch` and `cell_session`. Two guard types exist: `KernelAllocationContext` and `PipeAllocationContext`. | §2.1 |
| C4 | **The timer ISR runs the whole scheduler pass under the interrupted cell's identity.** `vi_timer_tick` calls `frame_run::reap` and `yield_cpu`. `yield_cpu` runs the retirement funnel, zombie reaping and the sweep. All of these allocate, and several allocations are infallible. If the interrupted cell is at its limit, an infallible allocation reaches `alloc_error_handler`, which idles forever. **[I]**: not exercised by any lane. | `task.rs:1056-1058,1105`; `task.rs:1529,1536`; `sched/scheduler.rs:2419-2420,2554`; `task.rs:1884`; `memory/heap.rs:450-452` |
| C5 | **Explicit charges are generation-blind.** `cell_quota::refund` and `record_dma_unmapped` subtract from whatever incarnation holds the CellId now. Only the heap owner word is generation-checked. A pipe that outlives its creator refunds the next incarnation of that CellId. | `memory/cell_quota.rs:337-346,445-452`; `ipc/pipe.rs:137`; `memory/cell_quota.rs:351-361` |
| C6 | **Two generation notions exist.** A quota row has a 24-bit per-CellId registration count. A task has a global 64-bit `cell_generation`. Neither is derived from the other. | `memory/cell_quota.rs:68-70,186-188`; `task/tcb.rs:540-550,617` |
| C7 | **Most frames are uncharged.** Grant pages (up to 16 MiB per call, any number of calls), root stacks, cell segments, kernel-root and domain page tables, guest RAM, EPT/NPT/Stage-2 tables and VMCB/VMCS/xAPIC frames are not charged to anyone. Only thread stacks and SHM frames are charged, and both go to the **heap** counter. | §4 |
| C8 | **`FrameRun` names an owner but never a charge, and almost every run says `Kernel`.** Only SHM uses `RunOwner::Cell`. VM structures use `RunOwner::Vm { owner: tid }`. | `memory/frame_run.rs:45-56`; §4.2 |
| C9 | **VMs are keyed by owner tid on both arches.** AArch64 allocates the VMID after the Stage-2 root, the guest carve and the guest map. x86 already reserves the SVM ASID before any vCPU frame. `create_vm` maps every registry error to `NotSupported`, not `OutOfMemory`. | `arch/aarch64/vm.rs:70-73,402-471`; `arch/x86_64/hypervisor/svm_registry.rs:163-164,304-320`; `task/syscall/vm.rs:64-65` |
| C10 | **There are three tag allocators, and VMX uses no tag.** `SEC_ENABLE_VPID` is defined but never set. One owner can hold all 255 VMIDs. | §5.2; `hal/arch/x86/src/x86_64/vmcs.rs:360` |
| C11 | **Every launch gets the same heap limit, and no per-cell limit source exists.** All three `TaskLaunchState::complete` callers pass the global default. The only per-path data the kernel keys on a kernel-resolved path is `catalog::Cell`. | `loader/governed_spawn.rs:218,316`; `loader/domain_admission.rs:539`; `catalog.rs:28-35` |
| C12 | **Spec 02 §2 states a different, stale rule (PC range).** The ABI telemetry doc states the ambient rule ("kernel heap in its context"). | `docs/specs/02-memory.md:21-24`; `libs/abi/src/telemetry.rs:20-22` |
| C13 | **`QUOTA_LIMITS` is a `Spinlock<BTreeMap>` locked on every heap allocation.** The pin in `register` exists only because the map allocates while that lock is held. | `memory/cell_quota.rs:49-50,190-204,309-313` |
| C14 | **`rt_heap` is a dead allocator front.** It is initialised and never called. | `memory/rt_heap.rs:63-80`; `main.rs:176`; `docs/specs/02-memory.md:26-30` |
| C15 | **Plan line references are stale.** `heap.rs:171` is now `:169`. `heap.rs:232` is the `HeapIrqMask` comment at `:228-232`. `cell_quota.rs:200-211` is now `:190-204`. | — |

## 1. Ambient charging today

### 1.1 Mechanism

| Element | Behaviour | Evidence |
|---|---|---|
| Owner choice | `alloc_impl` charges `scheduler::current_cell_id()`, which is the per-hart `ViHartLocal.current_cell_id` (0 = kernel). | `memory/heap.rs:169-170`; `sched/scheduler.rs:27-29`; `sched/hart_local.rs:280-282`; `arch/x86_64/hart_local.rs:47-59` |
| Who writes the identity | Dispatch writes `(cell, generation)` of the incoming task. The task→boot switch, boot completion and hart init write `(0, 0)`. | `sched/scheduler.rs:2826-2829,2949`; `sched/scheduler.rs:42`; `task.rs:1149`; `sched/hart_local.rs:227-228` |
| Owner word | A trailing `usize` after the payload holds the token returned by `charge_owner`: `(row generation << 40) | cell`. Kernel blocks carry `UNCHARGED_OWNER = 0`. | `memory/heap.rs:143-157,214-218`; `memory/cell_quota.rs:76-77,330` |
| Refund | `dealloc` reads the owner word and calls `refund_owner`. The refund is a no-op if the row has been re-registered since. | `memory/heap.rs:305-312`; `memory/cell_quota.rs:351-361` |
| `realloc` | Non-p01 builds use the default `GlobalAlloc::realloc`: allocate under the current cell, copy, then free under the old tag. Growing a block moves its whole charge to the current cell. | `memory/heap.rs:267-326` (only the p01 build overrides `realloc`, `:287-304`) |
| `charge_owner` | Cell 0 is uncharged. It locks `QUOTA_LIMITS` for a read. An unregistered cell gets limit `usize::MAX` but is still counted. A cell ≥ `MAX_CELLS` is uncharged. A checked add is capped by the limit and by `BYTES_MASK`. | `memory/cell_quota.rs:304-331` |
| `charge` / `refund` | Thin raw-id wrappers. `refund` is generation-blind and saturating. | `memory/cell_quota.rs:290-292,337-346` |
| `register` | Bumps the row generation, zeroes the bytes, pins identity to 0, then inserts the limit into the map. | `memory/cell_quota.rs:172-205` |
| `deregister` | Zeroes the bytes (keeps the generation), zeroes `DMA_IN_USE`, removes the limit. | `memory/cell_quota.rs:208-225` |
| `QuotaReservation` | `reserve(id)` refuses id 0 and ids ≥ `MAX_CELLS` and refuses a duplicate. `reserve_next` takes the lowest free id. An uncommitted drop deregisters. | `memory/cell_quota.rs:228-284` |
| `IN_USE` | `[AtomicUsize; MAX_CELLS]`. Bits `40..64` hold the 24-bit row generation; bits `0..40` hold bytes (at most 1 TiB). | `memory/cell_quota.rs:66-74` |
| `DMA_IN_USE` | `[AtomicUsize; MAX_CELLS]` of plain bytes with no generation. Its limit is the **heap** limit, or the default when no limit row exists. | `memory/cell_quota.rs:390,419-442` |
| `MAX_CELLS` | 64 by default, 4096 under feature `cell-scale-experiment` (used only by `scripts/qemu-cell-scale.sh`). It also bounds the CellId space: `cspace`, the scheduler owner directory, launch and telemetry depend on it, and `cpu_accounting` statically asserts it. | `memory/cell_quota.rs:22-25`; `kernel/Cargo.toml:134`; `scripts/qemu-cell-scale.sh:78-81`; `cell/cspace.rs:777`; `sched/scheduler.rs:636,699`; `task/launch.rs:137`; `sched/cpu_accounting.rs:19`; `task/telemetry.rs:59` |
| `QUOTA_LIMITS` | `Spinlock<BTreeMap<usize, usize>>`, rank `QUOTA_LIMITS`. | `memory/cell_quota.rs:49-50`; `sync/rank.rs:124` |
| `DEFAULT_QUOTA_BYTES` | 16 MiB. The runtime default can only be lowered, between 1 MiB and 16 MiB, by `system_config` (`memory.default_cell_heap_mib`). | `memory/cell_quota.rs:32-45`; `system_config.rs:31-33` |
| IRQ masking | Every allocator entry masks local interrupts (`HeapIrqMask`). Its comment says that "timer/IPI ISRs and the #PF funnel allocate and free" (the review's `heap.rs:232`). | `memory/heap.rs:228-260,268-270,305-306` |
| OOM | Quota or heap failure returns null. An infallible caller then reaches `alloc_error_handler`, which logs and idles forever. | `memory/heap.rs:170-179,204-212,403-453` |

### 1.2 Other allocator fronts

| Front | Charging | Evidence |
|---|---|---|
| `rt_heap` (TLSF, 256 KiB static) | None. No caller outside its own file; only `init` runs. | `memory/rt_heap.rs:29-80`; `main.rs:176` |
| `heap::try_box` | Global allocator, so ambient. | `memory/heap.rs:380-393` |
| `virtio_hal::dma_alloc` | Global allocator, so ambient. Panics on null. **[I]** Callers are kernel virtio paths running in kernel context. | `dev/virtio_hal.rs:15-41` |
| x86 VT-d IR table | `alloc_zeroed` through the global allocator, so ambient. Panics on null. Boot path **[I]**. | `arch/x86_64/dev/iommu_x86/ir/table.rs:36-46` |
| Frame allocator | No quota at all. The only bound is the quarantine cap. | `memory/frame.rs:812-833`; `memory/frame_run.rs:1629-1635` |

### 1.3 Readers of the ambient identity outside the allocator

The identity is also the trap-attribution signal, so it cannot simply be deleted.

| Reader | Use | Evidence |
|---|---|---|
| `vi_current_cell_id` | HAL lower-EL trap handlers: `cell_id != 0` decides that a trap is a recoverable Cell fault. | `task.rs:940-943`; `hal/arch/arm/src/aarch64/trap.rs:120-121,172`; `hal/arch/riscv/src/rv32/trap.rs:76` |
| Fault and exit records | `DeferredFault`/`DeferredExit` capture `(cell, generation)`. | `task.rs:1244-1258`; `task/syscall/spawn.rs:597-604` |
| x86 `fault_kill_cell` | Cell 0 means a kernel bug, so it panics. | `memory/paging.rs:1581-1588` |
| `spawn.rs` quota diagnostics | Logging only. | `task/syscall/spawn.rs:986-987` |
| p01 provenance | Trace only. | `memory/heap/p01_owner.rs:423` |

## 2. Identity swap and pin-to-0 sites

Grep patterns used, across `kernel/`, `hal/`, `libs/` and `cells/`:
- `set_current_cell_id|set_current_cell_context|KernelCellScope|kernel_charge|as_kernel|with_kernel_cell|CellIdGuard|PinKernel`
- `KernelAllocationContext|PipeAllocationContext::enter`
- `struct .*Context|struct .*Scope|struct .*Guard` in the IPC and task files
- `current_cell_id\(\)` for the readers

Nothing in `hal/`, `libs/` or `cells/` swaps the identity. `p01_owner::scope` is provenance only: its comment says "Quota routing remains unchanged" (`loader/governed_spawn.rs:204-207`).

### 2.1 Production scopes (31)

"Drop-only" means the scope only frees memory. It does not affect accounting today (C2).

| # | Site | What it allocates | Swap | Why it pins | Should be charged to | Conf. |
|---|---|---|---|---|---|---|
| P1 | `ipc/ipc_wire.rs:49-57` `IpcWireMessage::try_new` | Payload `Box<[u8]>` of an in-flight message (≤ `MAX_IPC_WIRE_PAYLOAD`) | → 0 | The wire outlives the sender and is freed by the receiver | **Sender incarnation** for task senders (the header carries `sender_cell_id`/`sender_generation`, `ipc/ipc_wire.rs:25-27`; p01 already labels it `SENDER_WIRE`, `task.rs:2219-2229`). **Kernel** for kernel senders (UART, SCI, GPIO). | medium |
| P2 | `ipc/ipc_wire.rs:74-82` `try_from_user` | Same, copied from the caller's view | → 0 | Same | Sender (the caller) | medium |
| P3 | `ipc/ipc_wire.rs:156-164` `try_clone` | A duplicate of the payload | → 0 | Same | The sender named in the header | medium |
| P4 | `ipc/ipc_wire.rs:174-178` `Drop` | — (drop-only) | → 0 | Refund side | Delete the swap | high |
| P5 | `ipc/pending_mailbox.rs:33-41` `PendingMsgData::try_copy` | Heap payload > 32 B | → `allocation_cell` | Receiver-owned storage | **Receiver incarnation**. Callers pass a raw id: `task.rs:236` (`target.cell_id`), `cell/hotswap.rs:372`. `sched/scheduler.rs:2344,2368` pass 0 but stay inline. The generation must come from `target.cell_generation`. | high |
| P6 | `ipc/pending_mailbox.rs:93-96` `Drop for PendingMsgData` | — (drop-only) | → `allocation_cell` | Refund side | Delete | high |
| P7 | `ipc/pending_mailbox.rs:230-233` `PendingMailbox::try_push` | Mailbox `Vec` growth | → 0 | "without charging mailbox-container growth to an arbitrary cell" | **Receiver** (depth-bounded; p01 labels it `MAILBOX` with the target as owner, `task.rs:240-244`) or kernel | medium |
| P8 | `ipc/pending_mailbox.rs:244-247` `Drop for PendingMailbox` | — (drop-only) | → 0 | Refund side | Delete | high |
| P9 | `ipc/pipe.rs:114-131` `PipeAllocationContext`, used at `:135` (`release_pipe`, drop-only) and `:185` (`create`) | Ring buffer, 4 KiB…max capacity | → 0 | The capacity is charged explicitly to `owner_cell` (`:181`) so the allocator does not charge it a second time | **Creator incarnation**, as one generation-checked reservation stored in `Pipe`. Today `refund(owner_cell)` at `:137,192` is generation-blind (C5). | high |
| P10 | `memory/cell_quota.rs:190-204` `register` | `BTreeMap` node | → 0 | Self-deadlock: `charge` would re-lock `QUOTA_LIMITS` | **Kernel**. Better: a fixed limits array removes the allocation entirely (C13). | high |
| P11 | `task/async_ipc.rs:142-145` `Operations::push` | Per-task op vector growth (≤ `MAX_OPERATIONS` = 64, `:15`) | → 0 | Documented as "kernel-charged storage" (`:107-110`) | Kernel (bounded) | medium |
| P12 | `task/async_ipc.rs:263-266` `Drop for Operations` | — (drop-only) | → 0 | Refund side | Delete | high |
| P13 | `task/async_ipc.rs:301-307` `AsyncIndex::reserve` | Scheduler `owners`/`holders` index growth | → 0 | Scheduler-owned index | Kernel | high |
| P14 | `task/async_ipc.rs:339-342` `note_holder` | `holders` growth; **infallible** `reserve(1)` | → 0 | "never to the receiving Cell" | Kernel; also make it fallible | high |
| P15 | `task/completion.rs:467-470` `queue_for` → `CompletionQueue::new` | Per-cell completion queue `Arc` | → 0 | Shared by every thread in the cell | **Owning cell incarnation** (p01 labels it `COMPLETION_SHARED`, `:462-466`) or kernel | low |
| P16 | `task/syscall/spawn.rs:604` (Exit) | One-way switch to 0 after `defer_exit` | → 0 | "Do not … log, audit … while the exiting Cell is still charged" (`:593-596`) | Kernel for everything after the record. The swap also changes trap attribution, so keep it as an attribution handoff. | high |
| P17 | `task.rs:1258` (user-fault funnel) | One-way switch to 0 after `defer_fault` | → 0 | Same, for faults (`task.rs:1264-1266`) | Kernel; keep as an attribution handoff | high |
| P18 | `sched/scheduler.rs:79-99` `KernelAllocationContext` (guard definition) | — | → 0 | "Lexically pins heap accounting to the kernel for scheduler-owned containers" | — | — |
| P19 | `sched/scheduler.rs:383` `subscribe_death` | `DEATH_SUBSCRIBERS` node and vector | → 0 | Kernel-funded, bounded to live pairs (`:400-402`) | Kernel (selftest at `:459-505` pins this) | high |
| P20 | `sched/scheduler.rs:430` `release_pending_deaths` | — (drop-only) | → 0 | Refund side | Delete | high |
| P21 | `sched/scheduler.rs:406` `queue_pending_death` | `task.pending_deaths` growth (infallible `push`) | → 0 | Scheduler-owned | Kernel | high |
| P22 | `sched/scheduler.rs:435` `drain_death_subscribers` | Mostly a drop; `watchers` hand-off | → 0 | Same | Kernel | high |
| P23 | `sched/scheduler.rs:629` `insert_cell_owner_slot` | `cell_owners` map node | → 0 | "scheduler bookkeeping, not an object of the Cell whose launch … grow[s] it" | Kernel | high |
| P24 | `task/launch.rs:170` `publish_prepared` | Unpublished task record, inherited dirs, argv move, measurement | → 0 | Per-principal accounting is the explicit `PublicationClaim` | Kernel bookkeeping; the creator is charged explicitly (`task/launch.rs:315-350`) | high |
| P25 | `ipc/cell_session.rs:3018` `Drop for KernelBytes` | — (drop-only) | → 0 | Refund side | Delete | high |
| P26 | `task/launch.rs:530` `SessionPublication::reserve` | As P24 | → 0 | Same | Kernel + explicit creator claim (`:563-567`) | high |
| P27 | `ipc/cell_session.rs:2740` `with_state` | Session tables | → 0 | "Bookkeeping heap is kernel memory; per-principal charges are explicit" (`:2737-2738`) | Kernel + explicit `Charge` per record (`:53-77`) | high |
| P28 | `ipc/cell_session.rs:2937` `dispatch` (pre) | Sweep and resolve under `SCHEDULER` | → 0 | Same | Kernel | high |
| P29 | `ipc/cell_session.rs:2998` `dispatch` (finish) | `finish_request` ledger | → 0 | Same | Kernel | high |
| P30 | `ipc/cell_session.rs:3081` `prepare_child` | Context staging bytes | → 0 | Charged explicitly to the preparer (`:3078`) | Kernel allocation + preparer reservation | high |
| P31 | `ipc/cell_session.rs:3111` `prepare_child` (commit) | Grant preflight and scheduler records | → 0 | Same | Kernel | high |

The drop-only rows are P4, P6, P8, P12, P20, P25, and the `:135` user of P9. Rows P1-P3 and P7 also run from IRQ context through `ipc_post_nonblock` (§3).

### 2.2 Dispatch-identity writers (not charging pins)

| Site | Writes | Evidence |
|---|---|---|
| Dispatch | `(next.cell_id, next.cell_generation)` | `sched/scheduler.rs:2826-2829` |
| Task→boot (no switch) | `(0, 0)` | `sched/scheduler.rs:2949` |
| Single-hart RV32 task→boot | `(0, 0)` | `sched/scheduler.rs:37-43` |
| Boot completion after switch | `(0, 0)` | `task.rs:1147-1150` |
| Hart init | `0` | `sched/hart_local.rs:227-228` |

### 2.3 Test and fixture swaps (rewrite with the model)

| Site | Gate | Relies on ambient charging for |
|---|---|---|
| `memory/heap.rs:491-531` | host `#[test]` | Charging owner A vs B through the dispatch id |
| `sched/scheduler.rs:469-499` | `test-hooks` (`:459`) | Death subscriptions stay off the owner and exit cells |
| `sched/scheduler.rs:2993-3047,3059-3122` | host `#[test]` (`:2986`) | Context save and restore only |
| `arch/riscv64/task/retirement_selftest.rs:841-868` | `test-hooks`, RV64 | `SMP-ROOT-EXIT-QUOTA` saturates the root quota and expects a probe allocation to be **refused** under the dispatch identity (`:842-855`) |
| `loader/atomic_publication_tests/success.rs:183-191` | test | Regression for the `register` self-deadlock |
| `ipc/cell_session.rs:3419,3680,3790,3840,3932,3998` | `test-hooks` (`:3395`) | Fixture staging as kernel |

### 2.4 Explicit charge and refund callers

| Site | Account | Generation-safe? | Evidence |
|---|---|---|---|
| Thread stacks: charge, map-failure refund, exit refund | heap | No (raw id) | `sched/scheduler.rs:1273,1292,1783-1786`; field `task/tcb.rs:348` |
| `PublicationClaim` (creator: measurement entry and ready slot) | heap | No | `task/launch.rs:315-350,563-567` |
| `cell_session::Charge` | heap | No; manual `forfeit` instead (`:66-71`) | `ipc/cell_session.rs:53-77` |
| Pipe ring | heap | No | `ipc/pipe.rs:137,181,192` |
| SHM frame | heap | No (refund at retirement, before `deregister`) | `task/syscall/grant.rs:127,1914,1934,1937` |
| DMA grant reservation | DMA | No | `memory/cell_quota.rs:395-442`; `task/syscall/grant.rs:1234-1235` |
| DMA export | DMA | No | `task/syscall/dma_export.rs:1013-1025` |
| `try_reserve_dma(0, 0)` used as a barrier | — | n/a | `task/syscall/grant.rs:23,2151` |

## 3. Allocation in interrupt and trap context

"Charged today" is the hart's identity at the trap: the interrupted task's cell (`sched/scheduler.rs:2826-2829`), or 0 on an idle or boot hart (`task.rs:1149`), or 0 after an exit or fault handoff (P16, P17).

| # | Path (entry → allocation) | Allocates | Fallible? | Charged today | Evidence |
|---|---|---|---|---|---|
| I1 | `vi_timer_tick` → `frame_run::reap` | `retry` and `kept` vectors; `Pending.entries` growth on re-push | fallible (`try_reserve`) | Interrupted cell (hart 0 only) | `task.rs:1056-1058`; `memory/frame_run.rs:1072,1112,1147,1195` |
| I2 | `vi_timer_tick` → `yield_cpu` → `drain_before_selection` → housekeeping | `reaped` vector, `waiter_wakes` `extend`, take-vectors, `IOMMU_CLEANUP_RETRIES.push_back`, retirement verdict, `release_shm_for`, `state_stash::release_retired` | `waiter_wakes.extend` and `push_back` are **infallible** | Interrupted cell | `task.rs:1105`; `task.rs:1350,1364,1521-1664`; `task.rs:1529,1536`; `task.rs:1867-1884` |
| I3 | `yield_cpu` → `pick_next` → global sweep | `waking_tasks` `VecDeque`, `vfs_context_drops`, `async_ipc::expire`, `cell_session::sweep` (P27 pins to 0) | `push_back`/`push` are infallible | Interrupted cell. After the dispatch write in the same pass, the **incoming** cell. | `sched/scheduler.rs:2398-2420,2533,2554`; `:2826-2829` |
| I4 | `vi_timer_tick` → `CONSOLE.poll()` → `relay_*_to_input` → `ipc_post_nonblock` | Wire payload (P1), mailbox growth (P7) | fallible | **Kernel** (pinned) | `task.rs:1088,1093`; `dev/console_drv.rs:249-268`; `task.rs:2218,2249`; `task.rs:283` |
| I5 | `vi_timer_tick` → `acpi::sci::relay_pending` (x86) | As I4 | fallible | Kernel (pinned) | `task.rs:1090`; `arch/x86_64/acpi/sci.rs:197-209` |
| I6 | GIC IRQ → `vi_gpio_notify_irq` (Pi) → `ipc_post_nonblock` | As I4 | fallible | Kernel (pinned) | `hal/arch/arm/src/aarch64/trap.rs:262`; `hal/arch/arm/src/aarch64/backend/bcm2837/trap.rs:205`; `dev/gpio_irq.rs:34-43` |
| I7 | `vi_ipi_service` → `yield_cpu` (x86, on preemption or retirement) | As I2 and I3 | as I2 and I3 | Interrupted cell | `task.rs:976-992` |
| I8 | UART RX IRQ | None: the ring is preallocated at 4096 B and bounded | — | — | `dev/uart.rs:333-345,400-452` |
| I9 | VirtIO, PLIC, MSI, SCI ISRs proper | None: atomics and MMIO ack only | — | — | `dev/virtio_common.rs:155-188`; `dev/irq_wait.rs:118-139`; `arch/x86_64/acpi/sci.rs:180-195`; `hal/arch/x86/src/x86_64/idt/dispatch.rs:81-85` |
| I10 | x86 `#PF` funnel (synchronous, user fault) | Demand **frame** (uncapped, uncharged, dormant: the VMA list is empty); `fault_kill_cell` → deferred fault | — | Frame: none | `memory/paging.rs:1459-1555,1581-1588` |

Consequence (C4). An interrupted cell is billed for kernel housekeeping. It is refunded later through the owner word, so this is a mis-charge, not a leak. A cell at its limit makes housekeeping allocations fail: fallible sites degrade and infallible sites idle the hart. The exit and fault paths already guard against this for the **victim** (P16, P17, and the `SMP-ROOT-EXIT-QUOTA` selftest), but nothing guards against an **unrelated** cell that the timer interrupts. **[I]**: no lane drives a saturated cell under a timer housekeeping pass.

## 4. Frame allocations and their charge

### 4.1 Inventory

| # | Frames | Allocation site | Owner known at that point | Charged? | Release site |
|---|---|---|---|---|---|
| F1 | Root task stacks (kernel and user) | `task/elf_prepare.rs:246,254` → `task/stack.rs:196,255` | Not yet: `prepare_elf_task(.., CellId(0), ..)` runs before the CellId is reserved (`loader/governed_spawn.rs:212`; `task/launch.rs:160-174`) | **No** | `Stack::drop` `task/stack.rs:583`; leak path `task/tcb.rs:813-841` |
| F2 | Thread stacks | `sched/scheduler.rs:1261-1262` | `cell_id` (raw) | heap counter, generation-blind | Charge refunded at exit `sched/scheduler.rs:1783-1786`; frames freed at reap |
| F3 | Cell segments | `loader/elf.rs:293`; run `memory/cell_segments.rs:224-228` (`RunOwner::Kernel`) | Not yet (as F1) | **No** | `Drop for CellSegments` `memory/cell_segments.rs:248` |
| F4 | Grant pages (SAS) | `task/syscall/grant.rs:1317` via `alloc_owned_grant` `:2182-2207`; run `:428` (`RunOwner::Kernel`) | `caller_id`; the incarnation is bound after allocation (`:2274`) | **No**. Bounded only by `MAX_GRANT_PAGES` = 4096 per call (`:165`) | `free_grant_pages` `:325`; `grant_free` `:2449`; `reap_grants_for_task` `:564` |
| F5 | Domain-owner grant backing | `task/syscall/mod.rs:279` (`RunOwner::Kernel` `:290`) | `caller_id` | **No** | as F4, domain path |
| F6 | SHM frame | `task/syscall/grant.rs:1918`; run `:1157-1167` (`RunOwner::Cell{cell,generation}`) | Incarnation (`:1903-1907`) | heap counter (`:1914`), generation-blind refund | `release_shm_for` `:118-127` at retirement (`task.rs:1626-1629`) |
| F7 | Kernel-root page tables (SAS) | `memory/paging.rs:399` (`map_page`), `:655` (`map_page_unflushed`), `:1132` (`remap_range_user`), `:1197` (framebuffer); boot `:132,187,221,497,557` | The cell whose stack, grant or SHM forced the table; tables are shared | **No** (shared) | `prune_empty_tables` / `ReclaimedPageTables` `memory/paging.rs:1007-1043` |
| F8 | Domain page tables (private root and intermediate tables) | `memory/address_space.rs:129-150` (`allocate_owned_frame`), from `build_inner` `:336-414` and the lazy `map_private_page` `:667`, `map_existing_task_stacks` `:752`, `map_grant_page` `:1065` | `DomainId` only (`:179-186`). The caller `admit_and_bind_domain` holds the `Task`, which has `(cell_id, cell_generation)` (`task/launch.rs:415-440`) | **No** | `Drop for AddressSpace` `:1262`; deferred `memory/deferred_release.rs:42,93` |
| F9 | Private-root ASID | `memory/address_space.rs:336` | `DomainId` | n/a (tag) | `AsidLease::release` `:1385-1403` |
| F10 | Stage-2 root (2 frames, 8 KiB-aligned) | `memory/stage2.rs:190` (`RunOwner::Kernel`) | Caller tid via `create_vm(owner, ..)` | **No** | `Drop for Stage2Table` `memory/stage2.rs:445-457` after `reap_vms_for_task` `arch/aarch64/vm.rs:892-926` |
| F11 | Stage-2 subtables (lazy) | `memory/stage2.rs:251-264` | as F10 | **No** | as F10 |
| F12 | AArch64 guest RAM (one contiguous run) | `memory/stage2.rs:233-247` → `memory/frame.rs:1753-1763` | as F10 | **No** | as F10 |
| F13 | EPT/NPT root | `arch/x86_64/memory/ept.rs:144-151` | tid via `create_vm` | **No** | `Drop for NestedPageTable` `arch/x86_64/memory/ept.rs:467-497` |
| F14 | EPT/NPT subtables (lazy) | `arch/x86_64/memory/ept.rs:254-267` | as F13 | **No** | as F13 |
| F15 | x86 guest RAM | `arch/x86_64/memory/ept.rs:201-215` → `memory/frame.rs:1753` | as F13 | **No** | as F13 |
| F16 | xAPIC page | `arch/x86_64/hypervisor/svm_registry.rs:218` (`vm_structure(owner, 0, 1)`: vm_id 0 is a placeholder) | tid | **No** | `SvmVm` drop after `reap_vms_for_task` `:599-610` |
| F17 | VMX VMCS (1 page per vCPU) | `arch/x86_64/hypervisor/svm_registry.rs:270` | tid, vm_id | **No** | `Drop for Vmcs` `:51-66` |
| F18 | SVM VMCB, host-save, MSRPM (2), IOPM (3): 7 pages per vCPU | `arch/x86_64/hypervisor/svm_registry.rs:320-323` → `vm_structure` `:70-80` (`RunOwner::Vm{owner: tid}`) | tid, vm_id | **No** | `SvmVm` drop |
| F19 | AArch64 vCPU state | Heap only (`Vm.vcpus`, `vcpu_irqs`, `vcpu_vgic` vectors, `arch/aarch64/vm.rs:48-68`) | — | heap, ambient | registry drop |
| F20 | `frame::allocate_guest_ram` | `memory/frame.rs:1753-1763`: whole-bitmap search under the lock, refuses past the quarantine cap | none (raw) | **No** | the caller's `PrivateRun` |
| F21 | Hypervisor heap side | `Stage2Table.tables` / `NestedPageTable.tables` `Vec<PrivateRun>` growth (`memory/stage2.rs:165,258`; `arch/x86_64/memory/ept.rs:128,261`); registry `BTreeMap` nodes (`arch/aarch64/vm.rs:491`; `arch/x86_64/hypervisor/svm_registry.rs:249`); staging buffer of up to 256 KiB per copy (`task/syscall/vm.rs:16-27`) | ambient = the hypervisor cell during the syscall | heap, ambient | drop |
| F22 | Boot-lifetime frames | `main.rs:164` (heap); `arch/x86_64/boot.rs:180,632`; `arch/x86_64/boot/cpu_features.rs:70` | kernel | n/a | never |

### 4.2 `FrameRun` owner field today

`RunOwner::Kernel` is used by every run except two: SHM (`task/syscall/grant.rs:1160`) and VM structures (`arch/x86_64/hypervisor/svm_registry.rs:73`). The `Kernel` sites are `arch/x86_64/memory/ept.rs:150,209,264`, `loader/elf.rs:325,422`, `memory/cell_segments.rs:227`, `memory/deferred_release.rs:93`, `memory/paging.rs:1043`, `memory/pin.rs:618`, `memory/stage2.rs:192,241,261`, `task/stack.rs:271`, `task/syscall/grant.rs:428,1419`, and `task/syscall/mod.rs:290`. The field is diagnostic only: `FrameRun::free` never reads it (`memory/frame_run.rs:454-466`).

## 5. VM keying and tag allocators

### 5.1 VM registries

| Registry | Key | VM id rule | Reap | Evidence |
|---|---|---|---|---|
| AArch64 `VM_REGISTRY` (rank 45) | `(owner tid, vm_id)` | `count(owner) + 1` | `reap_vms_for_task(tid)`: collect under the lock, `disable_stage2` + `s2_tlb_flush_all`, release VMIDs, drop | `arch/aarch64/vm.rs:70-73,490,892-926` |
| x86 `VM_REGISTRY` (rank 46, `SVM_VM_REGISTRY`) | `(owner tid, vm_id)` | `count(owner) + 1` | `reap_vms_for_task(tid)`: collect, drop | `arch/x86_64/hypervisor/svm_registry.rs:163-164,243,599-610` |
| Common front | tid passed through | — | Called once per member tid from `reap_retired_task_resources` | `hypervisor/registry.rs:19-21,107-109`; `task.rs:1903-1928` |
| Syscall | `caller_id` (tid) is the owner; gated on `t.hypervisor` | — | — | `task/syscall/vm.rs:9-11,51-66` |

`create_vm` maps any registry error to `SyscallError::NotSupported` (`task/syscall/vm.rs:64-65`).

Creation order in each registry. Refusal-before-effect matters for fixture (b).
- **AArch64** `build_vm`: EL2 check (`arch/aarch64/vm.rs:390-400`) → Stage-2 root (`:402`) → guest carve (`:406-427`) → guest map (`:431`) → GICV passthrough map (`:444`) → VMID (`:464-471`) → registry (`:472-506`). VMID exhaustion therefore refuses after the carve and the map. The frames return on drop.
- **x86** `create_vm`: EPT/INVEPT check (`arch/x86_64/hypervisor/svm_registry.rs:188-195`) → NPT/EPT root (`:196`) → carve (`:200`) → map (`:208`) → xAPIC frame (`:218`) → registry (`:239-256`). `create_vcpu` reserves the SVM ASID **before** any frame (`:306-319`), which is the precedent to follow.

### 5.2 Tag allocators

| Allocator | Width / count | Allocation | Reuse rule | Invalidation before reuse | Per-owner limit | Evidence |
|---|---|---|---|---|---|---|
| Private-root `LIVE_ASIDS` / `AsidLease` | 256 slots (`MAX_LIVE_ASIDS`); value = slot + 1; refused if ≥ `1 << asid_width()` (x86 12 bits, others 16) | Lowest free slot, under the `LIVE_ASIDS` lock (rank 99) | Reused after release | `confirm_tag_invalidation(value)`. On failure the slot stays reserved and is handed to `deferred_release` | None (keyed by `DomainId`) | `memory/address_space.rs:22-28,1319-1336,1358-1424,1432-1460` |
| x86 SVM `NEXT_ASID` | `1..=max_asid` (CPUID `8000_000A.EBX` − 1) | Monotonic `fetch_update`, before any vCPU frame | **Never** reused in a boot | n/a. `TLB_CONTROL` = flush-this-ASID on every VMRUN | None | `arch/x86_64/hypervisor/svm_registry.rs:132-143,304-319`; `hal/arch/x86/src/x86_64/svm.rs:38-49`; `hal/arch/x86/src/x86_64/vmcb.rs:112,174` |
| x86 VMX VPID | **None**: `SEC_ENABLE_VPID` is defined and never set. **[I]** per the Intel SDM, VPID 0 flushes on every VM transition. | — | — | — | — | `hal/arch/x86/src/x86_64/vmcs.rs:360` (no other use) |
| AArch64 `VmidPool` (HAL) | 8-bit (`VMID_COUNT` 256, VMID 0 = host), so 255 guest VMIDs | Lowest free bit, CAS | Reused after release | Holder runs `TLBI ALLE1IS` (`s2_tlb_flush_all`) before `release`; `enable_stage2` invalidates again on bind | **None. One owner can hold all 255** (Phase 16 residual; the fixture does exactly that, `arch/aarch64/vm.rs:1043-1065`) | `hal/arch/arm/src/aarch64/stage2_regs.rs:44-110`; `arch/aarch64/vm.rs:75-78,464-471,917-925` |

`scripts/mutable-globals.toml` has entries for `NEXT_ASID` (`:1201`), the x86 `VM_REGISTRY` (`:1212`), `LIVE_ASIDS` (`:2176`), `QUOTA_LIMITS` (`:2132`), `IN_USE` (`:2143`) and `DMA_IN_USE` (`:2154`). Each one changes with the design below.

## 6. Guest-RAM sizes and where a limit is set

### 6.1 What VM owners request

| Requester | Profile / feature | Guest RAM | Evidence |
|---|---|---|---|
| `/bin/hypervisor` x86 | Console / Fixture (default) | 128 MiB | `cells/services/hypervisor/src/boot_x86_profile.rs:5-6,61-66` |
| | `alpine-wide-guest` | 256 MiB | `:7-8` |
| | `ubuntu-wide-guest` | 512 MiB | `:9-10` |
| | **Chrome** (app mode) | **1536 MiB** (the guest reports 1475 MiB usable) | `:11-13,61-66`; used at `cells/services/hypervisor/src/boot_x86.rs:68,96` |
| `/bin/hypervisor` AArch64 | `alpine` (default) | 128 MiB | `cells/services/hypervisor/src/boot_arm_profile.rs:29-30` |
| | `alpine-wide-guest` | 256 MiB | `:31-32` |
| | `alpine-gui-guest` | 512 MiB | `:33-34`; used at `cells/services/hypervisor/src/boot_arm.rs:96` |
| `/bin/silo` | — | 16 pages (64 KiB) | `cells/guests/silo-guest/src/layout.rs:5-6`; `cells/services/silo/src/main.rs:64` |
| `hypervisor-test` cell | — | 512 pages (2 MiB) | `cells/tests/hypervisor-test/src/main.rs:46,56` |
| Kernel smoke `x86_smoke`, `x86_mmio_smoke` | synthetic owners `0xF00D`/`0xF00E` | 256 pages each | `arch/x86_64/hypervisor/svm_registry.rs:626-631,688-701` |
| Kernel AArch64 two-VM / VMID fixture | synthetic owner `usize::MAX - 0x2e2` | 512 pages; 255 × 1 page | `arch/aarch64/vm.rs:960-962,1010,1048,1070` |

None of the hypervisor cells spawn threads (`rg spawn_thread` finds nothing in their sources), and each creates one VM.

Budget for the largest profile (Chrome, x86, 4 KiB nested leaves: `map` calls `map_single` per page, `arch/x86_64/memory/ept.rs:283-325,440`):
- Guest RAM: 1536 MiB = 393,216 pages.
- Nested tables: 768 PT + 2 PD + 1 PDPT + 1 PML4, plus 2 for the xAPIC window at `0xFEE0_0000`. That is about 774 frames, about 3.0 MiB.
- xAPIC page: 4 KiB.
- Per SVM vCPU: 7 pages (28 KiB). Per VMX vCPU: 1 page.

Total: about 1539.1 MiB. A budget of **1544 MiB** (1536 + 8) covers it with headroom for one more vCPU and AArch64's Stage-2 shape. **[I]**: the table count follows from the walk code and was not measured.

### 6.2 How a cell's limit is set at launch

| Step | Today | Evidence |
|---|---|---|
| Source of the limit | Always `cell_quota::default_quota_bytes()` (or `DEFAULT_QUOTA_BYTES` in the admission probe) | `loader/governed_spawn.rs:213-219,311-316`; `loader/domain_admission.rs:534-539` |
| Carried in | `TaskLaunchState.quota_limit` | `task/launch.rs:39,67,84` |
| Registered | `QuotaReservation::reserve(_next)(state.quota_limit)` in `publish_prepared` and `SessionPublication::reserve` | `task/launch.rs:158-163,533` |
| Deregistered | Root retirement, after the owner slot is released | `task.rs:1637-1664` |
| Per-path data available | `catalog::Cell { path, request, boot, user_target, trusted_core }`, found by **kernel-resolved** path; `catalog::find` | `catalog.rs:28-35,639-664` |
| Hypervisor authority | `hypervisor: true` in the boot rows of `/bin/silo`, `/bin/hypervisor` and `/bin/init` | `catalog.rs:208,361-385` |
| Path spoofing | `SpawnFromMem` reduces the name to a `/mem/` label that matches no path row | `loader/mem_spawn_gate.rs:1-27` |
| Catalog export | Only cap fields are exported to `scripts/catalog.generated.json` (read by `sign-policy.py` and `check-catalog.py`) | `catalog_export.rs:1-30` |

So a guest budget can be set at launch without a manifest or ABI change. Add a kernel-only `guest_memory` field to `catalog::Cell`, look it up for the kernel-resolved path where `TaskLaunchState` is built, and leave it unexported (default 0).

## 7. Does Spec 02 §2 state the ambient rule?

No, it states a different rule. `docs/specs/02-memory.md:21-24` says that the allocator finds `CallerID` "through the Program Counter range" and that OOM returns `Err(OutOfMemory)` instead of panicking. Neither matches the code:
- The owner is the per-hart dispatch identity (`memory/heap.rs:169`).
- An infallible allocation that fails idles the hart (`memory/heap.rs:450-452`).

§3 says only "Quota nằm ở `cell_quota`" (`:44`). The ambient rule is written down in the ABI telemetry doc: `quota_charged_bytes` covers "kernel heap in its context" (`libs/abi/src/telemetry.rs:20-22`).

## 8. Design proposal

### 8.1 One charge record

All of the following go in `memory/cell_quota.rs`.

```rust
/// A quota owner: one Cell incarnation, or the kernel (cell 0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Charge { cell: CellId, generation: u64 }
impl Charge {
    pub(crate) const KERNEL: Charge;
    pub(crate) fn of(task: &Task) -> Charge;            // (cell_id, cell_generation)
    pub(crate) fn dispatched() -> Charge;              // hart (cell, generation); syscall entry only
}
pub(crate) enum Account { Memory, Dma, Guest }
/// Bytes taken from one account of one incarnation; refunded once on drop,
/// to that incarnation only (a no-op once the row was re-registered).
pub(crate) struct Reservation { charge: Charge, account: Account, bytes: usize }
impl Reservation {
    pub(crate) fn take(charge: Charge, account: Account, bytes: usize) -> Result<Self, QuotaExceeded>;
    pub(crate) fn split(&mut self, bytes: usize) -> Option<Reservation>; // draw-down
    pub(crate) fn charge(&self) -> Charge;
}
```

- **One generation.** The quota row stores the task `cell_generation`, not its own registration count. `register(cell, generation, Limits { memory, guest })` replaces `register(cell, limit)`. `IN_USE`, `DMA_IN_USE` and the new `GUEST_IN_USE` all pack `generation & 0xFF_FFFF` in bits `40..64`. A refund applies only when the packed generation matches. This needs the generation to be minted before the quota is reserved. Today `Task::new` mints it (`task/tcb.rs:617`) after `QuotaReservation::reserve` (`task/launch.rs:158-174`, `:533-537`). Expose `next_cell_generation` and pass the value into `into_task`.
- **Lock-free limits.** `QUOTA_LIMITS: Spinlock<BTreeMap>` becomes `[Limits; MAX_CELLS]` of atomics. `charge` never locks, `register` never allocates, and pin P10 is deleted.
- **One reservation type replaces six ad-hoc ones:** `DmaQuotaReservation` (`memory/cell_quota.rs:395-413`), `cell_session::Charge` (its `forfeit` is no longer needed), `PublicationClaim` (`transfer` becomes `split`), the pipe charge, `Task.stack_quota_charge`, and the SHM charge. All of them become generation-safe (C5).
- **Kernel charge.** `Charge::KERNEL` short-circuits every account, as cell 0 does today (`memory/cell_quota.rs:305-307`). Kernel bookkeeping is unlimited and is bounded by its own structure limits.

### 8.2 Heap: an explicit scope instead of dispatch identity

- Add a field to `ViHartLocal`: `alloc_charge: (cell, generation)`, default `KERNEL`. Append it after the existing fields. **[I]** The asm offset assertions cover only the leading fields (`sched/hart_local.rs:128`). `alloc_impl` reads `alloc_charge` instead of `current_cell_id` (`memory/heap.rs:169`). The owner word keeps its format.
- Add an RAII scope, `heap::ChargeScope::enter(charge)`, and a closure form, `heap::alloc_for(charge, f)`. Each saves the slot and restores it on drop. Only three kinds of code open a scope:
  1. **Syscall entry.** `ViCell_syscall_dispatch` (`task/syscall/mod.rs:2011`, `:2103`) opens `ChargeScope::enter(Charge::dispatched())`. Syscall-path allocations keep today's charge (grant rows, registry nodes, staging buffers), now with a generation. This is the only place a cell charge is created from dispatch identity.
  2. **Owner-directed sites.** These are P1-P3 (sender), P5 and P7 (receiver, with `target.cell_generation`), P9 (creator, through a `Reservation`) and P15 (the owning cell, or kernel).
  3. **Kernel scopes inside a syscall.** `alloc_for(Charge::KERNEL, ..)` at P11, P13, P14, P19, P21-P24 and P26-P31. These replace `KernelAllocationContext`, which is deleted along with `PipeAllocationContext`.
- **Switch rule.** `yield_cpu` (`task.rs:1350`) saves `alloc_charge`, sets `KERNEL` before `drain_before_selection`, and restores the saved value when its activation resumes. A scope therefore never crosses a switch. Housekeeping, the sweep and selection always run as kernel (closes I2, I3, I7).
- **Interrupt rule.** Each Rust interrupt entry opens `ChargeScope::interrupt()` (sets `KERNEL`) as its first statement. The entries are `vi_timer_tick` (`task.rs:1002`), `vi_ipi_service` (`:976`), `vi_handle_riscv_external_irq` (`arch/riscv64/dev/irq_dispatch.rs:37`), `vi_gpio_notify_irq` and `vi_signal_usb_irq` (`dev/gpio_irq.rs:34,47`), `vi_handle_uart_irq` (`dev/uart.rs:400`), `vi_handle_virtio_irq` (`dev/virtio_common.rs:155`) and `vi_handle_sci` (`arch/x86_64/acpi/sci.rs:180`). This closes I1 and I4-I6, and it also covers a timer that lands inside a syscall scope. Under `test-hooks`, a per-hart counter records allocations made inside an interrupt scope, so the inventory in §3 can be checked on lanes.
- **Delete every Drop-only pin** (C2) and the allocator role of P16 and P17. At P16 and P17, `set_current_cell_id(0)` stays as the trap-attribution handoff, and `ChargeScope` replaces its charging role (`replace(Charge::KERNEL)`).
- **`realloc` stays the default** (allocate under the current scope). The owner word records the new owner. Growth under a kernel scope never fails on quota.
- `rt_heap` is out of scope. It is dead (C14), and Spec 02 already says so.

### 8.3 `FrameRun` carries the charge

- Replace `RunOwner` (`memory/frame_run.rs:45-56`) with `charge: RunCharge { owner: Charge, account: Option<Account> }`. Delete `RunOwner::Vm`: VMs become keyed by incarnation, and the tid stays only in logs.
- Constructors (`FrameRun::mapped`/`unpublished`, `PrivateRun::allocate`/`adopt`, `OwnedFrame::into_run`) take a `Reservation`, or `Charge::KERNEL`. The reservation is taken **before** the frame allocator is called, so refusal comes before any effect.
- **Refund on free, against the record.** `FrameRun::free` (`memory/frame_run.rs:454-466`) refunds `pages × 4096` to `charge` right after `backend.free`. This happens whoever frees the run: the owner's syscall, retirement, or the hart-0 reaper in the timer ISR. The refund is a lock-free atomic and allocates nothing. Quarantined runs stay billed until they are freed. Leaked runs (`:479-494`) are never refunded; `deregister` resets the row at retirement.
- **Accounts for Phase 17** (scope per phase-17 Requirements):

| Frames | Account | Change from today |
|---|---|---|
| Thread stacks (F2), SHM (F6) | Memory | Same bytes; the refund moves into the run and becomes generation-safe |
| Domain page tables (F8) | Memory | **New**. `AddressSpace` holds the `Charge` from `admit_and_bind_domain` (`task/launch.rs:415-440`), and lazy table growth draws from it |
| Guest RAM, Stage-2/EPT/NPT tables, xAPIC, VMCB/VMCS/bitmaps (F10-F18) | Guest | **New** |
| Kernel-root tables (F7), boot frames (F22), the x86 demand frame (§3 I10, `memory/paging.rs:1544`) | — (KERNEL) | Explicitly kernel |
| Root stacks, segments, grant pages (F1, F3, F4, F5) | — (owner recorded, no account) | Unchanged; see Open questions |

### 8.4 Guest-memory budget

- Add `GUEST_IN_USE: [AtomicUsize; MAX_CELLS]` beside `DMA_IN_USE` (`memory/cell_quota.rs:390`). Its limit is `Limits.guest`, default 0. A cell without a budget is refused on its first `CreateVm`.
- Add a kernel-only field `guest_memory: usize` to `catalog::Cell` (`catalog.rs:28-43`), unexported:
  - `/bin/hypervisor` = 1544 MiB (§6.1)
  - `/bin/silo` = 1 MiB
  - the `hypervisor-test` row = 4 MiB **[I]**: its launch row was not read; it gets the hypervisor cap through a test path
  - every other row = 0

  `TaskLaunchState` gains `guest_limit`, filled from `catalog::find(path)` at `loader/governed_spawn.rs:213`. Init (`:311`) and the admission probe get 0. A `/mem/` label matches no row, so it gets 0.
- **`create_vm`, both arches.** The first effect after the cap check is

  ```rust
  Reservation::take(charge, Account::Guest, guest_bytes + nested_table_bound(guest_pages) + VM_FIXED_PAGES * 4096)?
  ```

  This happens before `Stage2Table::new` (`arch/aarch64/vm.rs:402`) and `NestedPageTable::new` (`arch/x86_64/hypervisor/svm_registry.rs:196`). The tag lease comes next (§8.5), and the VMID moves up from `:464`. Each later frame `split`s from that reservation, and the lazy tables of `map_guest_memory` draw from the remainder. `create_vcpu` takes its own reservation before `vm_structure` (`:270`, `:320-323`).
- **Refusal returns `ViError::OutOfMemory`,** as the handoff states. `task/syscall/vm.rs:64-65` must pass `OutOfMemory` through instead of mapping everything to `NotSupported`. No error number moves.
- **Registry key `(Charge, vm_id)`.** Use one `VmKey` for both registries, with an id that never repeats per incarnation (a monotonic counter, not `count + 1`). Reap per incarnation in the root-retirement funnel next to `release_shm_for` (`task.rs:1626-1629`), not per tid (`task.rs:1922`). The syscall ownership check becomes "caller's incarnation == key.charge".

### 8.5 One tag allocator with a per-owner limit

New file `memory/tag_pool.rs`:

```rust
pub(crate) enum Reuse { AfterInvalidation, Never }
pub(crate) struct TagPool { first: u16, count: u16, reuse: Reuse, per_owner: u16, /* bitmap + (Charge, held) table */ }
pub(crate) struct TagLease { pool: &'static TagPool, value: u16, owner: Charge }
pub(crate) struct Invalidated(());       // minted only by the invalidation that proves the tag is clean
impl TagPool { pub(crate) fn acquire(&'static self, owner: Charge) -> Result<TagLease, TagRefusal /* Exhausted | OwnerLimit */>; }
impl TagLease { pub(crate) fn release(self, proof: Invalidated); }  // Drop without release: slot stays reserved, counted
```

| Instance | Replaces | Range | Reuse | Proof minted by | Per-owner |
|---|---|---|---|---|---|
| `PRIVATE_ASIDS` | `LIVE_ASIDS`/`AsidLease` (`memory/address_space.rs:1319-1460`) | 1..=256, capped by `asid_width()` | AfterInvalidation | `tlb_shootdown::confirm_tag_invalidation` | 1 per incarnation |
| `VMIDS` | HAL `VmidPool` (deleted from `hal/arch/arm/src/aarch64/stage2_regs.rs:49-110`; `VMID_COUNT` stays) | 1..=255 | AfterInvalidation | `s2_tlb_flush_all` in `reap` | 4 |
| `SVM_ASIDS` | `NEXT_ASID` (`arch/x86_64/hypervisor/svm_registry.rs:132-143`) | 1..=`max_asid()` (runtime) | Never (unchanged) | — | 4 |

`Charge::KERNEL` is exempt from `per_owner`, so the 255-VMID fixture keeps working as a kernel-owned fixture. A new fixture shows that one cell is refused at its fifth VM while kernel and other cells still get tags.

### 8.6 Fixtures

- **(a) An allocation in interrupt context while Cell A runs is not charged to A.**
  - Host test in `memory/heap.rs`: register A with limit 0. Set the dispatch identity to A (`set_current_cell_context(A, gA)`) and open `ChargeScope::enter(A)`, as a syscall would. Then open `ChargeScope::interrupt()` and allocate. The allocation succeeds, `in_use(A) == 0`, and the owner word is `UNCHARGED_OWNER`. After the interrupt scope ends, an allocation is refused for A, which proves the scope restored.
  - Boot fixture `IRQ-ALLOC-CHARGE` (test-hooks, RV64/x86/AArch64): register A with limit 0, set the hart-0 identity and scope to A, and arm a probe in `vi_timer_tick` that calls `try_box([0u8; 64])` and records its owner word. Spin with interrupts on until the probe fires, then force one housekeeping pass. Assert that the probe succeeded, its owner word is the kernel's, and `in_use(A) == 0`.
  - Rewrite `SMP-ROOT-EXIT-QUOTA` (`arch/riscv64/task/retirement_selftest.rs:836-870`) to saturate through a scope instead of the dispatch identity.
- **(b) A VM whose guest RAM exceeds its owner's budget is refused before any mapping.** Boot fixture `VM-GUEST-BUDGET`, run on the `virt-vhe` lane and the x86 SVM/VMX hypervisor lanes (`scripts/lanes.toml:178,183`):
  1. Register B with guest limit 64 pages.
  2. Snapshot `frame::stats()`, `largest_free_run()`, the VMID/ASID pool free counts and `GUEST_IN_USE[B]`.
  3. Call `create_vm(B, 65)`. Assert `Err(OutOfMemory)` and that every snapshot is unchanged: no root, no carve, no map, no tag.
  4. Call `create_vm(B, 32)`. It succeeds, and the charge equals guest RAM plus tables plus the fixed pages.
  5. Reap. The charge returns to 0.

  Add host tests for `Reservation` refusal and draw-down, and for a stale-generation refund on `GUEST_IN_USE`.

### 8.7 Commits in risk order

1. **Delete Drop-only pins** (P4, P6, P8, P12, P20, P25, the `release_pipe` half of P9). Accounting is unchanged (C2). Host tests and boot lanes.
2. **`Charge`, `Account`, `Reservation`, lock-free limits, generation minted before reservation.** Convert the explicit callers in §2.4 and delete P10. The allocator is untouched. Host tests.
3. **Allocator reads `alloc_charge`.** Add `ChargeScope`, the syscall-entry scope, the `yield_cpu` and ISR kernel scopes, and explicit owners at P1-P3, P5, P7, P9 and P15. Delete `KernelAllocationContext` and `PipeAllocationContext`. Fix P14's infallible `reserve`. Rewrite the §2.3 tests. Add fixture (a). Update Spec 02 §2. Every cell's measured heap use moves, so re-measure and record (handoff §4).
4. **`FrameRun` carries `RunCharge`** (replaces `RunOwner`). Thread stacks and SHM move their charge into the run. Domain page tables are charged. Boot all three arches with domains.
5. **Guest account, catalog budget, VM creation.** Reserve in `create_vm`/`create_vcpu` before any frame, pass `OutOfMemory` through, key the registries by `(Charge, vm_id)`, and reap at incarnation retirement. Add fixture (b). Run the hypervisor lanes on x86 and AArch64, plus the Chrome-profile boot.
6. **`TagPool`.** Migrate the private-root ASIDs, VMIDs and SVM ASIDs, add the per-owner limit, move the VMID fixture to kernel ownership, and add the owner-limit fixture. Update `scripts/mutable-globals.toml` entries `:1201,1212,2132,2143,2154,2176`. This touches tag invalidation on every arch, so it runs last.

## 9. Risks

- **The syscall-entry scope keeps today's caller charge for unlisted syscall allocations.** Kernel bookkeeping stops being billed to cells only at the sites converted in §2.1 and in all interrupt and switch paths. Any other syscall-path allocation stays charged to the caller, as today. This is deliberate: billing it to the kernel would let a cell grow the kernel heap without bound.
- **Kernel-funded memory loses its accidental cap.** Housekeeping and ISR allocations that a saturated cell used to refuse now always succeed. They are bounded only by structure limits (mailbox depth, `MAX_OPERATIONS`, the quarantine cap) and by total heap.
- **Measured usage moves.** Fixtures that assert exact bytes (`task/thread_quota_selftest.rs:105-130`, `task/ipc_pending_selftest.rs:617-655`, `sched/scheduler.rs:459-505`) must keep passing. Telemetry `quota_charged_bytes` changes meaning without a layout change.
- **Generation truncation.** A refund matches on the low 24 bits of a global counter. A stale refund could alias only after 2^24 launches between two incarnations of the same CellId. Today's per-row counter has the same window per CellId.
- **Refunds in the timer reaper must stay lock-free.** `FrameRun::free` runs on hart 0 inside `vi_timer_tick`. Any lock added to `refund` would rank under `FRAME_ALLOCATOR` in an ISR.
- **A budget is not availability.** 1544 MiB of budget does not guarantee a 1536 MiB contiguous carve (`memory/frame.rs:1741-1763`). The fixture must snapshot the allocator, not assume a carve succeeds.
- **AArch64 VMID ordering.** Moving `VMIDS.allocate()` before the Stage-2 build changes the order inside `registry_lock` (`arch/aarch64/vm.rs:472-484`). The lease must be released on every error path between acquire and insert, with no TLBI needed because the VMID was never bound.
- **Per-incarnation VM reaping.** A VM now lives until root retirement instead of until its creating tid exits. The hypervisor cells are single-threaded (§6.1), so nothing observable changes **[I]**.
- **HAL change.** Removing `VmidPool` from `hal/arch/arm/src/aarch64/stage2_regs.rs` is in the request's §3 file set. Its only user is `arch/aarch64/vm.rs:17,75-78`.

## 10. Open questions for the owner

1. **Grant pages, root stacks and cell segments stay uncharged in Phase 17.** One cell can take 16 MiB of frames per `GrantAlloc` with no total bound (`task/syscall/grant.rs:165,1317`). Charging these to the Memory account would raise many cells' measured usage past today's 16 MiB default, which is a behaviour change beyond the handoff's §4. Should they be charged in this phase (and with what limit), or recorded as a residual?
2. **`libs/abi/src/telemetry.rs:20-22` states the ambient rule** ("kernel heap in its context"). Correcting that doc comment touches `libs/abi` with no byte or number change. Is a doc-only edit allowed under the "no `libs/abi` change" rule, or should the comment stay as it is?
3. **Guest budget values.** Confirm 1544 MiB for `/bin/hypervisor` (Chrome 1536 MiB + about 3 MiB of nested tables + headroom) and 1 MiB for `/bin/silo`. The values sit in a kernel-only catalog field that is not exported to `scripts/catalog.generated.json`, so neither the manifest nor the signed policy changes.
