---
title: "x86_64 Per-Vector IDT and Vector-Aware Dispatch"
description: "Replace ambiguous x86_64 interrupt handlers with generated per-vector entries, exact frame handling, CPL3-safe GS/PKRU transitions, deterministic routing, and boot-level proof."
status: complete
priority: P1
effort: 20h
branch: main
tags: [refactor, critical]
blockedBy: []
blocks: []
created: 2026-09-02
---

# x86_64 Per-Vector IDT and Vector-Aware Dispatch

## Overview

Make every IDT gate identify its vector, normalize CPU error frames, preserve interrupted state, and route by vector plus saved CPL. Attributable Ring-3 exceptions retire the current Cell through the existing trap-fault hook; kernel and non-attributable faults remain fatal. The cutover keeps IST=0, preserves 0x80 DPL3 and the independent SYSCALL path, treats 0xff as LAPIC-spurious/no-EOI, and adds actual-entry plus production-boot proof without changing excluded virtualization or QEMU-version work.

## Baseline

- Current generic no-error entry cannot identify its vector and EOIs unconditionally; every generic error-code entry reads CR2 and calls the page-fault hook.
- Current error set omits architectural vectors 21, 29, and 30 from normalized generic handling, and the frame type reads RSP/SS even for same-CPL entries.
- Before implementation, capture `cargo check -p cellos-kernel --target x86_64-unknown-none` and the production boot command in Phase 1; failures are baseline facts, not regressions to hide.

## Late Safety Gate Resolution (2026-09-02)

Review of the completed CPL0 probe found that hardware IDT entry does not
switch GS or PKRU. The repaired common entry now derives the transition from
saved CS: CPL3 entry swaps to kernel GS and sets kernel PKRU before Rust, while
return restores the selected task's PKRU and swaps back to user GS only after
interrupts are masked. The same audit corrected user-state preservation in
`syscall_entry`, selected-task PKRU restoration in the fresh `__trap_exit`
path, and scheduler GS-base ownership. A dedicated two-task real-CPL3 oracle
closed the blocker; generic `test-hooks` and production remain fixture-free.

## Phases

| Phase | Name | Status | Dependency |
|---|---|---|---|
| 1 | [Generate and Install Exact Entry Stubs](./phase-01-entry-stubs.md) | complete | — |
| 2 | [Implement Vector-Aware Dispatch](./phase-02-vector-dispatch.md) | complete | 1 |
| 3 | [Prove Actual Entry, CPL3 Transitions, and Timer Delivery](./phase-03-entry-probes.md) | complete | 2 |
| 4 | [Run Production Regression and Close Documentation](./phase-04-regression-docs.md) | complete | 3 |

## Ownership and Critical Path

Phases remain serial because the entry ABI, syscall return paths, and scheduler context boundary share one GS/PKRU invariant. Phase 3 now owns the repaired CPL3 transition paths and mandatory two-task oracle; Phase 4 may close only after both dedicated and production lanes pass. TSS/IST, virtualization, guest behavior, and emulator-version policy remain excluded.

## Acceptance Commands

1. `cargo test -p hal-x86 --lib --target x86_64-unknown-linux-gnu idt::policy::tests`
2. `cargo check -p cellos-kernel --target x86_64-unknown-none`
3. `bash scripts/build-x86_64-idt-test-ci.sh && objdump -d --disassemble=x86_64_idt_common target/x86-idt-test/x86_64-unknown-none/release/cellos-kernel && BOOT_WINDOW=90 bash scripts/qemu-x86_64-idt-test.sh`
4. `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc && bash scripts/x86/make-iso-ci.sh && BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh`
5. `cargo test --manifest-path tests/integration/Cargo.toml --test x86_64-boot -- x86_echo_command`

## Final Late-Gate Evidence

- `[PASS]` All focused modules and scripts remain below 200 lines; the largest
  is `idt/probe.rs` at 182 lines.
- `[PASS]` The dedicated runner enforced underlying QEMU debug-exit status 33
  and observed exactly one retained
  `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
  marker plus exactly one
  `X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544`
  marker. It observed two timer wakeups before one scheduler initialization and
  no FAIL/PANIC/FAULT/SKIP/RESET/TRIPLE-FAULT marker.
- `[PASS]` Linked production and dedicated disassembly proved the exact
  saved-CS-conditional SWAPGS/PKRU IDT contract, kernel PKRU before Rust,
  selected-task PKRU before return, masked return windows, corrected
  syscall-state preservation, and late SWAPGS in fresh and resumed exits.
- `[PASS]` The two real Ring-3 tasks proved fresh entry, INT80, timer
  preemption, suspended-syscall resume, kernel/user GS balance, and task PKRU
  values `0x55555550` and `0x55555544`.
- `[PASS]` Generic `test-hooks` contained none of the 18 CPL0/CPL3 fixture
  symbols, six fixture namespaces, two markers, or terminal fixture call.
  Only `x86-idt-cpl3-test` selects the fixture and HAL `qemu-exit` dependency.
- `[PASS]` A freshly rebuilt production ISO reached the shell, excluded all
  fixture symbols/namespaces/markers, and passed all 7 x86 boot integration
  tests. Final verification and both final reviewers returned PASS.

## Historical CPL0-Only Completion Evidence (superseded by late gate)

- All five acceptance lanes passed. The final validation also passed all 9
  `hal-x86` host tests, all 89 kernel host tests, the x86-none check and
  warning-denying Clippy build, and all 7 `x86_64-boot` integration tests.
- The deterministic generator check found 256 ordered stubs, 256 ordered table
  entries, 256 `ENDBR64` entries, 246 synthetic-zero stubs, and exactly the ten
  hardware-error vectors 8, 10, 11, 12, 13, 14, 17, 21, 29, and 30. The linked
  table is 2,048 bytes and names 256 distinct aligned vector symbols.
- The dedicated QEMU lane exited with debug status 33 and emitted exactly one
  `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
  marker with no failure marker. The separately rebuilt production image
  reached the shell cleanly and contained no probe/debug-exit path.
- Linked disassembly confirmed the 15-GPR save/restore, `cld`, dynamic SysV
  alignment, exact vector/error unwind, and `iretq`. It also confirmed the
  bootstrap's 8-byte synthetic bottom-frame slot before the unchanged
  tail-jump into Rust.
- Final verification and independent stress review both returned PASS with no
  findings. Physical x86 qualification remains outside this QEMU-scoped plan.

## Rollback

Revert harness/docs first, then revert the generator, IDT installer, entry record, and dispatcher as one atomic unit; never combine old `extern "x86-interrupt"` handlers with normalized assembly stubs. Rebuild the production ISO and rerun the boot command after rollback.

## Security and Documentation Gate

Only vector 0x80 remains DPL3; it returns without EOI or syscall semantics. #PF alone reads CR2; attributable Ring-3 exceptions use the existing Cell-retirement hook, while kernel faults and NMI/#DF/#MC remain fatal. Exceptions, unknown vectors, 0x80, and LAPIC-spurious 0xff never EOI. Assembly probes prove all 15 GPRs, DF clear-on-entry/iret restoration, pre-call alignment, and saved-CS-controlled GS/PKRU transitions. The terminal two-task fixture and HAL debug-exit dependency are enabled only by `x86-idt-cpl3-test`; generic `test-hooks` and production exclude them. Phase 4 documents the dedicated command and records the change under the existing Unreleased convention.
