# Phase 04 — HPET Driver

**Status:** TODO  
**Priority:** High — `timer.rs` is a 2-line stub; `now_ns()` / `set_timeout()` are needed by the scheduler  
**Estimated effort:** Medium (~150 lines new code in `hpet.rs` + small edits)

---

## Context Links

- `hal/arch/x86/src/x86_64/timer.rs:1-4` — current: just calls `init_lapic()`, no `now_ns`
- `hal/arch/x86/src/x86_64/apic.rs` — `init_lapic()` hardcodes initial_count = 62500 (assumes 1 GHz LAPIC)
- Research: HPET report — MMIO base 0xFED0_0000, present on q35 by default, 64-bit counter
- `hal/traits/timer/src/lib.rs` — `Timer` trait: `now_ns() -> u64`, `set_timeout(&self, ns: u64)`

---

## Overview

The existing `timer.rs` is:
```rust
pub fn init() { super::apic::init_lapic(); }
pub fn reset() {}
```

This delegates to `apic::init_lapic()` which hardcodes `62500` as the LAPIC timer initial
count (100 Hz at an assumed 1 GHz LAPIC clock — wildly wrong on QEMU). There is no `now_ns()`
and no proper calibration.

We need:
1. `hpet.rs` — full HPET MMIO driver: init, `now_ns()`, `set_timeout()`, LAPIC calibration
2. `timer.rs` — updated to use HPET for `now_ns()` / `set_timeout()`, LAPIC for periodic tick
3. `apic.rs` — `init_lapic_calibrated(ticks_per_ms: u32)` variant that uses the measured value
4. `x86_64.rs::init()` — call `hpet::init()` before `apic::init_lapic()` so calibration works

**HPET base address:** `0xFED0_0000` (confirmed by research — NOT `0xFEB0_0000` as in some specs docs)

---

## Requirements

- `now_ns()` returns monotonic nanoseconds from HPET main counter (no overflow for 500+ years)
- `set_timeout(ns)` arms HPET Timer 0 in one-shot mode; fires IDT vector 0x42
- LAPIC calibration: HPET times a 10 ms window; resulting `ticks_per_ms` replaces the hardcoded 62500
- HPET MMIO pointer initialized before any `now_ns()` or `set_timeout()` call

---

## Architecture

### HPET register layout

```
0x000: GCAP_ID (RO)  — bits [63:32]=CLK_PERIOD (fs/tick), [13]=64bit counter, [8:12]=num timers-1
0x010: GEN_CONF      — bit[0]=ENABLE_CNF (start counter), bit[1]=LEG_RT_CNF
0x0F0: MAIN_CTR      — main counter (64-bit on QEMU q35)
0x100: T0_CONF       — Timer 0 config
0x108: T0_COMP       — Timer 0 comparator
```

### `hpet.rs` key structs

```rust
static HPET_BASE: AtomicUsize = AtomicUsize::new(0);
static HPET_PERIOD_FS: AtomicU64 = AtomicU64::new(0);  // femtoseconds per tick

pub unsafe fn init(virt_base: usize) {
    HPET_BASE.store(virt_base, Ordering::Release);
    let gcap = read_reg(0x000);
    let period_fs = gcap >> 32;
    // Sanity: 10 MHz minimum = 100_000_000 fs/tick maximum
    assert!(period_fs > 0 && period_fs <= 100_000_000);
    HPET_PERIOD_FS.store(period_fs, Ordering::Release);
    // Disable counter, clear Timer 0, re-enable
    write_reg(0x010, 0);       // stop
    write_reg(0x0F0, 0);       // reset counter
    let t0 = read_reg(0x100);
    write_reg(0x100, t0 & !0b11_1100);  // clear INT_ENB, TYPE, VAL_SET
    write_reg(0x010, 1);       // start
}

pub fn now_ns() -> u64 {
    let period_fs = HPET_PERIOD_FS.load(Ordering::Relaxed);
    let raw = unsafe { read_reg(0x0F0) };
    // ns = raw * period_fs / 1_000_000  (fs → ns: divide by 10^6)
    ((raw as u128 * period_fs as u128) / 1_000_000u128) as u64
}

pub unsafe fn set_timeout_ticks(ticks_from_now: u64) {
    let now = read_reg(0x0F0);
    let target = now.wrapping_add(ticks_from_now);
    // One-shot, edge-triggered, INT_ENB, route 0 (legacy IRQ0)
    write_reg(0x100, 0b0000_0100);  // INT_ENB, one-shot, edge
    write_reg(0x108, target);
}

pub fn ns_to_ticks(ns: u64) -> u64 {
    let period_fs = HPET_PERIOD_FS.load(Ordering::Relaxed);
    // ticks = ns * 1_000_000 / period_fs
    ((ns as u128 * 1_000_000u128) / period_fs as u128) as u64
}
```

### LAPIC calibration using HPET

```rust
// Returns measured LAPIC ticks per millisecond
pub unsafe fn calibrate_lapic() -> u32 {
    const CAL_MS: u64 = 10;
    let period_fs = HPET_PERIOD_FS.load(Ordering::Relaxed);
    let hpet_ticks_per_ms = 1_000_000_000_000u64 / period_fs; // 10^12 fs per ms

    // LAPIC: divide-by-1, max initial count
    apic::lapic_write(apic::LAPIC_TDCR, 0x0B);         // divide by 1
    apic::lapic_write(apic::LAPIC_TIVT, 0xFF);          // masked, one-shot
    apic::lapic_write(apic::LAPIC_TMICT, 0xFFFF_FFFF);  // start counting down

    let hpet_start = read_reg(0x0F0);
    loop {
        let now = read_reg(0x0F0);
        if now.wrapping_sub(hpet_start) >= CAL_MS * hpet_ticks_per_ms { break; }
        core::hint::spin_loop();
    }
    let elapsed = 0xFFFF_FFFFu32 - apic::lapic_read(apic::LAPIC_TMCCT);
    (elapsed as u64 / CAL_MS) as u32  // ticks per ms
}
```

### Updated `x86_64.rs::init()`

```rust
fn init(&self) {
    gdt::init();
    idt::init();
    syscall::init();
    // SAFETY: HPET physical 0xFED0_0000 is identity-mapped by paging init.
    unsafe { hpet::init(0xFED0_0000); }
    let ticks_per_ms = unsafe { hpet::calibrate_lapic() };
    apic::init_lapic_calibrated(ticks_per_ms);
}
```

### IDT vector for HPET Timer 0

Use vector `0x42`. Register handler in `idt.rs` that calls the HPET timer ISR.
For bring-up, the handler can simply call `apic::eoi()` and wake the scheduler.

---

## Related Code Files

| Action | File |
|--------|------|
| Create | `hal/arch/x86/src/x86_64/hpet.rs` |
| Modify | `hal/arch/x86/src/x86_64/timer.rs` — delegate to hpet |
| Modify | `hal/arch/x86/src/x86_64/apic.rs` — add `init_lapic_calibrated`, expose LAPIC regs |
| Modify | `hal/arch/x86/src/x86_64.rs` — add `pub mod hpet;`, update `init()` call sequence |
| Modify | `hal/arch/x86/src/x86_64/idt.rs` — register handler for vector 0x42 |

---

## Implementation Steps

1. Create `hal/arch/x86/src/x86_64/hpet.rs`:
   - `AtomicUsize HPET_BASE`, `AtomicU64 HPET_PERIOD_FS`
   - `init(virt_base)`, `now_ns()`, `set_timeout_ticks()`, `ns_to_ticks()`, `calibrate_lapic()`
   - `read_reg` / `write_reg` helpers via `core::ptr::read/write_volatile`

2. Add `#[cfg(target_arch = "x86_64")] pub mod hpet;` to `x86_64.rs`

3. Update `apic.rs`:
   - Expose `LAPIC_TDCR`, `LAPIC_TIVT`, `LAPIC_TMICT`, `LAPIC_TMCCT` as public constants
   - Add `lapic_read(reg)` / `lapic_write(reg, val)` as `pub` functions
   - Add `init_lapic_calibrated(ticks_per_ms: u32)` that computes the correct initial count

4. Update `timer.rs` to provide a `now_ns()` fn delegating to `hpet::now_ns()`

5. Update `x86_64.rs::Arch::init()` to call `hpet::init` + `calibrate_lapic` + `init_lapic_calibrated`

6. Update `idt.rs` to install handler at vector 0x42 for HPET Timer 0

7. `cargo check -p hal-x86 --target x86_64-unknown-none`

---

## Success Criteria

- `now_ns()` compiles and returns a non-zero value when called post-boot
- `calibrate_lapic()` returns a plausible ticks_per_ms (1000–4000 range on QEMU)
- HPET base address constant is `0xFED0_0000` (not `0xFEB0_0000`)
- `Timer` trait is implemented for `X86_64Arch` via `hpet::now_ns` / `hpet::set_timeout_ticks`

---

## Risk Assessment

- **MED** — HPET init ordering: `hpet::init()` must run after paging activates (so
  0xFED0_0000 is mapped) but before `init_lapic_calibrated()`. Currently `hal::ARCH.init()` is
  called at step 1 of `kmain` — BEFORE paging. Either: 
  - Move `hpet::init()` out of `Arch::init()` into a separate `Arch::init_timers()` call in `kmain`
    (after paging is activated), OR
  - Identity-map HPET in paging init (Phase 03 already does this) AND call `hal::ARCH.init()`
    after paging (requires reordering in `kmain` for x86_64)
  - **Decision**: Call `ARCH.init()` after `activate_paging()` on x86_64. Add
    `#[cfg(target_arch = "x86_64")]` guard to move HAL init to after paging in `kmain`.

- **LOW** — HPET interrupt vector 0x42 conflicts if something already uses it — check
  `idt.rs` handler table before assigning.

---

## Security Considerations

- HPET MMIO is identity-mapped supervisor-only (PTE_US=0). No cell can access it directly.
- `calibrate_lapic()` spins for 10 ms — acceptable at boot; this is not in any hot path.
