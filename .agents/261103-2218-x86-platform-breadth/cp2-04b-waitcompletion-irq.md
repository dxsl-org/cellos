# CP2 — phase 04b: `IRQ` source for `WaitCompletion`, MSI wake hint, IRQ-driven nvme/ahci/xhci

Status: **awaiting the owner's decision (Law-1 confirmation 2).** Not approved.
Written 2026-11-05 after implementation on `feat/x86-04b-irq-drivers` (base `6fee21927`).
CP1: [cp1-04b-waitcompletion-irq.md](cp1-04b-waitcompletion-irq.md) (approved 2026-11-04, D-B-1…D-B-10), scheduled by [lane-04b-schedule-approval.md](lane-04b-schedule-approval.md).
Evidence: [reports/phase-04b-evidence.md](reports/phase-04b-evidence.md).

Commits:
- `5b1a3aac9` feat(kernel): IRQ completion source for WaitCompletion with an MSI wake hint (ABI, kernel, ostd, Law-1 digest script, Specs 17/03b/20)
- `3221af05b` feat(nvme): MSI-X completion waits through WaitCompletion(IRQ)
- `7fe8a6e41` feat(ahci): MSI completion waits through WaitCompletion(IRQ)
- `39c874adb` feat(xhci): MSI-X event-ring waits through WaitCompletion(IRQ)

## 1. ABI delta as implemented

| Item | As approved in CP1 §4 | As implemented | Delta |
|---|---|---|---|
| `source::IRQ` | `1 << 3` | `pub const IRQ: u32 = 1 << 3;` (`libs/api/src/abi/completion.rs`) | none |
| `ALL`, `is_single_supported` | add `IRQ` | `NET_RX \| TIMER \| IPC \| IRQ` in both | none |
| Record | v1, len/version/magic unchanged; `result` = line | unchanged layout; IRQ bytes `08 00 00 00`, result = line (`irq_record_carries_the_line_and_refuses_a_mixed_source_word`) | none |
| Mask tests | 1..16 accepted, `1 << 4` refused | `wait_masks_are_non_empty_subsets_of_the_known_sources`, `irq_is_the_next_single_bit_and_leaves_existing_values_unchanged` | none |
| 242 a0 bits 32..63 (D-B-7) | must be zero → `InvalidInput` | decode maps a mask that does not fit `u32` to `u32::MAX`, which the mask rule refuses with `InvalidInput` (`syscall.rs` decode, test `wait_completion_refuses_mask_bits_above_31_instead_of_dropping_them`) | none in behaviour |
| `IRQ` off x86_64 | `InvalidInput` | `source_is_valid` refuses the bit unless `target_arch = "x86_64"`; rv64 boot self-test passes with that row | none |
| No held line | `PermissionDenied` at entry or after every line was revoked | entry snapshot before the slot is reserved; after a failed take with an empty refresh the slot is released and `PermissionDenied` returned | none |
| Rotation | `NET_RX → IPC → IRQ → TIMER`, `INITIAL_ROTOR = len - 1` | `SELECTION_ORDER: [u32; 4]`, `INITIAL_ROTOR = (SELECTION_ORDER.len() - 1) as u8` | none |
| Raw message vs `IRQ` | does not interrupt | `task.rs` NET_RX-only interruption byte-identical | none |
| Wake hint (D-B-6) | `on_msi` sets `MSI_WAKE`; hart-0 pass swaps it into `events_pending` | `irq_wait::hint_msi_wake()` after `signal_irq`; `scheduler.rs:2226-2227` `has_any_pending() \| take_wake_hint()` (non-short-circuit) | none |
| Sweep arm | before the deadline arm, wakes on a pending snapshot line | `scheduler.rs` arm `WaitCompletion { source, .. } if completion_wait::irq_wake_due(task, *source)` | none |
| `pcie_function.rs:283` doc | mention `WaitCompletion(IRQ)` | done | none |
| ostd | `Wake::Irq(u8)`, decode 0..=255, `park_for_terminal` error arm | done; `sys_alloc_pcie_irq`/`sys_wait_completion_detailed` docs name `IRQ` | none |
| Opcode / allowlist | 242, bit 42; 435 bit 50 | unchanged | none |

ISR invariant: the MSI handler still does only atomic stores (pending flag, then hint); no lock, no scheduler access. New lock holds: `pcie_msi::STATE` alone (snapshot, `take_owned`, `repend_owned`) and `SCHEDULER` alone (select, publish, sweep — atomics and the task's own snapshot only). No nesting in either direction.

## 2. Deviations that need ratification

1. **File outside CP1 §6.** `kernel/src/task/ipc_pending_selftest.rs:911`: `!cw::source_is_valid(IPC | (1 << 3), Some(1))` → `(1 << 4)`. The boot self-test asserted that bit 3 is unknown; once `IRQ = 1 << 3` is valid on x86 it would log `ipc-pending self-test FAIL`. Granted by Main during implementation as a one-token edit; no other change in that file.
2. **`select_locked` keeps its signature; `WaiterRegistration::with_irq_lines` added.** CP1 §5 proposed passing `irq_ready` into `select_locked`. `ipc_pending_selftest.rs` (out of scope) calls both `select_locked(task, mask, net_rx, timer)` and `WaiterRegistration::new(queue, tid, slot, source)`. So `select_locked` peeks the task's own `CompletionWait.irq_lines` itself — the same lock-free acquire loads CP1 §3.3 step 1 names, done while `SCHEDULER` is held (as publish and the sweep do) — and `new` delegates to `with_irq_lines(.., IrqLineSet::EMPTY)`. Semantics are those of CP1 §3.3; no lock edge changes.
3. **NVMe allocates MSI-X before `CC.EN`.** Found in an instrumented KVM run: with 435 called after the controller was enabled, the admin CQ (vector 0) never signalled; every admin wait ended at its 100 ms park slice and the cell still came up. QEMU binds the admin CQ vector when the controller is enabled and drops notifications for a vector bound while MSI-X was off. The cell now calls 435 between programming AQA/ASQ/ACQ and `CC.EN`. Driver-only; no ABI effect.
4. **Driver wait shape (CP1 §7 detail).** Each cell spins a short bounded number of reads (nvme 64, ahci 32, xhci 64), then parks with `wait_any(IRQ, 10)` (≤ 100 ms per park) and re-reads the device after every return. Budgets: nvme keeps its millisecond budgets and refuses IRQ mode without a monotonic clock (`irq=polled reason=no-clock`); ahci and xhci charge each park against their existing poll-count budgets, so a lost interrupt ends in the same timeout. xhci's steady-state keyboard poll stays on the IPC receive timeout (an `IRQ` wait is not woken by IPC, D-B-5); only bring-up command/control-transfer waits park.

## 3. Acceptance (CP1 §9)

| Criterion | Result |
|---|---|
| Host api: `IRQ == 8`, masks 1..16, `1 << 4` refused, IRQ round trip, `IRQ\|NET_RX` refused | met (`cargo test -p api`: 192 passed) |
| Host kernel rows (4-source rotor, line round robin, `IrqPending`, sweep predicate, revoke/`PermissionDenied`, copy-out re-pend, lone NET_RX/TIMER unchanged) | met (432 passed); the sweep row is covered through `irq_wake_due`, the predicate that is the whole sweep arm, not by driving `pick_next` |
| Host ostd: `Wake::Irq(0x41)`, out-of-range → `Decode` | met |
| Digest script | `PASS` |
| `completion_selftest` on x86 and rv64, rv64 refuses `IRQ` | met (x86 TCG/KVM and rv64 TCG boot logs: `completion-queue self-test PASS`) |
| nvme/ahci/xhci/iommu-dmar lanes with `irq=msix`/`irq=msi`, TCG and KVM | met; nvme/ahci fault-ISO witnesses skipped (fault ISO not rebuilt) |
| async_lifecycle bench and net lanes unchanged | net lanes nic-x86, igb-x86, virtio-x86 pass under TCG and KVM; the async_lifecycle bench was **not run** |
| Masked-vector run takes the deadline path, logs it, no hang | met with a real lost vector (deviation 3's pre-fix build), not a synthetic mask |
| Idle-guest median MSI-to-wake < 1 ms vs tick bound | median 14 µs (KVM idle, n=130); the no-hint baseline was **not measured** |

## 4. Not changed / out of scope

`WaitIrq = 234` ABI and gate; NIC cells (e1000, igb, virtio-net) stay `irq=polled reason=request-driven` until the `MAILBOX` CP; virtio-blk/scsi PCI stay `04b-deferred`; interrupt remapping (phase 12); MSI on non-x86; no IPI on MSI (a busy BSP still wakes the waiter at the next tick).

## 5. Owner decision

| ID | Question | Recommendation |
|---|---|---|
| CP2-04b-1 | Accept §1 as implemented; freeze `IRQ = 8` (cannot be reassigned afterwards) | Yes |
| CP2-04b-2 | Ratify deviations 1–4 in §2 | Yes |
| CP2-04b-3 | Accept the two unmeasured items (async_lifecycle bench, no-hint latency baseline) as follow-up evidence, or require them before close | Owner's choice |

## 6. Approval record

Pending.
