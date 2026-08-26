# Phase 00 — Prerequisites: Syscalls + PlatformCap + Test-Code Gate

## Context Links
- Plan: [plan.md](plan.md)
- Law: `docs/specs/15-kernel-boundary.md`
- Reference cap: `kernel/src/task/cap.rs:72` (PcieDriverCap), `:59` (SupervisorCap)
- Reference blocking: `kernel/src/task/syscall.rs` RecvTimeout (sets `TaskState::Recv{deadline}` then `yield_cpu()`)
- IRQ waker: `kernel/src/task/waker.rs` (`signal_net_rx`, `consume_pending`, `csrsi sip,0x2`)

## Overview
- **Priority:** P1 — blocks ALL of 01-07.
- **Status:** complete (prereqs verified pre-existing 2026-06-24)
- **Risk:** HIGH (Law 1 ABI change + a new kernel blocking primitive that the trap/IRQ path must wake).
- **Description:** Add the two kernel primitives every Driver Cell needs (`sys_wait_irq`, `sys_register_pcie_bar`), the `PlatformCap` ZST token gating BAR registration, and gate the `user_hello` test code behind `test-hooks`. Nothing else can start until these land.

## Key Insights (verified)
- **Syscall numbers**: highest assigned is **420** (`Snapshot`, `libs/api/src/syscall.rs:72`). The dense free range after `GrantDma=233` is **234-255** (220-227 are hypervisor ops — the brief's "~220+" suggestion is WRONG). Assign `WaitIrq=234`, `RegisterPcieBar=235`.
- **Blocking pattern exists**: `WaitForEvent=217` already blocks a task on an event mask with a deadline via `TaskState::WaitEvent` + `yield_cpu()`, woken by the scheduler timer sweep / `waker::consume_pending`. `sys_wait_irq` reuses this machinery — **do not invent a new scheduler state**; add an IRQ-keyed wake to the existing waker.
- **IRQ dispatch today**: trap handler → `vi_handle_virtio_irq(irq)` (`kernel/src/task/drivers/virtio_blk.rs:82`) → per-device ack. For Cells, the kernel ISR must instead (a) ack the PLIC/device and (b) wake the Cell blocked on that IRQ. We add an IRQ→TID wait table.
- **PcieDriverCap is path-granted, not manifest** (`loader.rs:300`). `PlatformCap` follows the same pattern, granted only to `/bin/platform`, and is a **singleton** (kernel rejects a 2nd grant).
- **`user_hello.rs`** (112 LOC) is kernel-resident test code — blacklist item "Test/debug code → `#[cfg(feature='test-hooks')]` only".

## Requirements

### Functional
1. `sys_wait_irq(irq_num: u8) -> ViResult<()>` — block caller until `irq_num` fires; kernel ISR wakes it.
2. `sys_register_pcie_bar(bdf: u32, bar_base: usize, bar_len: usize) -> ViResult<()>` — populate kernel PCIE_BARS table; `PlatformCap`-gated.
3. `PlatformCap` ZST in `cap.rs`; granted by exact path `/bin/platform`; singleton.
4. `user_hello.rs` gated behind `#[cfg(feature = "test-hooks")]`.

### Non-Functional
- `sys_wait_irq` wake adds **no address-space switch** (SAS benefit — that's the whole point vs Fuchsia port model). Latency target: ISR→Cell-resume within one scheduler tick.
- Lost-wakeup safe: an IRQ that fires *between* the Cell's queue-check and its `wait_irq` call must not be missed (mirror `consume_pending`'s pending-flag-before-park guard).

## Architecture

### Data flow — `sys_wait_irq`

> **⚠️ Red-team fix (S2):** The ISR must NOT hold SCHEDULER lock or directly mark a task Ready.
> `waker.rs:9-10` is the invariant: wakers use lock-free atomics only; actual Ready transition
> happens in the scheduler sweep (next pick_next call). The diagram below is correct; any version
> that says "ISR wakes tid" (directly setting Ready) is WRONG and deadlocks.

```
Driver Cell                Kernel syscall              ISR context           Scheduler sweep
-----------                --------------              -----------           ----------------
service queue (empty)
sys_wait_irq(irq=3) ──────► validate PcieDriverCap
                             check IRQ_PENDING[3].swap(false)?
                               yes → return Ok(())       (lost-wakeup guard: IRQ already fired)
                               no  → IRQ_WAITERS[3].store(caller_tid, Release)
                                     task.state = WaitIrq{irq:3}
                                     yield_cpu()
                                                  ... device raises IRQ 3
                                                  plic_claim() → irq=3
                                                  irq_dispatch(3):
                                                    ack PLIC (plic_complete)
                                                    ack VirtIO InterruptStatus (offset 0x60)
                                                    IRQ_PENDING[3].store(true, Release)
                                                    ← ISR returns; NO SCHEDULER touch
                                                                          next pick_next():
                                                                            for WaitIrq{irq} tasks:
                                                                              if PENDING[irq].swap(false):
                                                                                task.state = Ready
                                                                                push_ready(tid)
   ◄────── resume ──────────────────────────────────────────────────────────── (tid scheduled)
service VirtIO used-ring
```

**Implementation rule:** `irq_wait::signal_irq(irq)` (called from ISR) does ONLY:
```rust
IRQ_PENDING[irq as usize].store(true, Ordering::Release);
// Nothing else. No SCHEDULER, no Spinlock, no wake call.
```
The `wake_irq` sweep (transition to Ready) runs in `scheduler::pick_next()` — which already
holds SCHEDULER and can safely modify task states. Model exactly on `waker::consume_pending`.

**Shared-IRQ / duplicate-waiter policy (H2):** `IRQ_WAITERS[irq]` holds exactly one TID.
A second `sys_wait_irq` on an already-claimed IRQ returns `Err(AlreadyClaimed)`. On RISC-V/ARM64
VirtIO MMIO each slot has a distinct IRQ → no sharing. On x86_64 PCI, Driver Cells POLL (no
`sys_wait_irq`), so INTx sharing never arises. Document this in the Cell template.
service VirtIO used-ring
```

### Data flow — `sys_register_pcie_bar`
```
Platform Cell                       Kernel
-------------                       ------
(after ECAM scan finds VirtIO dev)
sys_register_pcie_bar(bdf, base,len) ─► validate PlatformCap (singleton holder)
                                        PCIE_BARS.push(PciBarEntry{bdf,base,len})
                                        record BDF in resource_registry (ownership)
                                     ◄─ Ok(())
... later, VirtIO Block Cell:
sys_find_pcie_device(0x01,0x80,..) ──► reads PCIE_BARS / PCI_DEVICES → returns bar0_base
```

### Kernel structures to add
- `kernel/src/task/drivers/irq_wait.rs` (NEW): `IRQ_WAITERS: [AtomicUsize; MAX_IRQ]` (TID, 0=none) + `IRQ_PENDING: [AtomicBool; MAX_IRQ]`. Functions `register_waiter(irq, tid)`, `take_pending(irq)->bool`, `wake_irq(irq)->Option<tid>`.
- `kernel/src/task/tcb.rs`: add `TaskState::WaitIrq { irq: u8 }` variant (parallel to `WaitEvent`).
- `kernel/src/task/cap.rs`: add `PlatformCap(())` ZST + `static PLATFORM_CAP_GRANTED: AtomicBool` for singleton enforcement.
- `kernel/src/task/drivers/pcie_ecam.rs`: add `register_bar(bdf, base, len)` writing into the existing `PCI_DEVICES`/`PCIE_BARS` store so `find_class` keeps working (Platform Cell now feeds it instead of `init()`).

## Related Code Files

**Modify (Law 1 — needs 2× confirm):**
- `libs/api/src/syscall.rs` — add `WaitIrq = 234`, `RegisterPcieBar = 235`; add to `From<usize>`; add allowlist bits 51/52 in `declare_syscalls!`.
- `libs/ostd/src/syscall.rs` — add `sys_wait_irq`, `sys_register_pcie_bar` wrappers.

**Modify (kernel — no Law 1):**
- `kernel/src/task/syscall.rs` — dispatch arms for 234/235; cap checks.
- `kernel/src/task/cap.rs` — `PlatformCap`.
- `kernel/src/task/tcb.rs` — `TaskState::WaitIrq`.
- `kernel/src/loader.rs` — grant `PlatformCap` to `/bin/platform` (path match near line 300); singleton check.
- `kernel/src/task/scheduler.rs` (or wherever `pick_next` lives) — `WaitIrq` tasks are not Ready until woken (no timeout sweep; IRQ-only wake).
- the trap/IRQ handler that calls `vi_handle_virtio_irq` — route to `irq_wait::wake_irq(irq)` for Cell-claimed IRQs (keep kernel-driver dispatch for not-yet-migrated devices).
- `kernel/src/task/drivers/pcie_ecam.rs` — `register_bar()` entrypoint.

**Create:**
- `kernel/src/task/drivers/irq_wait.rs` — IRQ wait/pending table.

**Modify (test gate):**
- `kernel/src/task/user_hello.rs` — wrap module in `#[cfg(feature = "test-hooks")]`; remove its call site in `main.rs` unless under same cfg.
- `kernel/Cargo.toml` — confirm `test-hooks` feature exists (it gates `layer2_selftest.rs` already).

## Implementation Steps

1. **Spike: confirm IRQ-wake feasibility.** Read the trap handler IRQ path (the `plic_claim`/`plic_complete` site that currently calls `vi_handle_virtio_irq`). Confirm we can (a) identify which IRQ fired and (b) call into scheduler to mark a TID Ready from ISR context (interrupts disabled). Confirm `WaitForEvent` already does an analogous wake — model on it.

2. **🔴 LAW 1 GATE — get user confirmation #1 and #2** before editing `libs/api/src/syscall.rs`. Present: "Adding `WaitIrq=234`, `RegisterPcieBar=235` to the kernel ABI; allowlist bits 51, 52. OK to proceed?" Wait for explicit 2× yes.

3. **`libs/api/src/syscall.rs`**: add the two enum variants with ABI doc-comments; add `234 => WaitIrq`, `235 => RegisterPcieBar` to `From<usize>`; add allowlist bits 51 (`WaitIrq`) and 52 (`RegisterPcieBar`) to the `declare_syscalls!` mapping.

4. **`libs/ostd/src/syscall.rs`**: add wrappers mirroring `sys_request_mmio` style:
   - `pub fn sys_wait_irq(irq: u8) -> Result<(), SyscallError>` → `syscall(WaitIrq, irq as usize,0,0,0)`; 0=Ok.
   - `pub fn sys_register_pcie_bar(bdf: u32, base: usize, len: usize) -> Result<(), SyscallError>` → `syscall(RegisterPcieBar, bdf as usize, base, len, 0)`.

5. **`kernel/src/task/cap.rs`**: add `pub struct PlatformCap(());` with `pub(crate) fn new()`, plus a module-level `static PLATFORM_CAP_GRANTED: AtomicBool` and `try_grant_platform() -> Option<PlatformCap>` that returns `None` if already granted (singleton). Add `platform_cap: Option<PlatformCap>` field to the Task TCB.

6. **`kernel/src/loader.rs`**: at the path-match grant site (near `:300`), add `if path == "/bin/platform" { if let Some(c) = cap::try_grant_platform() { t.platform_cap = Some(c); } else { log::error!("[loader] PlatformCap already granted — refusing 2nd /bin/platform"); /* reject spawn */ } }`.

7. **`kernel/src/task/tcb.rs`**: add `WaitIrq { irq: u8 }` to `TaskState`.

8. **Create `kernel/src/task/drivers/irq_wait.rs`**: tables + `register_waiter`, `take_pending`, `wake_irq`. `MAX_IRQ` ≥ highest PLIC line used (VirtIO MMIO slots 1-8 + e1000 + nvme; pick 64 to be safe). Add `pub mod irq_wait;` to drivers parent (parallel `drivers.rs`, NOT a mod.rs).

9. **`kernel/src/task/syscall.rs`** dispatch:
   - `WaitIrq` arm: `if !caller_has_pcie_driver(caller) && !caller_has_platform(caller) { Err(PermissionDenied) }`; else `if irq_wait::take_pending(irq) { Ok(()) }` else set `TaskState::WaitIrq{irq}`, `irq_wait::register_waiter(irq, caller)`, `yield_cpu()`, then `Ok(())` on resume.
   - `RegisterPcieBar` arm: `if !caller_has_platform(caller) { Err(PermissionDenied) }`; else `pcie_ecam::register_bar(bdf, base, len)`, record BDF in resource_registry, `Ok(())`.

10. **Trap/IRQ handler**: in the IRQ dispatch site, before/instead of `vi_handle_virtio_irq` for migrated IRQs: `if let Some(tid) = irq_wait::wake_irq(irq) { /* marked Ready */ }` — and ack the device/PLIC. For not-yet-migrated devices keep the existing dispatch. (During 01-07, both coexist; Phase 08 removes the kernel-driver branch.)

11. **Scheduler**: ensure `WaitIrq` tasks are excluded from the ready set until `wake_irq` flips them to `Ready`. No deadline sweep (IRQ-only). Add to fault-teardown force-unlock list if `irq_wait` uses any lock.

12. **`user_hello.rs` gate**: wrap with `#[cfg(feature = "test-hooks")]`; guard its `main.rs` call site identically. Verify non-test build drops it.

13. **Build + boot smoke**: `cargo check` (all arches), boot RISC-V — no Cell uses the new syscalls yet, so boot must be byte-identical to pre-change. Add a throwaway test cell calling `sys_wait_irq` on an unused IRQ to confirm it blocks (and a manual device IRQ wakes it) — gate under test-hooks, remove before merge.

## Todo List
- [ ] Spike trap-handler IRQ wake feasibility
- [ ] 🔴 Law 1 confirmation #1
- [ ] 🔴 Law 1 confirmation #2
- [ ] syscall.rs enum + From + allowlist bits (api)
- [ ] ostd wrappers
- [ ] PlatformCap + singleton + TCB field
- [ ] loader path grant + singleton reject
- [ ] TaskState::WaitIrq
- [ ] irq_wait.rs table
- [ ] syscall dispatch arms + cap checks
- [ ] trap handler wake routing
- [ ] scheduler WaitIrq exclusion + force-unlock
- [ ] user_hello test-hooks gate
- [ ] cargo check all arches + boot-identical smoke

## Success Criteria
- [ ] `cargo check` passes riscv64 + aarch64 + x86_64.
- [ ] Boot output identical to pre-Phase-00 (no Cell uses new syscalls yet).
- [ ] Throwaway test cell: `sys_wait_irq(N)` blocks; a manually-triggered IRQ N wakes it exactly once; second call with no IRQ re-blocks (no spurious wake).
- [ ] `sys_register_pcie_bar` from a non-PlatformCap cell returns `PermissionDenied`.
- [ ] Spawning a 2nd `/bin/platform` is rejected (singleton).
- [ ] `user_hello` symbols absent from a non-`test-hooks` kernel binary.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Lost wakeup (IRQ fires between queue-check and `wait_irq`) | Med | High (Cell hangs) | `IRQ_PENDING` latch + `take_pending` before park — mirror `consume_pending` |
| ISR-context wake touches a held lock → deadlock | Med | High | `irq_wait` uses lock-free atomics only; no Spinlock in ISR path |
| Spurious double-wake | Low | Med | `wake_irq` clears waiter atomically (compare_exchange) |
| Law 1 ABI churn breaks an existing cell's syscall numbering | Low | High | Only ADD new numbers (234/235); never renumber existing |
| `WaitIrq` task starves (IRQ never fires) | Med | Med | Driver Cells must also poll-fallback on timeout in their own loop (document in 05/06); kernel side has no built-in timeout by design |

## Security Considerations
- `sys_wait_irq` gated by `PcieDriverCap`/`PlatformCap` — a non-driver Cell cannot block on or observe hardware IRQs.
- `sys_register_pcie_bar` gated by singleton `PlatformCap` — only the one trusted Platform Cell can declare device BARs, preventing a malicious Cell from forging a BAR mapping to alias another device's MMIO.
- BDF ownership recorded in resource_registry on registration → prevents two Cells claiming the same device.

## Next Steps
- Unblocks Phase 01 (Platform Cell needs `RegisterPcieBar` + `PlatformCap`) and all Driver Cells (need `WaitIrq`).
- Phase 05 depends on the IRQ-wake machinery proven here.
