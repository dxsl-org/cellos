# CP1: `IRQ` source for `WaitCompletion` (phase 04b re-plan, option B)

Status: **approved by the owner on 2026-11-04 (D-B-1…D-B-10, all accepted; Law-1 confirmation 1). Not yet scheduled.** The §6 lock handoff takes effect when the owner schedules 04b into the Intel WIP lane. CP2 follows implementation. Written 2026-11-04 against HEAD `b3b5d51ea`; every cited source file is clean at that revision. The owner chose option B of [cp1-04b-waitirq-timeout.md](cp1-04b-waitirq-timeout.md), which this package supersedes. `[INFERENCE]` marks claims that were not read in code. Decisions are in §8.

## 1. Summary

- A wait naming `IRQ` (`1 << 3`) completes when any MSI/MSI-X line the **calling task** holds from `AllocPcieIrq` (435) has fired. The record's `result` is that line number. It combines with `TIMER`, `IPC` and `NET_RX` in one wait.
- The interrupt path does not change: the ISR still only sets a pending flag. The completion queue gets no interrupt-side producer. The reservation is waiter-owned, exactly as for `TIMER`/`IPC`, which removes most of the "several hundred lines" the 04b draft estimated.
- **Blocking finding for 04b (either option):** an MSI does not trigger the scheduler sweep. A waiter wakes at the next 10 ms tick, not on the interrupt (E9). Without the wake hint in §3.4 (D-B-6), interrupt-driven storage would be far slower than today's polling.
- A queued raw message does **not** interrupt an `IRQ` wait (§3.6). NIC cells therefore need a separate, explicit mailbox source and a net-service protocol change. That work is a follow-up CP; 04b converts nvme, ahci and xhci only.

## 2. Evidence

| # | Fact | Where |
|---|---|---|
| E1 | The MSI handler only calls `signal_irq(vector)`, which does a `Release` store to `IRQ_PENDING`. No lock is taken and the scheduler is not touched. MSI is programmed to the BSP. | `kernel/src/task/drivers/pcie_msi.rs:57-59`; `drivers/irq_wait.rs:114-135`; `hal/arch/x86/src/x86_64/msi.rs:53-60`; `pcie_msi/program.rs:84` (MSI-X destination `[INFERENCE]` same helper) |
| E2 | IRQ numbers are the IDT vectors (0x30..=0xDF), and `MAX_IRQ = 256`. | `pcie_msi/vectors.rs:3-4`; `irq_wait.rs:22-27`; `pcie_msi.rs:67-72` |
| E3 | An allocation is keyed by the **owner tid** (the BDF claimant), not by the cell. `irq_owned_by` takes `STATE`. The x86 `WaitIrq` gate uses it. | `pcie_msi.rs:39-43,75-83,200-207`; `kernel/src/task/syscall.rs:8996-9001` |
| E4 | `STATE` is never held while `SCHEDULER` is taken, and no pcie_msi path takes `SCHEDULER`. `allocate` takes `STATE` after the ownership lookup and the table mapping. `allocate` and `release_for` clear the pending flags of the lines they hand out or free. | `pcie_msi.rs:103-124,179-181,212-227`; grep of `pcie_msi{,/}` for `SCHEDULER` (none) |
| E5 | `teardown_dma_authority` (which calls `release_for`) never runs under `SCHEDULER`. The reap funnel takes its tid lists under the lock and reaps outside it. CapRevoke drops the lock before teardown. | `kernel/src/task.rs:1466-1475,1914-1959`; `syscall.rs:6624-6640,6660-6670` |
| E6 | `WaitIrq` checks `take_pending` first, then registers a single waiter in `IRQ_WAITERS`, then parks. Only the sweep wakes it. | `syscall.rs:9002-9017`; `scheduler.rs:2310-2318` |
| E7 | The sweep runs on hart 0 only, and only when the tick advanced or `has_any_pending()` (NET_RX only) is true. | `scheduler.rs:2167-2171,2223-2226`; `kernel/src/task/waker.rs:43-46` |
| E8 | The BSP idle loop is `yield_cpu(); wait_for_interrupt()`. | `kernel/src/main.rs:1754-1757` |
| E9 | One scheduler tick is 10 ms. Together with E7 and E8, an MSI that lands mid-tick on an idle BSP is not seen until the next tick. This contradicts the 04b draft's "sweep runs straight after the interrupt". | `libs/ostd/src/ipc.rs:121`; `docs/evidence/c2c-multi-source-wait-x86.txt:39` |
| E10 | `WaitCompletion` validates the mask, reserves a slot (`TryAgain` when the queue is full) and records the slot owner: `NET_RX` when the mask names it, otherwise `TIMER` (waiter-owned). Every mask other than a lone `NET_RX` or `TIMER` goes through `wait_multi`. | `kernel/src/task/completion_wait.rs:264-300` |
| E11 | Multi-source pattern: NET_RX readiness is resolved outside `SCHEDULER` and passed in; `select_locked` picks one source by rotor under `SCHEDULER`. `publish_wait_state_locked` re-checks before parking. A failed copy-out restores the source. | `completion_wait.rs:86-106,130-164,183-200,426-512` |
| E12 | The exit funnel releases a waiter-owned (`TIMER`-class) slot outside `SCHEDULER`. `WaiterRegistration::drop` clears `completion_wait`. | `scheduler.rs:1650-1666`; `task.rs:1446-1460`; `completion_wait.rs:61-74`; `tcb.rs:572-592` |
| E13 | A raw message readies a `WaitCompletion` only when its mask names `NET_RX`, and the Law-1 digest asserts this. | `task.rs:340-356`; `completion_wait.rs:189-191`; `scripts/check-multi-source-wait-law1-digests.sh:133-134` |
| E14 | `IPC` reports terminals of operations the **caller submitted**, not incoming requests. | `libs/api/src/abi/completion.rs:62-66` |
| E15 | Record v1: decoders refuse unknown sources. `IPC` was appended with no version bump. A test pins `1 << 3` as invalid today. | `completion.rs:31,68-81,114-137,264-284` |
| E16 | Decode takes `a0 as u32`, so bits 32..63 of the mask are silently dropped. The arguments are a0..a3 only. | `syscall.rs:10616-10631`; `libs/ostd/src/syscall.rs:2415-2425` |
| E17 | The surface is Law-1 FROZEN: any added source must update the digest script and records in the same commit. The asserted items include `ALL`, the `matches!` set and `SELECTION_ORDER: [u32; 3]`. | `scripts/check-multi-source-wait-law1-digests.sh:8-18,91-96,126`; `.agents/cell-contract-0024-locks.md:142` |
| E18 | The ostd `Wake` enum and `decode_wake` have an exhaustive match in `park_for_terminal`. Bench matches use `_` arms. | `libs/ostd/src/ipc.rs:138-180,240-243`; `cells/tests/bench/src/scenarios/async_lifecycle.rs:597-615` |
| E19 | Consumers: nvme polls the CQ with a deadline. NIC cells are `ostd::app` loops parked in `sys_recv` and serve requests from the net service, which itself waits on `NET_RX`. Virtio PCI transports park vectors. No cell calls 435 yet (grep, this session). | `cells/drivers/nvme/src/queue.rs:188-213`; `cells/drivers/e1000/src/main.rs:26,103`; `libs/ostd/src/app.rs:190-192`; `cells/services/net/src/service-runtime.rs:874-912`; `cells/drivers/virtio-blk/src/pci_x86.rs:6-7` |

## 3. Semantics

**3.1 Source and record.**
- `IRQ = 1 << 3`. A record has `slot` = this wait's reservation, `source = 8`, and `result` = line number (0x30..=0xDF on x86_64).
- One record reports exactly one line. Other pending lines stay pending for the next wait.
- `COMPLETION_VERSION` stays 1 and the record layout is unchanged, following the `IPC` precedent (E15). A decoder built before this change never sees `IRQ`, because the kernel writes it only for a wait that named it.

**3.2 Which lines.**
- A wait covers every line the calling task holds at entry. The kernel snapshots this set from `STATE` before any `SCHEDULER` hold into `CompletionWait.irq_lines`, a 256-bit set of 32 bytes (E2).
- The snapshot is re-validated when a line is consumed (§3.3).
- **No subset argument:**
  - All four argument registers are already used (E16).
  - Ownership is per tid (E3), so only the claiming thread can wait at all, and a per-queue thread split is impossible today.
  - A driver that wants per-queue handling reads `result`.
- Packing a subset into a0 bits 32..63 would be silently ignored by today's kernel (E16). D-B-7 therefore makes those bits must-be-zero now, reserving them.

**3.3 Readiness, consumption, fairness.** Each round of `wait_multi`:
1. **Peek** the pending flags of the snapshot lines without taking a lock (atomics only).
2. **Select** under `SCHEDULER`: `select_locked` receives the ready set, the same way `net_rx_ready` is passed in today. If `IRQ` wins the rotor, it picks the first ready line after `Task::completion_irq_cursor` (a new `u8`, initially 0xFF) and advances the cursor. This round-robin keeps one line from starving the others.
3. **Take** the chosen line outside `SCHEDULER` with `pcie_msi::take_owned(tid, line)`. Under `STATE` alone, it checks ownership and swaps the flag. Because `release_for` clears flags under `STATE` too (E4), a revoked and re-allocated vector can never be consumed by its former owner.
4. **If the take fails** (the line was revoked between peek and take):
   - put any NET_RX observation back on its level flag, as `finish_multi` does;
   - refresh the snapshot;
   - if the set is now empty, release the slot and return `PermissionDenied`; otherwise loop.
5. **Copy-out failure:** a failed copy-out re-pends the line through `pcie_msi::repend_owned(tid, line)` (also under `STATE`), mirroring the IPC and NET_RX restore (E11).

Rotation becomes `NET_RX → IPC → IRQ → TIMER`. `INITIAL_ROTOR` becomes `SELECTION_ORDER.len() - 1`, so the first selection still starts at NET_RX, and every ready source is served within four selections.

**3.4 Park and wake.**
- **Before parking:** `publish_wait_state_locked` returns a new `IrqPending` decision, and the loop continues as it does for `IpcPending`, when a snapshot line is pending. This is a lock-free read under `SCHEDULER`.
- **Sweep arm:** a new arm, placed before the deadline arm, matches `WaitCompletion { source }` where `source & IRQ != 0` and a snapshot line is pending. It makes the task Ready with `timed_out = false`.
- **Wake hint (D-B-6):**
  - `pcie_msi::on_msi` also stores `irq_wait::MSI_WAKE = true`.
  - The hart-0 pass swaps this flag unconditionally and ORs it into `events_pending` (`scheduler.rs:2224`).
  - Effect: an idle BSP sweeps right after the MSI wakes `hlt` (E8). A busy BSP still waits up to one tick, because no IPI is proposed.
  - Only the x86 MSI handler sets the hint, so other boards and the G1 lane see no behaviour change.
  - `WaitIrq` gets the same latency fix for free.

**3.5 ISR safety and lock order.** The ISR adds one atomic store and still takes no lock. New lock holds:
- `STATE` alone, in the waiter's syscall context (snapshot, take, re-pend);
- `SCHEDULER` alone (select, publish, sweep), reading only atomics and the task's own snapshot.

No path nests `STATE` inside `SCHEDULER` or the reverse (E4, E5), so no new lock-order edge is created. `teardown_dma_authority` stays outside `SCHEDULER` (E5).

**3.6 Raw IPC and NIC cells.**
- A queued raw message does **not** interrupt an `IRQ` wait.
- **Why not:** a storage cell waiting on one command, with block requests queued behind it, would get an immediate empty return on every call until the command completed. That is a spin, the very cost this change removes.
- **What NICs need instead:** NIC cells (E19) must wake on a receive interrupt *or* a net-service request. Note that `IPC` does not cover incoming requests (E14).
- **Proposed for the NIC CP, not this one:**
  - an explicit `MAILBOX = 1 << 4` source: level-triggered while `pending_msgs` is non-empty, `result = 0`, woken through `wake_after_ipc_publish`;
  - a net-service push or long-poll RX protocol.
- The `task.rs` NET_RX-only interruption and its digest line stay byte-identical (E13).

**3.7 `WaitIrq` on the same line.**
- Both calls are legal on the same line. Both consume the same `IRQ_PENDING` flag.
- They can never run concurrently: both are restricted to the owning tid (E3), and a task is in only one syscall at a time.
- `WaitCompletion(IRQ)` does not use `IRQ_WAITERS`, so it never takes the `WaitIrq` single-waiter slot. `WaitIrq` itself is unchanged.

**3.8 Death, revocation, queue.**
- **Slot ownership:** a mask without `NET_RX` keeps a waiter-owned (`TIMER`-class) slot, released on every return and by the exit funnel (E12). With `NET_RX`, the existing NET_RX ownership rules apply unchanged.
- `RESULT_ABANDONED` never applies to `IRQ`, because no ISR writes into the queue.
- **Death:** the snapshot dies with `CompletionWait`. Pending flags are dropped by `release_for` after the function is quiesced (E4, E5).
- **Revocation of a parked waiter does not wake it:** `release_for` clears the flags. The wait ends at its deadline or through another source, and the next call returns `PermissionDenied`. 04b drivers always pass a deadline.

## 4. Exact ABI delta

**`libs/api/src/abi/completion.rs`:**
- add `pub const IRQ: u32 = 1 << 3;`;
- `ALL = NET_RX | TIMER | IPC | IRQ`;
- `is_single_supported` matches `NET_RX | TIMER | IPC | IRQ`;
- `COMPLETION_LEN`, `COMPLETION_VERSION` and `COMPLETION_MAGIC` are unchanged;
- tests: the test at 276-284 now accepts masks 1..16 and rejects `1 << 4`; add `irq_is_the_next_single_bit…`.

**`WaitCompletion = 242`** (doc at `libs/api/src/abi/syscall.rs:751-783`):
- Arguments:
  - `a0` = mask, a non-empty subset of `NET_RX | TIMER | IPC | IRQ`; bits 32..63 must be 0 (D-B-7);
  - `a1`/`a2`/`a3` are unchanged.
- Returns:
  - 1 with a record;
  - 0 with no record (unchanged cases).
- Errors:
  - `InvalidInput`: unknown bit; non-zero high bits; `TIMER` without a deadline; `IRQ` on a non-x86_64 kernel; null buffer.
  - `PermissionDenied`: `IRQ` named and the caller task holds no 435 line, at entry or after every line was revoked mid-wait.
  - `BufferTooSmall`, `TryAgain` and `Unknown` are unchanged.
- Rotation doc: `NET_RX → IPC → IRQ → TIMER`.

No new opcode or allowlist bit: bit 42 stays shared. Holding a line already requires `PcieDriverCap` through 435.

**`libs/api/src/abi/pcie_function.rs:283`:** the doc becomes "wait with `WaitIrq` or `WaitCompletion(IRQ)`".

**ostd:**
- `ipc::Wake::Irq(u8)`;
- `decode_wake` maps `source::IRQ` with `0..=255` to `Wake::Irq`; any other result → `IpcError::Decode`;
- `park_for_terminal` (ipc.rs:240-243) puts `Wake::Irq(_)` into its error arm;
- the `wait_any` docs list `IRQ`. Drivers call `ipc::wait_any(IRQ, ticks)`.

## 5. Kernel change map (for the implementer)

| File | Change |
|---|---|
| `drivers/pcie_msi.rs` | `lines_owned_by(tid) -> IrqLineSet`, `take_owned(tid, line) -> bool`, `repend_owned(tid, line)`. `on_msi` sets the wake hint. |
| `drivers/irq_wait.rs` | `IrqLineSet` (`[u64; 4]`), `pending_in(&set)` peek, `MSI_WAKE` + `take_wake_hint()` |
| `completion_wait.rs` | entry snapshot / `PermissionDenied`; off-x86 rejection in `source_is_valid`; `SELECTION_ORDER` ×4; `select_locked(.., irq_ready, ..)`; take/refresh loop; `IrqPending`; restore on copy-out failure |
| `tcb.rs` | `CompletionWait.irq_lines`, `Task::completion_irq_cursor`, `WaitCompletion` doc |
| `scheduler.rs` | IRQ wake arm before 2303; hint in the 2224 condition. Nothing else. |
| `syscall.rs` | decode at 10619: reject `a0 >> 32 != 0` |
| `completion_selftest.rs`, `async_ipc.rs` (tests only) | validity rows; rotor/selection tests use the new signature and `INITIAL_ROTOR` |

## 6. Lock handoff requested (beyond the S1 set)

- **Kernel:** `kernel/src/task/{completion_wait,tcb,completion_selftest}.rs`.
- **`kernel/src/task/scheduler.rs`:** lines 2223-2226 and the WaitCompletion/WaitIrq sweep arms (2303-2318) only. This needs the same kernel-repair Phase 07 overlap grant as slice d (`cell-contract-0024-locks.md:138`).
- **`kernel/src/task/async_ipc.rs`:** the `#[cfg(test)]` module only.
- **ABI and SDK:** `libs/api/src/abi/completion.rs`; `libs/ostd/src/ipc.rs`.
- **Law-1 records:**
  - `scripts/check-multi-source-wait-law1-digests.sh`, in the same commit as the ABI edit;
  - `docs/specs/17-ipc-wire-contract.md` §9 (a new dated entry);
  - `docs/specs/03b-async-reactor-adr.md`;
  - `docs/specs/20-unified-ipc-contract.md` (§2.6/§3/§4 source lists).
- **Already in S1 and used here:** `syscall.rs`, `drivers/{irq_wait,pcie_msi}.rs`, `libs/api/src/abi/{syscall,pcie_function}.rs`, `libs/ostd/src/syscall.rs`, `cells/drivers/{nvme,ahci,xhci}/**`, `tests/integration/tests/{nvme,ahci,xhci,iommu-dmar}-x86.rs`, `CHANGELOG.md`.
- **Pinned byte-identical:** `kernel/src/task.rs`, `completion.rs`, `waker.rs`, `cells/services/net/**`, the G1 lane (`cellos_g1_cpu`).
- **Compatibility check:** the `261006-0934` completion-capacity row is unaffected `[INFERENCE]`, because no queue capacity changes.

## 7. 04b re-scope

- **Convert nvme, ahci, xhci:**
  - allocate with 435;
  - submit, then `wait_any(IRQ, remaining_deadline)`;
  - **always** re-read the CQ or event ring after any return, so a lost interrupt degrades to deadline polling, never a hang;
  - log `irq=msix|msi`. A refusal by 435 or 242 keeps the polled path with `irq=polled reason=<why>`.
- **Stay polled with named reasons:**
  - e1000, igb, virtio-net: `irq=polled reason=request-driven`, until the NIC CP (mailbox source plus net-service RX protocol);
  - virtio-blk and virtio-scsi on PCI: `04b-deferred`, because vector programming is a transport change.

**Implementation order, each step undone by `git revert`:**
1. ABI, kernel, ostd and Law-1 records in one commit (§4-§6).
2. Driver conversions, one commit per driver.
3. CP2.

**Cannot be undone:** once CP2 ships, `IRQ = 8` is frozen and cannot be reassigned.

## 8. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| D-B-1 | `IRQ = 1 << 3`, `result` = line, record v1 unchanged | Yes |
| D-B-2 | The wait covers all lines of the calling **task** (snapshot, re-validated on take); no subset argument | Yes |
| D-B-3 | No lines → `PermissionDenied`; non-x86 → `InvalidInput` | Yes |
| D-B-4 | Rotation `NET_RX → IPC → IRQ → TIMER`; one line per record, round-robin across lines | Yes |
| D-B-5 | A raw message does not interrupt `IRQ`; the NIC mailbox source goes to its own CP | Yes |
| D-B-6 | x86 MSI wake hint feeds the sweep condition (also speeds up `WaitIrq`) | Yes. Without it, 04b's value is doubtful (E9). |
| D-B-7 | `WaitCompletion` refuses non-zero a0 bits 32..63 | Yes (no in-tree caller sets them, E16) |
| D-B-8 | 04b = nvme, ahci, xhci; NIC and virtio PCI follow-ups | Yes |
| D-B-9 | Approve the §6 handoff, including the kernel-repair overlap lines | Yes |
| D-B-10 | Supersede option A; `WaitIrq` ABI unchanged | Yes |

## 9. Acceptance

**Host (`libs/api`):**
- `IRQ == 8`, single bit, disjoint from the other sources;
- masks 1..16 accepted, `1 << 4` refused;
- the IRQ record round-trips;
- a record with `IRQ|NET_RX` is refused.

**Host (kernel test modules):**
- the 4-source rotor serves every ready source within 4 selections;
- line round-robin;
- `IrqPending` stops a park;
- the sweep wakes on a pending snapshot line and ignores a pending line outside the snapshot;
- a take after revoke fails, and with an empty refresh the wait returns `PermissionDenied` with the slot released;
- copy-out failure re-pends the line;
- a lone `NET_RX`/`TIMER` path is unchanged.

**Host (ostd):**
- `decode_wake(IRQ, 0x41) == Wake::Irq(0x41)`;
- an out-of-range result gives `Decode`.

**Gates:**
- the updated digest script passes;
- `completion_selftest` passes on x86 and rv64, and rv64 rejects `IRQ`.

**QEMU (TCG and KVM):**
- `nvme-x86`, `ahci-x86`, `xhci-x86` and `iommu-dmar-x86` pass with `irq=msix` (nvme, xhci) or `irq=msi` (ahci);
- the `async_lifecycle` bench and the net lanes are unchanged;
- a throwaway masked-vector run takes the deadline path, logs it and does not hang;
- on an idle guest, the median MSI-to-wake time is under 1 ms, against today's tick bound of up to 10 ms.

## 10. Risks

- **Busy-BSP latency of up to one tick.** Drivers keep a short bounded poll before parking (nvme already has `POLL_YIELD_EVERY`/`FALLBACK_POLLS`, `queue.rs:224`).
- **A revoked waiter sleeps until its deadline** (§3.8).
- **Law-1 drift.** The digest script fails loudly.
- **Per-task growth** of 33 bytes `[INFERENCE: no size budget test read]`.
- **Hint sweeps on every MSI burst**, bounded by the interrupt rate.

## 11. Not checked

- The MSI-X message helper (assumed to be `bsp_message`, like MSI).
- Whether `yield_cpu` on the BSP always reaches `pick_next(0)`.
- ahci and xhci wait loops (taken from the 04a CP1, E3).
