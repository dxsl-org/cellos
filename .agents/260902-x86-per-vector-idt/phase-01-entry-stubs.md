---
phase: 1
title: "Generate and Install Exact Entry Stubs"
status: complete
priority: P1
effort: "6h"
dependencies: []
tier: thinking
---

# Phase 1: Generate and Install Exact Entry Stubs

> **Required — deviation-log:** Log every Decision / Deviation / Surprise below when it occurs. Choose the smallest reversible response; escalate any entry-ABI change.

## Overview

Replace the oversized mixed-handler IDT with a generated 256-address table and one exact, reviewable common entry. This phase establishes mechanics only; Phase 2 supplies route behavior.

## Requirements

- Functional: every vector 0–255 has a distinct address; normalize `[vector,error,RIP,CS,RFLAGS,(RSP,SS on CPL change)]` using error vectors 8,10,11,12,13,14,17,21,29,30.
- Functional: install selector 0x08, IST0 interrupt gates with DPL0 except vector 0x80 DPL3; delete old `extern "x86-interrupt"` handlers and unused dynamic installer.
- Non-functional: every new/touched IDT implementation unit stays below 200 lines; generated 256-stub output lives only in `OUT_DIR`.
- Non-functional: preserve every GPR, clear DF for Rust, use dynamic SysV alignment, no red zone, and exact unwind to `iretq`.

## Architecture

`build.rs` owns the sole error-vector set and emits named stubs plus `[usize;256]` addresses. `idt.rs` indexes that table. `idt/entry.rs` defines the common assembly and a 160-byte `repr(C)` record.

Push order is `RAX,RBX,RCX,RDX,RBP,RSI,RDI,R8,R9,R10,R11,R12,R13,R14,R15`; record offsets are R15=0, R14=8, R13=16, R12=24, R11=32, R10=40, R9=48, R8=56, RDI=64, RSI=72, RBP=80, RDX=88, RCX=96, RBX=104, RAX=112, vector=120, error=128, RIP=136, CS=144, RFLAGS=152. Bytes 160/168 are optional old RSP/SS and are not Rust fields.

After pushes, save the record pointer in R12, `andq $-16,%rsp`, pass the unaligned record pointer in RDI, call the normal C ABI dispatcher, restore RSP from callee-saved R12, pop in reverse, `addq $16,%rsp`, then `iretq`. RSP itself is preserved by unwind/iret.

For saved CS.RPL3, hardware has not switched GS or PKRU. The completed late
safety correction therefore performs `swapgs` and selects kernel PKRU before
calling Rust, then masks interrupts, restores the selected task PKRU, restores
all saved GPRs, and swaps back to user GS immediately before `iretq`. CPL0
entry bypasses both transitions.

## Assumptions

- **Claim:** kernel x86 code does not use interrupt-time SIMD/FPU state.
  **Confidence:** medium
  **How to verify:** inspect x86 target rustflags and generated kernel code policy before editing; stop and redesign if SIMD is enabled.
- **Claim:** a build script may emit assembly consumed by `global_asm!(include_str!(concat!(env!("OUT_DIR"), ...)))`.
  **Confidence:** high
  **How to verify:** already compile-proved in a disposable `/tmp` rustc spike; reconfirm with the Phase success command.

## Related Files / Ownership

| File | Action | Owner | Size guard |
|---|---|---|---|
| `hal/arch/x86/build.rs` | Create generator | Phase 1 | <200 lines |
| `hal/arch/x86/src/x86_64/idt.rs` | Rewrite descriptor/install only | Phase 1 | <200 lines |
| `hal/arch/x86/src/x86_64/idt/entry.rs` | Create frame/common entry | Phase 1 | <200 lines |
| `hal/arch/x86/src/lib.rs` | Remove obsolete ABI feature/comment | Phase 1 | remains <200 |
| `hal/arch/x86/Cargo.toml` | Register build inputs only if Cargo requires it | Phase 1 | manifest |
| `hal/arch/x86/src/x86_64/boot.rs` | Correct tail-entry SysV stack phase | Phase 1 deviation | <200 lines |

## Implementation Steps

1. Before edits, record results of `cargo check -p cellos-kernel --target x86_64-unknown-none` and the production build/ISO/runner command; do not repair unrelated baseline failures.
2. Generate all 256 stubs and a relocatable address table from one loop/error set; emit `cargo:rerun-if-changed=build.rs` and generated Rust constants for assertions.
3. Define the fixed record and compile-time size/offset assertions; provide checked optional `old_rsp/old_ss` accessors and `interrupted_rsp = old_rsp.unwrap_or(record_base + 160)`.
4. Implement common entry exactly as Architecture states, including `cld`, all 15 pushes, dynamic alignment, reverse restore, normalized-word removal, and `iretq`.
5. Rewrite IDT initialization to use table addresses and retain only vector 0x80 DPL3; remove all direct Rust ABI handler addresses and `install_vector` after confirming no callers.
6. Remove `abi_x86_interrupt` feature use if no other x86 source needs it. Confirm each generated address is nonzero/distinct and table length is 256 in a host test or debug assertion.

## Success Criteria

- [x] Generated assembly has exactly 256 labels/table entries and the exact ten error vectors.
- [x] Record size/offset assertions match the table above; optional words are read only after `(cs & 3) != 0`.
- [x] `cargo check -p cellos-kernel --target x86_64-unknown-none` succeeds relative to baseline.
- [x] No new IDT code file exceeds 200 lines and no old `extern "x86-interrupt"` entry remains.

## Completion Evidence (2026-09-02)

- Two independent generator runs produced byte-identical output. Structural
  validation counted 256 ordered stubs and table entries, 256 `ENDBR64`
  entries, 246 synthetic-zero stubs, and the exact hardware-error set
  `[8,10,11,12,13,14,17,21,29,30]`.
- The linked release ELF contains a 2,048-byte table matching 256 distinct,
  ordered, 16-byte-aligned vector symbols. Common-entry disassembly shows all
  15 GPR pushes and reverse pops, `cld`, dynamic 16-byte call alignment,
  removal of exactly the vector and error words, and `iretq`.
- The x86-none check and release build passed. The final linked contract check
  also proved CPL0 bypass and saved-CS-conditional CPL3 SWAPGS/PKRU transitions.
  The final size inventory kept every focused implementation source and script
  below 200 lines; the largest was `idt/probe.rs` at 182 lines.

## Security Considerations

Preserve DPL exactly; a mistaken DPL3 exception gate is a privilege boundary failure. Do not assign IST, read beyond same-CPL frames, or return with a partially restored record. Generated file contents must be fixed data from integer iteration, never environment-controlled text.

## Risk Notes

Any push-order, alignment, or normalized-word mismatch can triple-fault. Treat generator, record, and common entry as one atomic ABI; do not land or roll back them separately.

## Deviation Log

- **Deviation (2026-09-02):** `hal/arch/x86/src/x86_64/boot.rs` was added to
  Phase 1 ownership at the integration owner's direction after the entry probe
  exposed its pre-existing SysV handoff mismatch. `_start` now reserves a zero
  synthetic bottom-frame slot after 16-byte alignment, so its JMP target enters
  with `RSP % 16 == 8`; the JMP and never-return contract remain unchanged.
- **Deviation (2026-09-02):** The plan artifacts do not retain a separate
  transcript for the requested pre-edit compile and production-boot capture.
  Closure therefore records the passing final x86-none check, the clean rebuilt
  production boot, and the clean diff check against baseline `0136c58e`; it
  does not infer an unobserved pre-edit command result.
- **Late safety deviation (2026-09-02):** The CPL0-only entry proof did not
  exercise the architectural fact that privilege-changing IDT entry leaves GS
  and PKRU untouched. Phase 3 added the saved-CS-conditional transitions above;
  final production and dedicated linked checks plus the real-CPL3 oracle
  verified the corrected common-entry contract.
