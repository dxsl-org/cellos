# CP1 draft: bounded IRQ waits for driver cells (phase 04b re-plan)

Status: **superseded 2026-11-04.** The owner chose option B; see [cp1-04b-waitcompletion-irq.md](cp1-04b-waitcompletion-irq.md). The wake-latency note in §2 is wrong. The scheduler sweep runs on hart 0 only, and only when the tick advanced or NET_RX is pending, so an MSI does not make the sweep run on an idle CPU. The option B package corrects this (E9, D-B-6). Written 2026-11-04 against HEAD `b3b5d51ea`. Phase 04b was deferred because `WaitIrq` has no timeout. A driver that swaps its poll-and-deadline loop for `WaitIrq` would hang on one lost interrupt (see the [phase file](phase-04b-irq-driven-pcie-drivers.md) Deviation Log).

## 1. Evidence

| # | Fact | Where |
|---|---|---|
| E1 | `WaitIrq` (234) takes `a0 = irq`, `a1 = mmio_base`. `a2` and `a3` are ignored and every in-tree caller passes 0 (`ostd::syscall::sys_wait_irq`). | `kernel/src/task/syscall.rs` WaitIrq arm; `libs/ostd/src/syscall.rs` |
| E2 | `TaskState::WaitIrq { irq }` has no deadline. The scheduler sweep wakes it only on the pending flag ("No deadline: IRQ-only wake"). | `kernel/src/task/tcb.rs:131-135`; `kernel/src/task/scheduler.rs` sweep |
| E3 | The neighbouring wait states already carry `deadline: Option<u64>` in absolute `system_ticks` and are expired by the same sweep (`RecvTimeout`, `WaitForEvent`, `WaitCompletion`). | `tcb.rs:57-114`; scheduler sweep |
| E4 | On x86, `WaitIrq` accepts only vectors the caller holds from `AllocPcieIrq` (CP2-04a). | `syscall.rs` WaitIrq arm |
| E5 | Driver cells sit in `sys_recv` when idle. They poll only while a command is outstanding. NIC receive is driven by requests from the net service. | `libs/ostd/src/app.rs`; `cells/services/net` |

## 2. Options

| | A: `WaitIrq` timeout | B: IRQ source in `WaitCompletion` |
|---|---|---|
| ABI | `a2 = timeout_ticks` (0 = forever, today's behaviour). Returns 0 on interrupt, 1 on expiry. Wire compatible, because old callers pass 0. | New source bit `IRQ = 1 << 3`. It fires for any vector the caller holds; `result` = the IRQ number. Combines with `IPC` and `TIMER` in one wait. |
| Kernel | Add `deadline` to `TaskState::WaitIrq`. Add one sweep arm that releases the waiter registration and returns 1. About 40 lines plus tests. | Hooks into the completion queue: a reservation per waiting thread, ISR-safe signalling into it, and rotation with the other sources. Several hundred lines, in completion code that is not in the S1 file set. |
| Covers | Command and event waits in nvme, ahci and xhci, which are single-threaded and block one command at a time. | All of A, plus NIC cells that must wait for an interrupt *or* an IPC request at the same time (interrupt-driven RX). |
| Files beyond the 04a set | `kernel/src/task/{tcb,scheduler}.rs` (sweep arm only); `libs/api` syscall doc; `libs/ostd` wrapper. | The above plus the kernel completion module and `libs/api/src/abi/completion.rs`. |
| Risk | Low. Same pattern as `RecvTimeout`. | Medium. The completion queue has its own capacity and abandonment rules (`RESULT_ABANDONED`). |

**Wake latency**, under either option: the interrupt only sets a pending flag. The waiter becomes Ready at the next scheduler sweep. On an idle CPU that sweep runs straight after the interrupt wakes `hlt`. On a busy CPU it waits for the next tick. This is the same as `WaitIrq` today; no IPI kick is proposed.

## 3. Recommendation

- **Now: option A**, so that 04b can convert nvme, ahci and xhci to `irq=msix|msi` while keeping their deadlines. The success criterion `no irq=polled on q35` would then cover the storage and USB cells.
- **Later: option B** for interrupt-driven NIC RX (e1000, igb, virtio-net, phase 08+ NICs), proposed as its own CP1 when a NIC slice needs it.
- **Until then:** NIC cells and the virtio cells keep request-driven polling and log `irq=polled reason=request-driven`.

## 4. Proposed ABI delta (option A)

- **`WaitIrq = 234`:** `a0 = irq`, `a1 = mmio_base` (unchanged), `a2 = timeout_ticks` (new; 0 waits indefinitely), `a3` must be 0.
- **Returns:**
  - 0 when the interrupt fired;
  - 1 when the deadline passed. The waiter registration is released, so the next `WaitIrq` on the same line succeeds;
  - the existing errors are unchanged.
- **New ostd wrapper:** `sys_wait_irq_timeout(irq, timeout_ticks) -> Result<bool, SyscallError>`, where `true` means the interrupt fired. `sys_wait_irq` keeps its behaviour.
- **Lost-wakeup rule (unchanged):** `take_pending` is checked before parking. On expiry the pending flag is left as it is, so an interrupt that lands at the deadline is reported by the next wait.

## 5. Lock handoff requested (04b)

- **Kernel and ABI:**
  - `kernel/src/task/tcb.rs` (the `WaitIrq` variant);
  - `kernel/src/task/scheduler.rs` (the sweep arm only);
  - `kernel/src/task/syscall.rs` (the WaitIrq arm);
  - `libs/api/src/abi/syscall.rs` (documentation);
  - `libs/ostd/src/syscall.rs`.
- **Cells:** `cells/drivers/{nvme,ahci,xhci}/**`.
- **Tests:** `tests/integration/tests/{nvme,ahci,xhci}-x86.rs`.
- **G1 private lane (`cellos_g1_cpu`):** untouched. It would need a check that it does not depend on the `TaskState::WaitIrq` shape. `[INFERENCE]`: it does not, because G1 is CPU accounting.

## 6. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| D-04b-1 | Option A now (timeout on `WaitIrq`); option B later as its own CP1 | Yes |
| D-04b-2 | Re-scope 04b to nvme, ahci and xhci; NIC and virtio cells stay polled with a named reason | Yes |
| D-04b-3 | Approve the §5 lock handoff | Yes |

## 7. Acceptance (re-scoped 04b)

- Host tests:
  - a `WaitIrq` with a deadline expires and returns 1;
  - the next wait still succeeds;
  - an interrupt that is already pending returns 0 without parking.
- `nvme-x86`, `ahci-x86`, `xhci-x86` and `iommu-dmar-x86` pass under TCG and KVM with `irq=msix` (nvme, xhci) or `irq=msi` (ahci) in the log.
- A throwaway fault test masks the device's interrupt and shows the timeout path is taken and logged, with no hang.
