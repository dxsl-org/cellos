# Scout Report: x86_64 Per-Vector IDT

## Scope and Evidence

Recon source: `agent://ScoutX86IdtContracts`, spot-checked against the cited repository files on 2026-09-02. This report covers only host-kernel x86_64 IDT entry, exception/IRQ dispatch, and the minimum x86 test image/runner support. It excludes SYSCALL/SYSRET changes, TSS/IST stacks, emulator pin/parity work, VMM guests, and hardware claims.

## Current Baseline

- `hal/arch/x86/src/x86_64/idt.rs:73-104` installs one no-error handler broadly, a shared error handler for only 8, 10–14, 17, a separate #CP handler, and special timer/UART handlers. Vector 0x80 alone gets DPL3.
- `idt.rs:139-145` cannot classify the generic vector and unconditionally EOIs. `idt.rs:187-204` reads CR2 and calls `vi_handle_page_fault` for every event reaching the generic error handler.
- `idt.rs:16-24` models RIP/CS/RFLAGS/RSP/SS as unconditional, although same-CPL, IST0 entry contains only RIP/CS/RFLAGS after the normalized words.
- `kernel/src/memory/paging.rs:1150-1193` confirms the page-fault ABI and documents the stale-CR2/#GP defect. The hook needs a real interrupted RSP for its kernel diagnostic scan.
- `hal/traits/arch/src/kernel_abi.rs:117-124` exposes `vi_terminate_on_user_trap_fault(cause,pc,fault_addr)`; `kernel/src/task.rs:691-703` restricts it to trap-proven user faults. Attributable Ring-3 exceptions should retire the current Cell instead of halting the host kernel.
- `kernel/src/task.rs:794-830` permits the timer callback to context-switch, so LAPIC EOI must precede it. `kernel/src/task/drivers/uart.rs:231-278` drains UART data; its EOI remains after the callback.
- `kernel/src/main.rs:183-194` loads GDT/IDT before LAPIC MMIO is ready. `hal/arch/x86/src/x86_64/apic.rs:102-105` makes an early or exception-path EOI unsafe, while `apic.rs:94-99` explicitly programs 0xff as the LAPIC spurious vector.
- `hal/arch/x86/src/x86_64.rs:112-188` already performs a real ring-0 LAPIC timer `sti; hlt; cli` smoke after LAPIC setup.
- `hal/arch/x86/src/x86_64/syscall.rs:157-296` and `trap.rs:1-53` are independent context/syscall ABIs. They must not share or be rewritten around the IDT entry record.
- `hal/arch/x86/src/x86_64/gdt.rs:22-42,95-127` and `idt.rs:38-47` show IST=0 with no provisioned IST stacks. This plan keeps it that way.

## Chosen Generation and Addressing

Add a host `hal/arch/x86/build.rs` under 200 lines. One constant set `{8,10,11,12,13,14,17,21,29,30}` drives a loop that writes an `OUT_DIR` assembly file containing 256 named stubs and a 256-entry `.quad` address table; it also writes a generated Rust error-vector constant for host assertions. `idt.rs` indexes the address table directly, avoiding 256 checked-in declarations, handwritten address lists, label concatenation tricks, and fixed-instruction-stride assumptions.

`entry.rs` imports the generated file with:

`global_asm!(include_str!(concat!(env!("OUT_DIR"), "/x86_64_idt_stubs.S")), options(att_syntax));`

A disposable `/tmp` spike compiled and linked this exact `OUT_DIR` + `include_str!` form with `rustc --edition=2021`; a second spike assembled a 256-stub `.rept` fixture and verified a 4096-byte table span. The build-script/table form is selected because explicit relocatable addresses are simpler and do not make correctness depend on a 16-byte instruction stride. No spike file touched the repository.

Generated stubs are exactly:

- CPU-error vector: `pushq $vector; jmp x86_64_idt_common`.
- Other vector: `pushq $0; pushq $vector; jmp x86_64_idt_common`.

Thus the common entry always begins with `[vector,error,RIP,CS,RFLAGS,(old RSP,old SS only after CPL change)]`.

## Exact Saved Record

The common entry executes `cld`, then pushes in this order:

`RAX, RBX, RCX, RDX, RBP, RSI, RDI, R8, R9, R10, R11, R12, R13, R14, R15`.

At the pointer passed in RDI, offsets are fixed:

| Offset | Field | Offset | Field |
|---:|---|---:|---|
| 0 | R15 | 8 | R14 |
| 16 | R13 | 24 | R12 |
| 32 | R11 | 40 | R10 |
| 48 | R9 | 56 | R8 |
| 64 | RDI | 72 | RSI |
| 80 | RBP | 88 | RDX |
| 96 | RCX | 104 | RBX |
| 112 | RAX | 120 | vector |
| 128 | error | 136 | RIP |
| 144 | CS | 152 | RFLAGS |
| 160 | optional old RSP | 168 | optional old SS |

The fixed Rust record ends at byte 160 and never declares the optional words as fields. `has_privilege_frame = (CS & 3) != 0`; only then may accessors read offsets 160/168. `interrupted_rsp()` returns the saved old RSP after a CPL change, otherwise the address `record_base + 160`, which is the same-CPL pre-interrupt RSP and is safe for the existing PF diagnostic ABI.

After saves, R12 holds the exact record pointer; RSP is rounded down with `andq $-16,%rsp` before the SysV call. The call return address occupies scratch space below the record. The callee preserves R12; common entry restores `rsp=r12`, pops `R15..RAX`, adds 16 for vector/error, and executes `iretq`. This saves every mutable GPR; RSP is preserved by exact unwind/iret rather than a destructive pop. No red zone or static alignment assumption is used.

## Dispatch Contract

Pure classification consumes both vector and saved-CS origin and returns a route plus EOI phase:

| Vectors/origin | Route | EOI |
|---|---|---|
| 14, any CPL | Read CR2, call PF hook with error/RIP/CS/effective RSP; may return | none |
| Ring 3, 0–31 except 2/8/14/18 | `vi_terminate_on_user_trap_fault(vector,RIP,0)`; return if hook returns | none |
| Ring 0 exception, or any-CPL NMI/#DF/#MC | bounded diagnostic, then `cli; hlt` | none |
| 0x20 | LAPIC EOI, then timer callback | before callback |
| 0x24 | UART drain callback, then LAPIC EOI | after callback |
| 0x80 | explicit tolerated legacy no-op return; not a syscall | none |
| 0xff | explicit LAPIC-spurious return, no callback | none |
| all remaining vectors | bounded diagnostic, then `cli; hlt` | none |

#GP (13), #CP (21), and every non-PF exception never read CR2 and never call the PF hook. A trap-proven Ring-3 #GP/#CP uses the existing user-retirement hook with a zero fault address; kernel faults and non-attributable NMI/#DF/#MC remain fatal. Unknown vectors remain fatal, but the configured 0xff LAPIC spurious vector returns without EOI. Gate selector 0x08, interrupt-gate attributes, DPL0 default, DPL3 only for 0x80, and IST0 remain unchanged.

## Proof Design and Oracle

1. Host pure tests assert Ring-0/Ring-3 route differences, non-attributable NMI/#DF/#MC handling, timer/UART EOI phases, the explicit 0xff spurious return, and the exact ten-vector generated error set. They do not execute privileged instructions.
2. Under `hal-x86/test-hooks`, assembly-owned BP and #GP probes preserve their Rust caller’s ABI, load distinct sentinels into all 15 saved GPRs, execute `std`, then enter through real `int3` and invalid-DS #GP frames. The dispatcher asserts every record slot, saved DF=1, live Rust DF=0, normalized metadata, and (for #GP) error 0xfffc before narrowly rewriting RIP. Immediately after `iretq`, assembly stores all 15 restored registers and RFLAGS, proves DF was restored, executes `cld`, restores its caller’s callee-saved state, and returns for comparison.
3. A test-only assembly dispatch shim records its entry RSP so the probe asserts `(shim_rsp + 8) & 15 == 0`, proving the common entry aligned RSP before `call`; linked disassembly must independently show `andq $-16,%rsp` with no stack-changing instruction before that call.
4. The first real LAPIC timer entry after the existing `sti; hlt; cli` path records post-EOI state, calls the timer hook, records callback return, prints exactly `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`, and exits QEMU.
5. A dedicated runner adds `-device isa-debug-exit,iobase=0xf4,iosize=0x04`. With `qemu-exit` 4.0.0 configured with odd success code 33, success requires both process status 33 and the exact marker; status 1, timeout 124, panic/fault text, missing/duplicate marker, or any other status fails.
6. The production ISO is rebuilt without `test-hooks` and must still reach `Cellos >`; the focused host serial integration then proves UART input remains live.

## Build and File Conventions

- Existing `global_asm!` conventions are in `boot.rs:78-132` and `syscall.rs:157-200`; AT&T syntax must be declared explicitly.
- `kernel/Cargo.toml:112-118` exposes `test-hooks`, but `hal/core/Cargo.toml:43` currently forwards it only to RISC-V. Add optional x86 propagation and an optional `qemu-exit` dependency in `hal-x86`; production defaults stay empty.
- `scripts/x86/make-iso-ci.sh:17-21` hard-codes kernel/root paths. Add backward-compatible environment overrides so the isolated test target cannot overwrite production output.
- `scripts/qemu-x86_64-test.sh:39-71` is the production smoke and remains free of debug-exit semantics. Add a dedicated test-hook runner instead.
- All new checked-in Rust/shell/build files remain below 200 lines. Rewrite `idt.rs` below 200 by splitting `idt/{entry,policy,dispatch,probe,probe_entry}.rs`; only necessary comments may change in pre-existing larger files.

## Risks and Assumptions

- The real #GP recovery is QEMU-lane proof, not a claim about arbitrary physical CPUs. It is feature-gated and never shipped.
- The dispatcher must not borrow optional RSP/SS before checking CS RPL; `repr(C)` size/offset assertions and actual-entry captures guard this.
- Returning from unarmed exceptions is prohibited. A mismatch must halt/exit, preventing recursive faults and triple-fault ambiguity.
- The probe must clear DF before any Rust return and restore its caller’s callee-saved registers; otherwise the test harness itself corrupts the kernel.
- GPR preservation does not add SIMD/FPU saves; current kernel target conventions avoid interrupt-time SIMD. Verify target flags before Build and log a deviation if that assumption is false.
- Existing long kernel files are not split as part of this focused change; expanding that refactor would be unrelated risk.

## Precedent

Git reflog precedent reported by recon: `46937ed4c7132228...` centralized kernel ABI hooks; `b3dad2fe97be...` established fresh serial-output integration behavior. No IDT-specific precedent was found.
