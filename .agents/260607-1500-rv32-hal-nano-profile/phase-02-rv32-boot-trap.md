# Phase 02 — RV32 Boot + Trap Handler

**Status**: 📋 PLANNED
**Priority**: P3
**Effort**: 2 days
**Parallel**: Yes — independent of Phase 01 and Phase 03

---

## Context Links

- RV64 boot reference: `hal/arch/riscv/src/rv64/boot.rs`
- RV64 trap reference: `hal/arch/riscv/src/rv64/trap.rs`, `rv64/asm/trap.S`
- Target module: `hal/arch/riscv/src/rv32.rs` (existing skeleton)

---

## Overview

Creates the two remaining assembly sub-modules for RV32:
- `rv32/boot.rs` — non-PIE `_start` entry point for QEMU RV32 virt + OpenSBI
- `rv32/trap.rs` + `rv32/asm/trap.S` — 32-bit trap frame + S-mode trap handler
- Replaces the placeholder `trap.S` stub created in Phase 01

The boot code is SIMPLER than RV64 because:
1. Non-PIE: no self-relocation loop (no `.rela.dyn`)
2. 32-bit: BSS clear uses `sw zero` (4-byte stores)
3. Same ORIGIN = 0x80200000 (QEMU RV32 virt + OpenSBI jump address)

---

## Key Insights

**Boot:**
- OpenSBI ABI: hartid in `a0`, DTB pointer in `a1` — must NOT clobber before `kmain` call
- Stack setup: `lla sp, __stack_top` (PC-relative, avoids GOT)
- No `.rela.dyn` section — non-PIE binary, link-time addresses are final
- BSS clear uses 4-byte `sw zero` instead of 8-byte `sd zero`
- `gp` relaxation: `lla gp, __global_pointer$` must still happen before any `lui`/`auipc` with GP-relative accesses

**Trap:**
- RV32 scause: interrupt bit = bit 31 (NOT bit 63 like RV64)
- `ViTrapFrame32`: 32 regs × 4B + 4 CSRs × 4B = 144 bytes (vs 288 bytes on RV64)
- sstatus SIE bit = bit 1 on both RV32 and RV64 — same trap enable/disable logic
- `vi_trap_handler` receives `&mut ViTrapFrame32` — same extern "Rust" linkage as RV64
- PLIC claim/complete via `crate::common::plic` — works for both arches (u32 pointers)
- The existing `vi_trap_handler` in `rv64/trap.rs` masks interrupt bit with `0x7FFF_FFFF_FFFF_FFFF` — for RV32, use `0x7FFF_FFFF`

---

## Architecture

```
_start (rv32/boot.rs global_asm!)
    disable interrupts (csrw sie, zero)
    init gp, clear tp
    set sp = __stack_top
    clear BSS (sw zero, 4-byte stride)
    call kmain

trap entry (rv32/asm/trap.S __trap_entry32)
    save all 32 regs + CSRs to stack (144 bytes)
    call vi_trap_handler32(&mut ViTrapFrame32)
    restore regs + sret

rv32/trap.rs
    pub struct ViTrapFrame32 { regs: [u32; 32], sstatus: u32, sepc: u32, stval: u32, scause: u32 }
    pub fn init() → csrw stvec, __trap_entry32; csrw sscratch, zero
    pub extern "C" fn vi_trap_handler32(frame: &mut ViTrapFrame32)
```

---

## Related Code Files

| File | Action |
|------|--------|
| `hal/arch/riscv/src/rv32/boot.rs` | CREATE — non-PIE `_start` |
| `hal/arch/riscv/src/rv32/trap.rs` | CREATE — `ViTrapFrame32`, `init()`, `vi_trap_handler32` |
| `hal/arch/riscv/src/rv32/asm/trap.S` | CREATE — replaces Phase 01 placeholder |
| `hal/arch/riscv/src/rv32.rs` | MODIFY — wire `init()` to `trap::init()`, remove stub comments |

---

## Implementation Steps

1. **Create `rv32/boot.rs`**:
   ```rust
   use core::arch::global_asm;

   global_asm!(r#"
       .section .text.boot
       .global _start
   _start:
       csrw sie, zero
       csrw sip, zero

       .option push
       .option norelax
       lla gp, __global_pointer$
       .option pop

       mv tp, zero
       lla sp, __stack_top

       # Clear BSS
       lla t0, __bss_start
       lla t1, __bss_end
   1:
       bgeu t0, t1, 2f
       sw zero, 0(t0)
       addi t0, t0, 4
       j 1b
   2:
       call kmain

   3:
       wfi
       j 3b
   "#);
   ```

2. **Create `rv32/asm/trap.S`** (replaces the placeholder from Phase 01):
   ```asm
   .section .text
   .global __trap_entry32
   .align 2

   # ViTrapFrame32 layout (144 bytes):
   #   0-127: x0-x31 (4 bytes each, x0 slot kept for alignment)
   # 128: sstatus, 132: sepc, 136: stval, 140: scause

   __trap_entry32:
       addi sp, sp, -144
       sw   x0,   0(sp)
       sw   x1,   4(sp)
       # (x2/sp saved below after offset stabilizes)
       sw   x3,  12(sp)
       sw   x4,  16(sp)
       sw   x5,  20(sp)
       sw   x6,  24(sp)
       sw   x7,  28(sp)
       sw   x8,  32(sp)
       sw   x9,  36(sp)
       sw  x10,  40(sp)
       sw  x11,  44(sp)
       sw  x12,  48(sp)
       sw  x13,  52(sp)
       sw  x14,  56(sp)
       sw  x15,  60(sp)
       sw  x16,  64(sp)
       sw  x17,  68(sp)
       sw  x18,  72(sp)
       sw  x19,  76(sp)
       sw  x20,  80(sp)
       sw  x21,  84(sp)
       sw  x22,  88(sp)
       sw  x23,  92(sp)
       sw  x24,  96(sp)
       sw  x25, 100(sp)
       sw  x26, 104(sp)
       sw  x27, 108(sp)
       sw  x28, 112(sp)
       sw  x29, 116(sp)
       sw  x30, 120(sp)
       sw  x31, 124(sp)
       # Save original sp (sp + 144)
       addi t0, sp, 144
       sw   t0,   8(sp)
       # Save CSRs
       csrr t0, sstatus
       sw   t0, 128(sp)
       csrr t0, sepc
       sw   t0, 132(sp)
       csrr t0, stval
       sw   t0, 136(sp)
       csrr t0, scause
       sw   t0, 140(sp)
       # Call Rust handler
       mv   a0, sp
       call vi_trap_handler32
       # Restore CSRs
       lw   t0, 128(sp)
       csrw sstatus, t0
       lw   t0, 132(sp)
       csrw sepc, t0
       # Restore registers (skip x0)
       lw   x1,   4(sp)
       lw   x3,  12(sp)
       lw   x4,  16(sp)
       lw   x5,  20(sp)
       lw   x6,  24(sp)
       lw   x7,  28(sp)
       lw   x8,  32(sp)
       lw   x9,  36(sp)
       lw  x10,  40(sp)
       lw  x11,  44(sp)
       lw  x12,  48(sp)
       lw  x13,  52(sp)
       lw  x14,  56(sp)
       lw  x15,  60(sp)
       lw  x16,  64(sp)
       lw  x17,  68(sp)
       lw  x18,  72(sp)
       lw  x19,  76(sp)
       lw  x20,  80(sp)
       lw  x21,  84(sp)
       lw  x22,  88(sp)
       lw  x23,  92(sp)
       lw  x24,  96(sp)
       lw  x25, 100(sp)
       lw  x26, 104(sp)
       lw  x27, 108(sp)
       lw  x28, 112(sp)
       lw  x29, 116(sp)
       lw  x30, 120(sp)
       lw  x31, 124(sp)
       lw   sp,   8(sp)
       sret
   ```

3. **Create `rv32/trap.rs`**:
   ```rust
   #[derive(Debug, Clone, Copy, Default)]
   #[repr(C)]
   pub struct ViTrapFrame32 {
       pub regs: [u32; 32],
       pub sstatus: u32,
       pub sepc: u32,
       pub stval: u32,
       pub scause: u32,
   }

   const _: () = assert!(core::mem::size_of::<ViTrapFrame32>() == 144);

   extern "C" { fn __trap_entry32(); }

   pub fn init() {
       unsafe {
           let entry = __trap_entry32 as usize;
           core::arch::asm!("csrw stvec, {}", in(reg) entry);
           core::arch::asm!("csrw sscratch, zero");
       }
   }

   #[no_mangle]
   pub extern "C" fn vi_trap_handler32(frame: &mut ViTrapFrame32) {
       let scause = frame.scause;
       let is_interrupt = (scause >> 31) != 0;          // bit 31 on RV32, not bit 63
       let code = scause & 0x7FFF_FFFF;

       if is_interrupt {
           match code {
               1 => unsafe {                              // SSIP: zero-latency RT preemption
                   core::arch::asm!("csrci sip, 0x2");
                   vi_timer_tick();
               },
               5 => unsafe { vi_timer_tick(); },          // STIP: timer tick
               _ => {}
           }
       } else {
           match code {
               8 | 9 => {
                   vi_handle_syscall(frame);
                   frame.sepc += 4;
               },
               _ => {
                   let cell_id = unsafe { vi_current_cell_id() };
                   if cell_id != 0 {
                       unsafe { vi_terminate_on_fault(code as usize, frame.sepc as usize); }
                   } else {
                       panic!("ViCell: Kernel exception: scause={} sepc={:#x}", code, frame.sepc);
                   }
               }
           }
       }
   }

   fn vi_handle_syscall(frame: &mut ViTrapFrame32) {
       extern "Rust" { fn ViCell_syscall_dispatch32(frame: &mut ViTrapFrame32); }
       unsafe { ViCell_syscall_dispatch32(frame); }
   }

   extern "Rust" {
       fn vi_timer_tick();
       fn vi_terminate_on_fault(scause: usize, sepc: usize);
       fn vi_current_cell_id() -> usize;
   }
   ```
   Note: `ViCell_syscall_dispatch32` needs a stub in the kernel for Phase 04. PLIC is omitted
   for RV32 Nano (no external interrupt claim on QEMU virt in Nano mode).

4. **Update `rv32.rs` `Arch::init()`**:
   ```rust
   fn init(&self) {
       trap::init();
       // Enable S-mode software interrupt (SSIE) for RT zero-latency preemption.
       // SAFETY: csrsi on sie bit 1 is valid in S-mode.
       unsafe { core::arch::asm!("csrsi sie, 0x2"); }
   }
   ```
   Remove the TODO comment about trap::init().

5. **Remove stub module placeholders** (if Phase 01 created empty `trap` and `boot` stubs in rv32.rs, replace them with real declarations):
   ```rust
   pub mod boot;     // rv32/boot.rs — assembly _start
   pub mod context;  // rv32/context.rs — Rv32Context::switch (Phase 01)
   pub mod trap;     // rv32/trap.rs — ViTrapFrame32 + vi_trap_handler32
   mod asm;          // rv32/asm.rs — global_asm! includes
   ```

6. **Run compile check** (after Phase 01 is merged or in parallel branch):
   ```
   cargo check -p hal-riscv --target riscv32imac-unknown-none-elf
   ```

---

## Todo List

- [ ] Create `rv32/boot.rs` with non-PIE `_start`
- [ ] Create `rv32/asm/trap.S` (replaces Phase 01 placeholder)
- [ ] Create `rv32/trap.rs` with `ViTrapFrame32`, `init()`, `vi_trap_handler32`
- [ ] Update `rv32.rs` `init()` to call `trap::init()` + enable SSIE
- [ ] Update module declarations in `rv32.rs`
- [ ] Verify `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` passes
- [ ] Verify `cargo check -p hal-riscv --target riscv64gc-unknown-none-elf` still passes

---

## Success Criteria

- `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` passes
- `ViTrapFrame32` size assert: `size_of::<ViTrapFrame32>() == 144`
- `init()` calls `trap::init()` (no TODO comment remaining)
- `boot.rs` provides a `_start` symbol in `.text.boot`

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `vi_trap_handler32` needs extern "Rust" symbols from kernel | Kernel provides stubs in Phase 04; `cargo check` passes without them (weak linkage check) |
| Interrupt bit position differs (bit 31 vs 63) | Explicitly documented; code uses `>> 31` not `>> 63` |
| RV32 has no x2 save before sp is decremented | Save original sp = `sp + 144` after frame allocation (step 2 of trap.S) |
