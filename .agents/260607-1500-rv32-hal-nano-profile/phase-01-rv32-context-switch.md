# Phase 01 — RV32 Context Switch

**Status**: 📋 PLANNED
**Priority**: P3
**Effort**: 2 days
**Parallel**: Yes — independent of Phase 02 and Phase 03

---

## Context Links

- Existing skeleton: `hal/arch/riscv/src/rv32.rs`
- RV64 reference: `hal/arch/riscv/src/rv64/context.rs`, `hal/arch/riscv/src/rv64/asm/switch.S`
- Module wiring: `hal/arch/riscv/src/lib.rs`

---

## Overview

The existing `rv32.rs` defines `Rv32Context` with only 14 fields (ra, sp, s0-s11) and
`switch_context()` is `unimplemented!()`. This phase completes the context switch:
- Extends `Rv32Context` to match the full set of callee-saved + CSR fields (same as RV64 `Context`)
- Creates `rv32/context.rs` with `Rv32Context::switch()` calling external `__switch32`
- Creates `rv32/asm/switch.S` with 32-bit `sw`/`lw` instructions
- Wires `switch_context()` in `rv32.rs` to call `Rv32Context::switch()`

---

## Key Insights

- RV64 `Context` has: ra, sp, s0-s11, sepc, sstatus, gp, tp, sscratch = 19 × 8B = 152 bytes
- `Rv32Context` must have the same 19 fields as u32 = 19 × 4B = 76 bytes
- Context switch is cooperative (called from kernel, not from trap) — `sepc`/`sstatus` must still be saved so that tasks pre-empted by a timer trap can be correctly resumed via `sret`
- `switch.S` for RV32: replace `sd`/`ld` (64-bit) with `sw`/`lw` (32-bit), offsets ×4 instead of ×8
- Assembly uses `global_asm!(include_str!("asm/switch.S"))` pattern from `rv64/asm.rs`
- Do NOT rename `__switch` → keep it `__switch32` to avoid linker conflicts when both arch crates are compiled in tests

---

## Architecture

```
rv32.rs (Arch impl)
  switch_context() → rv32/context.rs Rv32Context::switch()
                          → extern "C" __switch32 (rv32/asm/switch.S)
rv32.rs (module decls)
  mod asm        → rv32/asm.rs → global_asm!(switch.S, trap.S)
  pub mod context → rv32/context.rs
```

---

## Related Code Files

| File | Action |
|------|--------|
| `hal/arch/riscv/src/rv32.rs` | MODIFY — extend `Rv32Context` fields; add `mod asm`, `pub mod context`; wire `switch_context()` |
| `hal/arch/riscv/src/rv32/context.rs` | CREATE — `Rv32Context::switch()` |
| `hal/arch/riscv/src/rv32/asm.rs` | CREATE — `global_asm!` includes |
| `hal/arch/riscv/src/rv32/asm/switch.S` | CREATE — 32-bit context switch assembly |

---

## Implementation Steps

1. **Extend `Rv32Context`** in `rv32.rs`:
   ```rust
   #[cfg(target_arch = "riscv32")]
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct Rv32Context {
       pub ra: u32, pub sp: u32,
       pub s0: u32, pub s1: u32, pub s2: u32, pub s3: u32,
       pub s4: u32, pub s5: u32, pub s6: u32, pub s7: u32,
       pub s8: u32, pub s9: u32, pub s10: u32, pub s11: u32,
       pub sepc: u32, pub sstatus: u32,
       pub gp: u32, pub tp: u32, pub sscratch: u32,
   }
   ```
   Add a `compile_time_assert` after the struct:
   ```rust
   const _: () = assert!(core::mem::size_of::<Rv32Context>() == 76);
   ```

2. **Add module declarations** to `rv32.rs` `#[cfg(target_arch = "riscv32")]` section:
   ```rust
   mod asm;
   pub mod context;
   pub mod trap;   // needed by Phase 02; stub empty mod here for now
   pub mod boot;   // needed by Phase 02; stub empty mod here for now
   ```

3. **Create `rv32/asm.rs`**:
   ```rust
   use core::arch::global_asm;
   global_asm!(include_str!("asm/switch.S"));
   global_asm!(include_str!("asm/trap.S"));
   ```
   Note: `trap.S` does not exist yet — Phase 02 creates it. The file must exist or `global_asm!` will error.
   Create an empty placeholder `rv32/asm/trap.S` (just a comment) to let Phase 01 compile independently:
   ```asm
   // trap.S — placeholder; implemented in Phase 02
   ```

4. **Create `rv32/asm/switch.S`**:
   ```asm
   .section .text
   .global __switch32

   # fn __switch32(old: *mut Rv32Context, new: *const Rv32Context)
   # a0: old context ptr, a1: new context ptr
   # Rv32Context layout (76 bytes, 4 bytes each):
   #   0: ra, 4: sp, 8: s0, 12: s1, 16: s2, 20: s3,
   #  24: s4, 28: s5, 32: s6, 36: s7, 40: s8, 44: s9,
   #  48: s10, 52: s11, 56: sepc, 60: sstatus, 64: gp, 68: tp, 72: sscratch

   __switch32:
       # Save old context
       sw  ra,   0(a0)
       sw  sp,   4(a0)
       sw  s0,   8(a0)
       sw  s1,  12(a0)
       sw  s2,  16(a0)
       sw  s3,  20(a0)
       sw  s4,  24(a0)
       sw  s5,  28(a0)
       sw  s6,  32(a0)
       sw  s7,  36(a0)
       sw  s8,  40(a0)
       sw  s9,  44(a0)
       sw  s10, 48(a0)
       sw  s11, 52(a0)
       csrr t0, sepc
       sw   t0, 56(a0)
       csrr t0, sstatus
       sw   t0, 60(a0)
       sw  gp,  64(a0)
       sw  tp,  68(a0)
       csrr t0, sscratch
       sw   t0, 72(a0)

       # Load new context
       lw  ra,   0(a1)
       lw  sp,   4(a1)
       lw  s0,   8(a1)
       lw  s1,  12(a1)
       lw  s2,  16(a1)
       lw  s3,  20(a1)
       lw  s4,  24(a1)
       lw  s5,  28(a1)
       lw  s6,  32(a1)
       lw  s7,  36(a1)
       lw  s8,  40(a1)
       lw  s9,  44(a1)
       lw  s10, 48(a1)
       lw  s11, 52(a1)
       lw  t0,  56(a1)
       csrw sepc, t0
       lw  t0,  60(a1)
       csrw sstatus, t0
       lw  gp,  64(a1)
       lw  tp,  68(a1)
       lw  t0,  72(a1)
       csrw sscratch, t0
       ret
   ```

5. **Create `rv32/context.rs`**:
   ```rust
   use super::Rv32Context;

   impl Rv32Context {
       /// Perform a context switch from `old` to `new`.
       ///
       /// # Safety
       /// Both pointers must be valid, aligned `Rv32Context` objects.
       /// `old` is the currently-executing context; `new` is the next context to run.
       #[inline(always)]
       pub unsafe fn switch(old: *mut Rv32Context, new: *const Rv32Context) {
           extern "C" {
               fn __switch32(old: *mut Rv32Context, new: *const Rv32Context);
           }
           __switch32(old, new);
       }
   }
   ```

6. **Wire `switch_context()`** in `rv32.rs` `Arch` impl:
   ```rust
   unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
       context::Rv32Context::switch(old, new);
   }
   ```

7. **Run compile check**:
   ```
   cargo check -p hal-riscv --target riscv32imac-unknown-none-elf
   ```

---

## Todo List

- [ ] Extend `Rv32Context` struct (add 5 CSR fields, add size assert)
- [ ] Add module declarations to `rv32.rs`
- [ ] Create `rv32/asm.rs` with `global_asm!` includes
- [ ] Create `rv32/asm/switch.S` (32-bit)
- [ ] Create placeholder `rv32/asm/trap.S` (one-line comment)
- [ ] Create `rv32/context.rs` with `Rv32Context::switch()`
- [ ] Wire `switch_context()` in `rv32.rs`
- [ ] Verify `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` passes
- [ ] Verify `cargo check -p hal-riscv --target riscv64gc-unknown-none-elf` still passes

---

## Success Criteria

- `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` clean (no errors)
- `Rv32Context` size assert passes: `size_of::<Rv32Context>() == 76`
- `switch_context()` compiles (no `unimplemented!()` in path)
- RV64 check still clean (no regressions in `rv64/` code)

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `global_asm!(include_str!("asm/trap.S"))` fails if file missing | Create placeholder trap.S in this phase |
| Assembly register names differ between RV32/RV64 | Only mnemonic changes (sw/lw vs sd/ld); offsets are the critical difference |
| `Rv32Context` field order must match assembly offset table exactly | Size assert + compile-checked field layout |
