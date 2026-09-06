---
phase: 3
title: "Prove Actual Entry, CPL3 Transitions, and Timer Delivery"
status: complete
priority: P1
effort: "6h"
dependencies: [2]
tier: thinking
---

# Phase 3: Prove Actual Entry, CPL3 Transitions, and Timer Delivery

> **Required — deviation-log:** Log every Decision / Deviation / Surprise below when it occurs. Test-only recovery may alter only the expected saved RIP and must remain absent from production.

## Overview

Add an isolated `x86-idt-cpl3-test` image that executes real no-error and CPU-error IDT entries, then observes a real LAPIC timer entry. The dedicated feature depends on the generic test harness without changing generic `test-hooks` behavior. The oracle combines an exact serial marker with debug-exit status so a reset, triple fault, timeout, or false-positive log cannot pass.

## Requirements

- Functional: assembly-owned probes execute actual vector3 (`int3`) and vector13 (invalid DS-selector load), then actual vector0x20 arrives through the existing LAPIC `sti; hlt; cli` smoke.
- Functional: each exception probe loads distinct sentinels into all 15 saved GPRs, requires the entry record to match, captures every register immediately after `iretq`, and restores its Rust caller’s SysV callee-saved state.
- Functional: each probe executes `std`; dispatch requires live DF clear while saved RFLAGS has DF set; post-iret assembly requires DF restored, captures it, then executes `cld` before any Rust call/return.
- Functional: #BP resumes naturally and #GP only by rewriting saved RIP to an armed recovery label; no deliberate #PF, `ud2`, or unarmed exception returns.
- Functional: record actual pre-call alignment through a test assembly shim, then observe real timer EOI-before-callback and callback return.
- Non-functional: all probe/debug-exit code is behind the dedicated `x86-idt-cpl3-test` feature; production and generic `test-hooks` image defaults remain unchanged and the dedicated runner is strengthened fail-closed.
- Late safety gate: the common entry must conditionally SWAPGS/zero PKRU only
  for saved CS.RPL3, and every Ring-3 return must restore the selected task's
  PKRU and user GS without an IRQ window.
- Late safety gate: a mandatory two-task real-CPL3 image requires CPUID.PKU,
  CR4.PKE, distinct valid PKRU values, a fresh INT80 round trip, exactly one
  timer switch to a suspended syscall, and deterministic A/B resume reports.
- Oracle: require status 33, one retained CPL0 marker, exactly one full CPL3
  marker, and zero FAIL/PANIC/FAULT/SKIP/reset/triple-fault output.

## Architecture

`idt/probe.rs` holds a single-BSP atomic state machine: `Idle → ArmedBp → SawBp → VerifiedBp → ArmedGp(recovery) → SawGp → VerifiedGp → TimerAfterEoi → Complete`. Each assembly function first saves RBX/RBP/R12–R15 plus its caller stack alignment, calls the arm helper, then loads these exact sentinels: RAX=`0x1111111111111111`, RBX=`0x2222222222222222`, RCX=`0x3333333333333333`, RDX=`0x4444444444444444`, RBP=`0x5555555555555555`, RSI=`0x6666666666666666`, RDI=`0x7777777777777777`, R8=`0x8888888888888888`, R9=`0x9999999999999999`, R10=`0xaaaaaaaaaaaaaaaa`, R11=`0xbbbbbbbbbbbbbbbb`, R12=`0xcccccccccccccccc`, R13=`0xdddddddddddddddd`, R14=`0xeeeeeeeeeeeeeeee`, R15=`0xffffffffffffffff`. It sets DF and triggers entry without reusing a sentinel register; #GP uses a RIP-relative memory operand to load DS selector `0xffff`.

Dispatch validates every field, normalized vector/error, saved DF=1, and live `pushfq` DF=0 before returning; only armed #GP changes RIP. After `iretq`, probe assembly stores all 15 GPRs before clobbering any, captures RFLAGS and requires DF=1, executes `cld`, restores its saved caller ABI state, and returns to Rust for exact comparison. A test-only assembly call shim records its entry RSP; `(shim_rsp + 8) & 15 == 0` proves common-entry RSP was aligned immediately before `call`. Linked disassembly must show `andq $-16,%rsp` and no intervening stack change.

After the CPL0 timer callback, the fixture emits the retained `X86-IDT-SELFTEST` marker, masks that timer, and continues booting. After scheduler initialization, the two-task Ring-3 state machine owns the terminal status: only its final A report emits the full `X86-IDT-CPL3` marker and exits successfully with status 33.

## Assumptions

- **Claim:** loading DS with selector `0xffff` on the q35/qemu64 test lane raises #GP with error `0xfffc` and leaves DS unchanged.
  **Confidence:** high
  **How to verify:** actual-entry lane must capture vector/error and reach its recovery label; any other result fails, not loosens, the oracle.
- **Claim:** `vi_timer_tick` returns during the pre-scheduler timer smoke.
  **Confidence:** medium
  **How to verify:** require the completion marker after callback return; if it context-switches, inspect boot ordering rather than skipping the callback.

## Related Files / Ownership

| File | Action | Owner | Size guard |
|---|---|---|---|
| `hal/arch/x86/src/x86_64/idt/probe.rs` | Create state/probes/oracle | Phase 3 | <200 lines |
| `hal/arch/x86/src/x86_64/idt/dispatch.rs` | Add cfg-gated probe seams | Phase 3 | <200 lines |
| `hal/arch/x86/src/x86_64/idt/entry.rs` | Add alignment shim and saved-CS GS/PKRU transitions | Phase 3 | <200 lines |
| `hal/arch/x86/src/x86_64/idt/probe_entry.rs` | Create assembly trigger/post-iret capture | Phase 3 | <200 lines |
| `hal/arch/x86/src/x86_64/idt/cpl3_entry.rs` | Real Ring-3 code blob | Late gate | <200 lines |
| `hal/arch/x86/src/x86_64/idt/cpl3_probe.rs` | State/frame/GS/PKRU oracle | Late gate | <200 lines |
| `hal/arch/x86/src/x86_64/idt/cpl3_platform.rs` | Capability/image/live-state checks | Late gate | <200 lines |
| `hal/arch/x86/src/x86_64/idt/probe_timer.rs` | Exact two-wakeup CPL0 handoff | Late gate | <200 lines |
| `kernel/src/task/x86_idt_cpl3.rs` | Map code and reserve A/B schedule | Late gate | <200 lines |
| `kernel/src/task.rs`, `kernel/src/task/syscall.rs` | Minimal cfg-gated call/intercept | Late gate | logged oversized deviation |
| `hal/arch/x86/src/x86_64/pku.rs` | Test-only deferred-activation log | Late gate | production policy unchanged |
| `hal/arch/x86/src/x86_64/syscall.rs` | Preserve user state and make resumed return GS/PKRU-safe | Late gate | existing assembly unit |
| `hal/arch/x86/src/x86_64/boot.rs` | Make fresh `__trap_exit` GS/PKRU-safe | Late gate | <200 lines |
| `kernel/src/task/scheduler.rs` | Select task PKRU without rewriting GS-base MSRs | Late gate | existing scheduler unit |
| `hal/arch/x86/src/x86_64/idt.rs` | Invoke probes after `lidt` under cfg | Phase 3 | <200 lines |
| `hal/arch/x86/Cargo.toml` | Add optional `qemu-exit` and dedicated x86 CPL3 feature | Phase 3 | manifest |
| `hal/core/Cargo.toml`, `kernel/Cargo.toml` | Forward the dedicated feature through HAL and make it depend on `test-hooks` | Late gate | manifest |
| `scripts/x86/make-iso-ci.sh` | Add backward-compatible kernel/root overrides | Phase 3 | <200 lines |
| `scripts/build-x86_64-idt-test-ci.sh` | Create isolated build/ISO lane | Phase 3 | <200 lines |
| `scripts/qemu-x86_64-idt-test.sh` | Create debug-exit runner | Phase 3 | <200 lines |

## Implementation Steps

1. Add x86 test-hook propagation and optional qemu-exit dependency; keep default features empty.
2. Implement atomic probe state/captures and a failure function that emits one FAIL marker and exits failure. Unexpected/unarmed exceptions retain production fatal behavior.
3. Implement both probes entirely in assembly: save caller callee-saved state, align nested helper calls, arm the expected entry, load the exact 15 sentinels above, `std`, and trigger without consuming a sentinel register.
4. In dispatch, assert all 15 record slots, vector/error/CS/RIP, same-CPL optional words, saved DF=1, and live DF=0. #BP returns unchanged; #GP requires error0xfffc/effective RSP=`base+160`, mutates only RIP, and returns.
5. Immediately after each `iretq`, store all 15 registers and RFLAGS to dedicated captures, verify DF=1, `cld`, restore RBX/RBP/R12–R15 and original stack, return, then compare capture to the exact sentinel array.
6. Route the common entry call through a cfg-gated assembly shim that records shim-entry RSP. Assert `(rsp+8)%16==0`; retain linked disassembly showing dynamic `and` and no stack-changing instruction before `call`.
7. Instrument timer route after EOI and after callback. Emit the expanded exact PASS marker and success exit only from final state.
8. Generalize ISO assembly with opt-in `X86_KERNEL`/`X86_ISO_ROOT`, build in isolated `CARGO_TARGET_DIR`, and add a dedicated runner with `isa-debug-exit,iobase=0xf4,iosize=0x04`.
9. Require status33, exactly one expanded PASS marker, and no FAIL/panic/fault marker. Status1, timeout124, reset/other status, missing/duplicate marker, or early exit fails.
10. Repair IDT/SYSCALL/fresh-IRET GS and PKRU transitions before enabling the
    Ring-3 fixture; scheduler stack updates must not write GS-base MSRs.
11. Map the position-independent probe blob USER+EXEC, spawn B then A with
    PKRU(B/A), and reserve the exact syscall/timer/resume order.
12. Force CPUID.PKU/CR4.PKE as an insecure test-only activation and fail on
    absence; require the full CPL3 marker and forbidden-output oracle.

## Test Scenario Matrix

| Priority | Entry | Required observation |
|---|---|---|
| critical | `int3` | entry has 15 sentinels + error0 + saved DF; post-iret has same 15 + DF restored |
| critical | invalid DS load | entry has 15 sentinels + #GP/error0xfffc; RIP-only recovery; exact post-iret restore |
| critical | common call | recorded shim proves pre-call RSP%16=0; disassembly confirms dynamic alignment |
| critical | LAPIC timer | real vector32, EOI returned before callback, callback returned |
| high | caller ABI | assembly restores RBX/RBP/R12–R15 and clears DF before Rust return |
| high | triple fault/reset | no status33+marker, therefore fail |
| high | production build | probe symbols/debug-exit dependency absent |
| critical | fresh Ring-3 entry | `__trap_exit` restores PKRU(A), swaps to user GS late, and reaches A's INT80 |
| critical | timer task switch | one timer preempts A, resumes B's suspended syscall with exact RIP/RDX and PKRU(B), then returns to A with PKRU(A) |
| critical | CPL3 IDT boundary | saved CS/SS/RSP, kernel GS/KGS, kernel PKRU, user GS, and selected-task PKRU match at every transition |
| high | feature isolation | generic `test-hooks` and production exclude the terminal fixture; only `x86-idt-cpl3-test` includes it |

## Historical CPL0 Success Criteria

- [x] `bash scripts/build-x86_64-idt-test-ci.sh` creates only distinct test kernel/ISO outputs.
- [x] `BOOT_WINDOW=90 bash scripts/qemu-x86_64-idt-test.sh` passes only with status33 and one exact `gprs=15 df=ok align=ok` marker.
- [x] Entry and post-iret captures equal all 15 exact sentinels for both probes; saved/live/restored DF states are 1/0/1 and assembly clears DF before return.
- [x] Alignment shim observes pre-call RSP multiple of16 and linked disassembly confirms the `and`/`call` sequence.
- [x] Negative marker/status/register/DF/alignment oracle checks each fail when deliberately perturbed, and perturbations are reverted.
- [x] Production symbol/dependency inspection shows no probe recovery, capture, or debug-exit path.

## Late CPL3 Success Criteria

- [x] PKU capability and CR4.PKE are mandatory; unsupported hardware fails.
- [x] The seven-state Ring-3 order and exactly one task-switching timer are observed.
- [x] GS/KGS, kernel/user PKRU, frame privilege words, syscall RIP/RDX, and
  task-selected PKRU values match at every named boundary.
- [x] Status/marker/forbidden-output and production/isolation gates pass.

## Late Safety Gate Evidence (2026-09-02)

- `[PASS]` Both required `init_timers` CPL0 `sti; hlt` iterations were
  acknowledged by the explicit two-callback state/count machine. The second
  callback emitted the retained CPL0 marker and masked the LAPIC timer before
  scheduler handoff.
- `[PASS]` QEMU `qemu64,+pku` completed the exact seven-state order
  `B_PARKED → A_FRESH → A_INT80_RETURNED → A_TIMER_ENTERED → B_SYSCALL_RETURNED → A_TIMER_RETURNED → COMPLETE`.
- `[PASS]` Every CPL3-origin frame proved CS=0x23, SS=0x1b, exact old RSP,
  kernel GS/KGS=`&CPU_LOCAL/0`, kernel PKRU=0, and selected-task PKRU.
- `[PASS]` B returned from its suspended nonzero syscall at the exact RIP with
  nonzero RDX preserved; A returned from its suspended timer with PKRU(A).
- `[PASS]` The runner enforced underlying debug-exit status 33, exactly one
  `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
  marker, exactly one
  `X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544`
  marker, two pre-scheduler timer wakeups, one scheduler initialization, and
  zero FAIL/PANIC/FAULT/SKIP/RESET/TRIPLE-FAULT matches.
- `[PASS]` Generic `test-hooks` and production excluded all 18 fixture symbols,
  six fixture namespaces, both markers, the test dispatch shim, and the
  terminal task call. Only `x86-idt-cpl3-test` included the fixture and HAL
  `qemu-exit` edge.

## Historical CPL0-Only Completion Evidence (superseded)

- The isolated build completed under `target/x86-idt-test` and
  `build/x86-idt-test`, separate from production outputs. Its QEMU run exited
  with debug status 33, printed the exact PASS marker once, and printed no
  FAIL/PANIC/FAULT marker.
- Real #BP and #GP entries validated all 15 entry and post-`iretq` GPR values,
  normalized vector/error (`3/0` and `13/0xfffc`), and saved/live/restored DF
  states of 1/0/1. The first real vector-32 timer entry completed only after
  EOI and callback return.
- The alignment shim and linked common-entry disassembly independently proved
  pre-call SysV alignment. Before the bootstrap correction, that strict check
  exited 1 with one FAIL marker and no PASS marker; after the correction the
  complete #BP/#GP/timer oracle reached status 33.
- Production ELF inspection found no probe symbols or self-test marker. The
  independent stress review confirmed that timeout, reset, or triple fault
  cannot satisfy the combined status-plus-marker oracle.

## Security Considerations

The recovery bypass and register captures are test-only and cannot be armed from user mode. Exact state/vector/error checks precede RIP mutation. The probe must clear DF and restore its caller’s callee-saved registers on every successful path; failure exits instead of returning corrupted state.

## Risk Notes

A malformed recovery address can recurse into #GP; a missed `cld` can corrupt later Rust memory operations. Assembly owns trigger and immediate post-iret capture so compiler register allocation cannot fake restoration. The serial+status oracle distinguishes success from timeout/reset and never accepts text alone.

## Deviation Log

- **Surprise (2026-09-02):** Adding the required optional `qemu-exit` edge to
  `hal-x86` also requires Cargo to refresh the workspace lockfile's `hal-x86`
  dependency list. `Cargo.lock` is outside phase ownership and the delegated
  source/test/script edit boundary, so it remains untouched and was escalated
  to the integration owner for the centralized validation pass.
- **Resolution (2026-09-02):** Centralized validation refreshed and audited
  `Cargo.lock`. Its only plan-related delta is the expected `qemu-exit` edge in
  `hal-x86`; `qemu-exit` 4.0.0 was already present through the kernel.
- **Resolved blocker (2026-09-02):** The earlier dedicated image proved only
  CPL0 #BP/#GP/timer entry and therefore missed CPL3 SWAPGS/PKRU and
  syscall/fresh-exit defects. The mandatory two-task Ring-3 oracle now covers
  those transitions and passed.
- **Decision (2026-09-02):** The replacement oracle uses two real Ring-3 tasks,
  scheduler FIFO from an otherwise empty test scheduler, and a single LAPIC
  one-shot. Missing PKU is a terminal FAIL, never a SKIP.
- **Deviation (2026-09-02):** Minimal test-only integration touches the
  pre-existing oversized `kernel/src/task.rs` and `kernel/src/task/syscall.rs`;
  new CPL3 fixture modules remain below 200 lines. A cfg-only PKU log branch is
  also necessary so the insecure test activation is deferred explicitly
  without emitting a forbidden SKIP line. No validation is claimed here.
- **Source blocker (2026-09-02):** The first late-gate run timed out after one
  `init_timers` wakeup because the first CPL0 callback stopped the periodic
  LAPIC before the required second `sti; hlt`; the CPL3 fixture was never
  reached. This is failed evidence, not a partial pass.
- **Resolution verified (2026-09-02):** CPL0 timer handoff requires explicit
  `first EOI → first callback → second EOI → second callback` state plus
  count=2 and stops the timer only in the second callback. The final runner
  observed both wakeups before one scheduler initialization.
- **Size correction verified (2026-09-02):** Capability/image and live GS/PKRU
  helpers moved to `cpl3_platform.rs`; every focused module/script is below
  200 lines, with `idt/probe.rs` largest at 182.
- **Source blocker (2026-09-02):** Reverification reached CPL3 fixture setup
  after both timer wakeups, then faulted while zeroing a newly allocated
  physical code frame through its unmapped physical address.
- **Resolution verified (2026-09-02):** Probe code initialization writes
  through `phys_to_virt(frame)` following the loader/address-space HHDM
  pattern while retaining physical-frame ownership and USER+EXEC publication.
  The final real-CPL3 oracle passed.
- **Source blocker (2026-09-02):** Final review found the terminal Ring-3
  fixture was gated only by broad `test-hooks`, so every generic x86 test
  image would run it instead of its intended workload.
- **Resolution verified (2026-09-02):** The dedicated
  `x86-idt-cpl3-test` feature owns every HAL probe/shim, task fixture
  call/intercept, forced-PKU log branch, and HAL `qemu-exit` dependency. It
  depends on `test-hooks` and is enabled only by the isolated build script.
  Generic `test-hooks` retained its pre-existing hooks but excluded the
  terminal fixture; production excluded both fixture markers and all fixture
  symbols. Final verification and both final reviews passed.
