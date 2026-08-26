---
phase: 01
title: Stop Silent Death — Guard Pages + Reboot-on-Panic
priority: P0
status: planned
depends_on: ["00"]
risk: high
---

# Phase 01 — Stop Silent Death

> ⚠️ **Red-team revisions (authoritative — override conflicting text below):**
> - **SRST already exists.** `system_reset` is implemented at [syscall.rs:1281-1294](../../kernel/src/task/syscall.rs)
>   (EID `0x53525354`, syscall 502, `shutdown` cmd; OpenSBI on target honors it). **DELETE** the
>   "add `system_reset` + `probe_extension`" step. Instead reuse the existing inline pattern with
>   `a0=1` (cold reboot) in the kernel-panic branch ([main.rs:342](../../kernel/src/main.rs)). ~5 lines.
> - **Drop the "user-VA stack window".** Stacks are contiguous **identity-mapped** frames and
>   [stack.rs:67-81](../../kernel/src/task/stack.rs) admits "no Virtual Address Allocator yet" —
>   a VA window is a hidden new subsystem, out of scope. Do **in-place guard unmap with a PA check**:
>   skip the unmap iff the guard frame's PA is a kernel page-table frame (the real blocker per
>   stack.rs:113-117). SUM=1 is already always set, so the SUM concern is moot.
> - **Add an `extern "Rust" fn vi_is_guard_fault(stval) -> bool` shim** — the HAL trap handler
>   can't depend on kernel paging directly (cf. `vi_current_cell_id`). Add it to the file list.
> - **Reboot safety now lives in Phase 00** (panic misclassification + lock-leak). This phase
>   only flips the panic-branch from `wfi` to cold-reboot AFTER Phase 00 makes classification correct.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.1
- Code: [kernel/src/task/stack.rs](../../kernel/src/task/stack.rs) (guard unmap disabled @113-118)
- Code: [kernel/src/main.rs](../../kernel/src/main.rs) (`#[panic_handler]` @310-343)
- Code: [hal/arch/riscv/src/common/sbi.rs](../../hal/arch/riscv/src/common/sbi.rs) (SBI calls)

## Overview
- **Priority:** P0 (cheapest, blocks the worst failure mode)
- **Status:** planned
- **Description:** Two independent silent-death killers: (1) stack overflow currently
  corrupts neighbor memory with no trap because guard-page unmapping is disabled;
  (2) a true kernel panic `wfi`-halts forever instead of rebooting. A robot needs a
  loud, recoverable failure — never silent corruption, never a frozen brick.

## Key Insights
- Guard frame IS already allocated (`pages + 1`); only the **unmap** is skipped. The blocker
  (per the in-code comment): removing the kernel identity-map PTE makes `memset` during
  page-table setup fault, because stack frames share VAs with kernel page-table physical
  addresses. Fix requires a **user-VA region distinct from kernel identity map** for stacks.
- SBI **SRST extension** (EID `0x53525354`) provides `system_reset(type, reason)` —
  type `0x00000001` = cold reboot, `0x00000000` = shutdown. Confirm OpenSBI advertises SRST
  via `sbi_probe_extension`; fall back to shutdown-then-watchdog if absent.

## Requirements
**Functional**
- Stack overflow on any task stack raises a page fault that is identified as a guard-page hit.
- A Tier-1 cell guard-page hit → kill the cell (reuse fault path), kernel survives.
- A kernel-stack guard-page hit → kernel panic path (loud), then reboot.
- True kernel panic triggers SBI cold reboot after flushing the audit ring to console.

**Non-functional**
- No regression to boot or to existing cell spawning.
- Reboot path must not depend on a working scheduler/heap (panic context may be corrupt).

## Architecture
```
Stack::allocate(guard=true)
  ├─ allocate pages+1 frames in a USER-VA window (new: separate from kernel identity map)
  ├─ map usable pages RW (+U for user)
  └─ leave guard frame UNMAPPED  ← re-enabled

trap (load/store/instr page fault)
  ├─ stval ∈ a known guard page range?  → guard-fault
  │     ├─ current cell != 0 → terminate_current_cell_on_fault (existing)
  │     └─ current cell == 0 → kernel panic → reboot
  └─ else existing handling

panic_handler (kernel branch)
  ├─ print PanicInfo via SBI DBCN (existing)
  ├─ best-effort audit flush to console
  └─ sbi::system_reset(COLD_REBOOT)   ← new; on failure, wfi-loop (current behavior)
```
Guard-page identification: maintain a small registry of `[guard_va_start, guard_va_end)`
ranges (one per live stack), checked in the trap handler against `stval`. Alternative
(simpler): mark unmapped guard PTEs with a reserved software bit and check the PTE in the
fault handler. Prefer the PTE-bit approach (O(1), no registry, no lock).

## Related Code Files
**Modify**
- `kernel/src/task/stack.rs` — re-enable guard unmap; allocate stacks in user-VA window.
- `kernel/src/memory/paging.rs` — helper to map/unmap with a reserved guard SW bit.
- `hal/arch/riscv/src/rv64/trap.rs` — classify guard-page faults before generic handling.
- `hal/arch/riscv/src/common/sbi.rs` — add `system_reset` (SRST) + `probe_extension`.
- `kernel/src/main.rs` — panic handler calls `system_reset` after diagnostics.

**Create**
- (none — keep within existing modules; no mod.rs)

## Implementation Steps
1. Add `sbi::probe_extension(eid)` and `sbi::system_reset(reset_type, reason)` in `sbi.rs`
   with `// SAFETY:` on the `ecall`. Document SRST EID and return semantics.
2. In `main.rs` kernel-panic branch: after printing, call `system_reset(COLD_REBOOT, 0)`;
   if it returns (unsupported), fall through to the existing `wfi` loop.
3. Introduce a reserved guard SW bit in the PTE flags in `paging.rs`; add
   `map_guard_unmapped(va)` that installs a non-valid PTE carrying the guard bit.
4. Carve a user-VA stack window (below `USER_VADDR_MAX`, disjoint from kernel identity map)
   and allocate task stacks there so unmapping the guard frame cannot disturb kernel memset.
5. Re-enable the guard unmap in `stack.rs::allocate` (remove the `let _ = guard;` skip).
6. In `trap.rs` page-fault arm: if the faulting PTE carries the guard bit → guard-fault →
   route to `terminate_current_cell_on_fault` (cell) or panic→reboot (kernel).
7. Build (`cargo check` per-arch), boot in QEMU.

## Todo List
- [ ] SBI `system_reset` + `probe_extension`
- [ ] Panic handler reboots (with wfi fallback)
- [ ] Guard SW-bit PTE + `map_guard_unmapped`
- [ ] User-VA stack window
- [ ] Re-enable guard unmap
- [ ] Trap classifies guard faults
- [ ] Test: deliberate stack overflow in a test cell → cell killed, kernel alive
- [ ] Test: forced kernel panic → QEMU shows reboot

## Success Criteria
- A test cell that recurses to overflow its stack is **killed** (audit `CellFault`), shell
  stays responsive.
- A deliberately panicking kernel build **reboots** in QEMU (visible re-run of boot banner).
- Normal boot + spawn + hotswap regression: unchanged.

## Risk Assessment
- **Stack VA relocation breaks context switch / existing pointers (High).** Stacks moving to
  a user-VA window may interact with SUM bit and trap-frame sp setup. *Mitigation:* land the
  VA-window change behind a build that boots before re-enabling unmap; bisect in two commits.
- **Reboot loop on a persistent panic (Med).** If panic cause survives reboot, device
  hard-loops. *Mitigation:* gate reboot behind a boot-count / "panic reason" check later
  (Phase 04 supervisor can escalate); for now a reboot beats a freeze for robots.
- **SRST unsupported on target firmware (Low).** *Mitigation:* probe + wfi fallback.

## Security Considerations
- Guard SW bit must be a kernel-only PTE concept; cells cannot set page flags (no `unsafe`,
  no paging syscall). No new attack surface.

## Next Steps
- Phase 02 builds on a known-good fault/reboot baseline to add active detection.
