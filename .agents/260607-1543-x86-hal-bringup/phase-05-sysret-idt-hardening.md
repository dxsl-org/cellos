# Phase 05 — SYSRET Security Fix + IDT Hardening

**Status:** TODO  
**Priority:** High — must complete before any ring-3 code runs on x86_64  
**Estimated effort:** Small (~30 lines across 2 files)

---

## Context Links

- `hal/arch/x86/src/x86_64/syscall.rs` — `syscall_entry` asm stub + `sysretq`
- `hal/arch/x86/src/x86_64/idt.rs` — 256-vector IDT, all handlers generic
- Research: QEMU report §B8 (SYSRET preconditions, CVE-2012-0217), §B10 (swapgs + NMI)

---

## Overview

### SYSRET bug (CVE-2012-0217)

Intel CPUs raise `#GP(0)` at CPL=0 when `SYSRET` is executed with a non-canonical `RCX`
(return RIP). At that point `GS` is already swapped to the user value — the `#GP` handler
runs with a user-controlled `%gs` pointer → arbitrary kernel read/write.

QEMU `-cpu qemu64` emulates Intel behavior. This must be mitigated.

**Fix**: Before `sysretq`, check `RCX` for canonical form. Non-canonical = kill the task
with a simulated `#GP` (or just return -EFAULT; the task can't recover from it anyway).

Canonical check: all bits 63:47 must be the same value (all 0 for user VA below 128 TB).
For QEMU user space (cells run below 0x0000_8000_0000_0000), the check is:
```asm
; RCX holds user return RIP
mov rax, rcx
shr rax, 47
test rax, rax           ; canonical if bits 63:47 all zero (user VA)
jnz .kill_task          ; non-canonical → abort, don't sysretq
```

For a SAS kernel where user code lives in the lower half, this is sufficient. A more
general check for signed-extended upper-half addresses would use `sar rax, 47; add rax, 1; cmp rax, 1`.

### IDT: exception handlers

Current `idt.rs` uses a single generic handler for all 256 vectors. For useful crash
diagnostics, the following exception vectors need handlers that print the vector + error code:
- Vector 8: `#DF` (Double Fault) — install on IST2 to avoid stack overflow during fault
- Vector 13: `#GP` — print "General Protection Fault, err=%lx"
- Vector 14: `#PF` — print "Page Fault, cr2=%lx, err=%lx"

These don't need full stack traces for bring-up — just serial output before halting.

### NMI/swapgs hazard

The `swapgs` + NMI hazard (NMI arriving between SYSCALL CS-load and the `swapgs` in
`syscall_entry`) is a real but narrow race. For bring-up, no mitigation is needed:
- QEMU TCG is deterministic (no async NMIs during this 1-cycle window)
- NMI handler for bring-up can just halt
- Note in code: `// TODO: add IST+RDMSR paranoid NMI path before adversarial testing`

---

## Requirements

- `sysretq` never executes with non-canonical RCX
- `#GP` and `#PF` handlers print diagnostics to COM1 before halting
- `#DF` handler uses IST (separate stack) to avoid double fault → triple fault cascade

---

## Architecture

### SYSRET canonicality check (inline in `syscall_entry` asm)

```asm
; ... (after restoring user RSP, before sysretq)
; Check RCX canonical: shift right 47 bits; result must be 0 (user lower-half addr)
mov rax, rcx
shr rax, 47
jnz sysret_noncanonical    ; non-canonical → cannot sysretq safely
swapgs
sysretq

sysret_noncanonical:
    ; Restore kernel state, synthesize SIGSEGV equivalent
    ; For bring-up: just hlt + jmp loop
    hlt
    jmp sysret_noncanonical
```

### Exception handler macros in `idt.rs`

```rust
fn exception_handler_gp(frame: &InterruptStackFrame, error_code: u64) {
    // Print to COM1: "EXCEPTION #GP error=0x..."
    uart_16550::puts("#GP fault\n");
    loop { unsafe { core::arch::asm!("hlt", options(nomem, nostack)); } }
}

fn exception_handler_pf(frame: &InterruptStackFrame, error_code: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2); }
    uart_16550::puts("#PF fault\n");
    loop { unsafe { core::arch::asm!("hlt", options(nomem, nostack)); } }
}
```

### IST for `#DF`

`gdt.rs` already has a TSS. Add IST2 entry (8 bytes at offset 36 in TSS) pointing to a
dedicated 4 KB fault stack allocated in `.bss`. Set `IDT[8].ist = 2`.

---

## Related Code Files

| Action | File |
|--------|------|
| Modify | `hal/arch/x86/src/x86_64/syscall.rs` — canonicality check before sysretq |
| Modify | `hal/arch/x86/src/x86_64/idt.rs` — #GP, #PF, #DF handlers + IST for DF |
| Modify | `hal/arch/x86/src/x86_64/gdt.rs` — IST2 stack pointer in TSS |

---

## Implementation Steps

1. **`syscall.rs`** — in the `syscall_entry` asm block, immediately before `swapgs; sysretq`:
   - Add `mov rax, rcx; shr rax, 47; jnz <label>` canonicality check
   - Add `<label>: hlt; jmp <label>` for non-canonical case (bring-up: halt)

2. **`idt.rs`** — replace the generic handler for vectors 13 and 14 with named handlers
   that print diagnostic info; add IST=2 to vector 8 gate

3. **`gdt.rs`** — add `IST2_STACK: [u8; 4096]` static array, set `tss.ist[1]` to
   `IST2_STACK.as_ptr().add(4096) as u64`

4. `cargo check -p hal-x86 --target x86_64-unknown-none`

---

## Success Criteria

- Sysretq path includes canonicality check (visible in `objdump -d`)
- `#PF` and `#GP` handlers print to COM1 before halting
- `#DF` uses IST2 (observable in GDT/TSS IST slot)

---

## Risk Assessment

- **LOW** — the canonicality check is 3 instructions; risk is register clobber (use RAX
  which is caller-saved / already clobbered by syscall return value setup)
- **LOW** — IST requires GDT and TSS to be set up (Phase 01 already ensures GDT runs first)
- NMI hazard: deliberately deferred; annotated in code

---

## Security Considerations

- **CVE-2012-0217 mitigated**: non-canonical RCX → halt (bring-up) or task kill (G2)
- SMEP/SMAP: `qemu64` does not advertise these bits in CPUID; kernel does not set CR4.SMEP.
  Note in `paging.rs::activate()`: `// TODO: set CR4.SMEP when cell page-bit policy is decided`
