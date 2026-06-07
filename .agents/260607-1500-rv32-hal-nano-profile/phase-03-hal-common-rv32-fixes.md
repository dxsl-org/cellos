# Phase 03 — HAL Common RV32 Fixes

**Status**: 📋 PLANNED
**Priority**: P3
**Effort**: 1 day
**Parallel**: Yes — independent of Phase 01 and Phase 02

---

## Context Links

- Timer: `hal/arch/riscv/src/common/timer.rs`
- SBI: `hal/arch/riscv/src/common/sbi.rs`

---

## Overview

Two functions in the common HAL are silently broken for RV32 targets — they return 0 or
do nothing because they are `cfg(target_arch = "riscv64")` only:

1. `read_mtime()` → returns `0u64` on RV32 (never breaks; just wrong timing)
2. `set_timer()` → **does nothing** on RV32 — timer interrupt NEVER fires → scheduler deadlock

This phase adds `#[cfg(target_arch = "riscv32")]` branches to both.

---

## Key Insights

**`read_mtime()` on RV32:**
- On RV32, the `time` CSR is only 32-bit (low half). High half is in the `timeh` CSR.
- Must use a read-retry loop to avoid a torn read between `timeh` and `time`:
  ```
  loop {
      hi1 = csrr timeh
      lo  = csrr time
      hi2 = csrr timeh
      if hi1 == hi2 { break; }  // no overflow between reads
  }
  return (hi1 << 32) | lo
  ```
- CSR numbers: `time = 0xC01`, `timeh = 0xC81` — both read-only user-mode CSRs

**`set_timer()` on RV32:**
- SBI Timer extension (EID = 0x54494D45, FID = 0) spec: on RV32, `stime_value_lo` in `a0`, `stime_value_hi` in `a1`, `a2 = 0`
- Current code passes `stime_value as usize, 0, 0` — only works on RV64 where `usize = u64`
- Fix: on RV32 split the u64 into lo (a0) and hi (a1)
- SBI spec reference: RISC-V SBI Spec v1.0, §5.1, Table 4 (note on RV32 calling convention)

---

## Related Code Files

| File | Action |
|------|--------|
| `hal/arch/riscv/src/common/timer.rs` | MODIFY — add `#[cfg(target_arch = "riscv32")]` block to `read_mtime()` |
| `hal/arch/riscv/src/common/sbi.rs` | MODIFY — add `#[cfg(target_arch = "riscv32")]` block to `set_timer()` |

---

## Implementation Steps

1. **Fix `read_mtime()` in `timer.rs`**:

   Replace the existing implementation:
   ```rust
   pub fn read_mtime() -> u64 {
       let time: u64;
       #[cfg(target_arch = "riscv64")]
       unsafe {
           core::arch::asm!("csrr {0}, time", out(reg) time);
       }
       #[cfg(target_arch = "riscv32")]
       unsafe {
           // RV32: time is 32-bit; timeh holds the upper 32 bits.
           // Retry if timeh changes between reads to avoid a torn 64-bit value.
           let lo: u32;
           let hi: u32;
           loop {
               let hi1: u32;
               let hi2: u32;
               core::arch::asm!("csrr {}, timeh", out(reg) hi1, options(nomem, nostack));
               core::arch::asm!("csrr {}, time",  out(reg) lo,  options(nomem, nostack));
               core::arch::asm!("csrr {}, timeh", out(reg) hi2, options(nomem, nostack));
               if hi1 == hi2 {
                   hi = hi1;
                   break;
               }
           }
           time = ((hi as u64) << 32) | (lo as u64);
       }
       #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
       {
           time = 0;
       }
       time
   }
   ```

   Note: `out(reg)` in a loop is valid here — the loop body is `asm!` only, no Rust state
   that the compiler might move. But to satisfy the borrow checker, declare `lo` and `hi`
   inside the loop and break with them assigned. See the exact pattern above.

2. **Fix `set_timer()` in `sbi.rs`**:

   Replace the existing implementation:
   ```rust
   pub fn set_timer(stime_value: u64) {
       #[cfg(target_arch = "riscv64")]
       sbi_call(SBI_EID_TIMER, SBI_FID_SET_TIMER, stime_value as usize, 0, 0);
       #[cfg(target_arch = "riscv32")]
       {
           // SBI spec §5.1: on RV32, a0 = low 32 bits, a1 = high 32 bits.
           let lo = stime_value as usize;           // low 32 bits
           let hi = (stime_value >> 32) as usize;   // high 32 bits
           sbi_call(SBI_EID_TIMER, SBI_FID_SET_TIMER, lo, hi, 0);
       }
   }
   ```

3. **Verify compile check**:
   ```
   cargo check -p hal-riscv --target riscv32imac-unknown-none-elf
   cargo check -p hal-riscv --target riscv64gc-unknown-none-elf
   ```

---

## Todo List

- [ ] Fix `read_mtime()` — add RV32 branch with `time`+`timeh` retry loop
- [ ] Fix `set_timer()` — add RV32 branch with lo/hi split
- [ ] Replace `#[cfg(not(target_arch = "riscv64"))]` fallback with `#[cfg(not(any(riscv64, riscv32)))]` in `read_mtime()`
- [ ] Run `cargo check -p hal-riscv` for both RV32 and RV64 targets

---

## Success Criteria

- `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf` passes
- `read_mtime()` on RV32 reads `time` + `timeh` with carry-safe retry
- `set_timer()` on RV32 passes the correct lo/hi split to SBI
- No regressions on RV64

---

## Security Considerations

`read_mtime()` reads user-mode CSRs — these are always available in S-mode (and U-mode for
the `time` CSR). No privilege escalation risk.
