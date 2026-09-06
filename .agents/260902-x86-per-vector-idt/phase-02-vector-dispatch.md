---
phase: 2
title: "Implement Vector-Aware Dispatch"
status: complete
priority: P1
effort: "5h"
dependencies: [1]
tier: thinking
---

# Phase 2: Implement Vector-Aware Dispatch

> **Required — deviation-log:** Log every Decision / Deviation / Surprise below when it occurs. Do not widen kernel ABI or acknowledge an unregistered vector without explicit evidence.

## Overview

Add a pure route/EOI policy and connect the common entry to existing kernel hooks. This phase removes CR2/page-fault ambiguity and makes every vector’s return or fatal behavior explicit.

## Requirements

- Functional: #PF (14) alone reads CR2 and calls `vi_handle_page_fault`; no other route uses CR2 or PF semantics.
- Functional: for saved CS RPL3, exceptions 0–31 except NMI=2, #DF=8, #PF=14, and #MC=18 call `vi_terminate_on_user_trap_fault(vector,RIP,0)` so an attributable Cell fault cannot halt the host.
- Functional: Ring-0 exceptions and non-attributable NMI/#DF/#MC are fatal without EOI; timer 0x20 EOIs before `vi_timer_tick`, while UART 0x24 drains before EOI.
- Functional: 0x80 is an explicit DPL3 legacy no-op return; configured LAPIC-spurious 0xff is an explicit no-callback/no-EOI return; all remaining vectors are fatal without EOI.
- Non-functional: routing is allocation-free and pure-policy testable by vector plus CPL; dispatch and policy files each stay below 200 lines.

## Architecture

A pure `classify(vector, origin) -> Policy { route, eoi }` separates saved-CS provenance from effects. Routes are `PageFault`, `TerminateUser`, `FatalException`, `Timer`, `Uart`, `LegacyInt80`, `LapicSpurious`, and `FatalUnknown`; EOI is `None`, `Before`, or `After`. Derive `origin` only from `cs & 3`, and make the pure API require it so tests cannot omit CPL.

Only `PageFault` executes `mov cr2`; it passes normalized error, RIP, CS, and `EntryFrame::interrupted_rsp()`. `TerminateUser` calls the existing `vi_terminate_on_user_trap_fault(vector as usize,rip as usize,0)` and returns only if that hook returns. Fatal routes write a bounded polled-COM1 diagnostic, execute `cli`, and loop on `hlt`. `LegacyInt80` and `LapicSpurious` return unchanged with no callback/EOI.

## Assumptions

None — the user-trap hook/signature (`kernel_abi.rs:117-124`, `task.rs:691-703`), LAPIC spurious configuration (`apic.rs:94-99`), and callback ordering constraints were read directly.

## Related Files / Ownership

| File | Action | Owner | Test impact |
|---|---|---|---|
| `hal/arch/x86/src/x86_64/idt/policy.rs` | Create pure classification | Phase 2 | focused host matrix |
| `hal/arch/x86/src/x86_64/idt/dispatch.rs` | Create effectful routes | Phase 2 | host compile + boot |
| `hal/arch/x86/src/x86_64/idt/fatal.rs` | Create only if needed to keep dispatch <200 | Phase 2 | boot diagnostics |
| `hal/arch/x86/src/x86_64/idt.rs` | Wire dispatcher module/symbol | Phase 2 | target compile |
| `kernel/src/memory/paging.rs` | Correct stale generic-error comments only | Phase 2 | PF behavior unchanged |
| `hal/arch/x86/src/x86_64/trap.rs` | Clarify `ViTrapFrame` is not IDT entry state | Phase 2 | docs-only code comment |

## Implementation Steps

1. Write the pure vector+CPL classifier and focused tests before effect wiring.
2. Assert Ring-0 and Ring-3 matrices: user #DE=0/#UD=6/#GP=13/#CP=21→terminate/none; the same Ring-0 vectors→fatal/none; NMI=2/#DF=8/#MC=18→fatal/none at both CPLs; #PF=14→PF/none at both CPLs.
3. Assert IRQ/software policy: 0x20→timer/before; 0x24→UART/after; 0x80→legacy return/none; 0xff→LAPIC-spurious return/none; 0x21 and 0xfe→fatal unknown/none.
4. Assert the generator-produced error-vector constant equals `[8,10,11,12,13,14,17,21,29,30]`; assert vectors 3,0x20,0x24,0x80,0xff are no-error stubs.
5. Implement dispatch as a total match. Keep CR2 inline only in #PF. For attributable saved-RPL3 exceptions, call `vi_terminate_on_user_trap_fault` with vector/RIP/zero address; never classify NMI/#DF/#MC as Cell-attributable.
6. Implement timer as `eoi(); vi_timer_tick()`, UART as `vi_handle_uart_irq(); eoi()`, and 0xff as immediate return. Do not infer EOI from an IRQ range.
7. Implement bounded fatal output/halt for kernel exceptions and unknowns. Keep 0x80 a no-op return documented as independent of SYSCALL.
8. Correct stale generic-error comments in the PF hook and distinguish `ViTrapFrame` from the IDT record in `trap.rs`; do not change either ABI.

## Test Scenario Matrix

| Priority | Scenario | Oracle |
|---|---|---|
| critical | user vs kernel exception | saved RPL3 attributable exception selects termination; same Ring0 vector is fatal |
| critical | non-attributable exception | NMI/#DF/#MC remain fatal at either CPL |
| critical | #GP vs #PF | no #GP route contains CR2/PF/EOI; #PF alone selects PF |
| critical | timer/UART ordering | exact `Before` vs `After` policy and effect order |
| high | int 0x80 / spurious 0xff | both return with no EOI; only 0x80 is DPL3 |
| high | unknown vector | fatal route, no EOI |
| medium | optional stack | same-CPL computes base+160; CPL3 reads offsets 160/168 |

## Success Criteria

- [x] `cargo test -p hal-x86 --lib --target x86_64-unknown-linux-gnu idt::policy::tests` passes CPL, spurious, EOI, and exact error-set matrices.
- [x] `cargo check -p cellos-kernel --target x86_64-unknown-none` links the generated entries, existing user-termination hook, and one dispatcher.
- [x] Search shows CR2 only in vector14 and EOI only in timer/UART; user-attributable faults cannot reach fatal halt, and 0xff cannot reach callback/EOI.
- [x] Phase 2 made no syscall, TSS/IST, VMM, or guest change; the later real-CPL3 gate corrected syscall/fresh-exit state handling as a logged Phase 3 deviation.

## Completion Evidence (2026-09-02)

- All 9 `hal-x86` host tests passed, including the vector/CPL policy and exact
  error-vector matrix; the x86-none kernel check and warning-denying Clippy
  build also passed.
- Final boundary tracing confirmed that only #PF reads CR2; attributable
  Ring-3 exceptions retire the Cell, while Ring-0 exceptions and NMI/#DF/#MC
  remain fatal. Timer uses EOI-before-callback, UART uses EOI-after-callback,
  and 0x80 plus 0xff return without callback or EOI.
- Phase 2's diff inventory found no syscall/SYSRET, TSS/IST, VMM, or guest
  change. The later mandatory real-CPL3 gate changed syscall and fresh trap-exit
  state restoration at the shared GS/PKRU boundary; both final reviewers
  accepted that deviation and the routing policy with no findings.

## Security Considerations

A user-triggered attributable exception must retire only that Cell, while NMI/#DF/#MC cannot be falsely charged to it. Int 0x80 must neither acknowledge LAPIC state nor enter syscall dispatch; 0xff must not EOI a spurious interrupt. Fatal diagnostics must not dereference attacker-controlled stack memory. #PF retains existing W^X/demand-paging enforcement.

## Risk Notes

Wrong CPL attribution lets a Cell halt the kernel or hides a host failure as a Cell fault. Derive origin from saved CS only. Keep EOI/callback order visible: timer after-callback EOI can be skipped by context switch, UART before-drain EOI can lose input, and LAPIC spurious must not EOI.

## Deviation Log

- **Deviation (2026-09-02):** The stale generic-error comments in
  `kernel/src/memory/paging.rs` were left unchanged. That pre-existing file is
  1296 lines, so even a comment-only edit violates the binding under-200-line
  touched-file rule. Runtime #PF/#GP separation is implemented entirely in the
  HAL dispatcher; the integration owner directed this minimal scope reduction.
- **Late safety deviation (2026-09-02):** The original no-syscall-change scope
  could not survive the real-CPL3 boundary audit: `syscall_entry` had to preserve
  every live user field before switching to kernel PKRU and restore a blocked
  task's frame-owned RSP, while fresh `__trap_exit` had to restore selected-task
  PKRU before a late user-GS swap. These fixes do not change the pure vector/EOI
  policy; the dedicated two-task oracle and final linked checks passed.
