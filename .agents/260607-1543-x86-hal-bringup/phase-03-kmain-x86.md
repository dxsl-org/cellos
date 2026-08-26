# Phase 03 — Kernel `kmain` x86_64 Path

**Status:** TODO  
**Priority:** High — without this, kernel init hangs after boot  
**Estimated effort:** Medium (4 files, mostly `#[cfg]` additions)

---

## Context Links

- `kernel/src/main.rs:42–321` — `kmain()` — needs x86_64 `#[cfg]` blocks
- `kernel/src/memory/paging.rs:125–170` — needs x86_64 MMIO identity-map block
- `kernel/src/main.rs:323–377` — `panic_handler` — needs x86_64 output path
- `hal/arch/x86/src/x86_64/uart_16550.rs` — `puts()` / `putchar()` for early output

---

## Overview

`kmain` has RISC-V-only and AArch64-only branches for:
1. Early UART init (`task::drivers::uart::init()` wraps RISC-V SBI; AArch64 has PL011)
2. `puts()` helper — needs x86_64 to call `uart_16550::putchar()`
3. PLIC init (`#[cfg(target_arch = "riscv64")]`) — x86_64: no PLIC (has APIC instead, already init'd by HAL)
4. SUM bit enabling (`csrs sstatus ...`) — RISC-V only, must skip on x86_64
5. Interrupt enable before idle loop — `csrs sstatus` / `msr daifclr` → x86_64: `sti` (but HAL's `enable_interrupts()` already does this)
6. `paging.rs::init_kernel_paging` — needs x86_64 MMIO identity-map block (LAPIC at 0xFEE0_0000, IOAPIC at 0xFEC0_0000, HPET at 0xFED0_0000)
7. `panic_handler` — needs x86_64 output path via `uart_16550::putchar()`

---

## Requirements

- `kmain` compiles and runs on x86_64 without hitting RISC-V-specific code
- Early UART output works before HAL `ARCH.init()` (COM1 init is idempotent)
- x86_64 MMIO regions are identity-mapped so paging activation doesn't break LAPIC/UART access
- Panic handler produces serial output on x86_64

---

## Architecture

### `puts` helper on x86_64

Add to `kmain`:
```rust
#[cfg(target_arch = "x86_64")]
fn x86_putchar(c: u8) {
    crate::hal::uart_16550::putchar(c);
}
```

`hal::uart_16550::putchar` is the COM1 port-I/O `puts` from the HAL. It is safe to
call before `ARCH.init()` because COM1 at 0x3F8 is always accessible via port I/O
(no MMIO mapping needed).

### MMIO regions for x86_64 paging

Add to `memory/paging.rs` identity-map MMIO section:
```rust
#[cfg(target_arch = "x86_64")]
{
    // QEMU q35 MMIO layout (all need identity-map for post-paging kernel access):
    //   0xFEC0_0000: I/O APIC (4 KB)
    //   0xFED0_0000: HPET (1 KB, but map a page)
    //   0xFEE0_0000: Local APIC (4 KB)
    root_table.identity_map(0xFEC0_0000, 0xFEC0_1000, mmio_flags, &mut alloc_fn)?;
    root_table.identity_map(0xFED0_0000, 0xFED0_1000, mmio_flags, &mut alloc_fn)?;
    root_table.identity_map(0xFEE0_0000, 0xFEE0_1000, mmio_flags, &mut alloc_fn)?;
}
```

### Interrupt enable on x86_64

`HAL::enable_interrupts()` already emits `sti`. Use it instead of inline asm.

Replace the `#[cfg(target_arch = "riscv64")] csrs sstatus...` block for interrupts with:
```rust
#[cfg(target_arch = "x86_64")]
crate::hal::ARCH.enable_interrupts();
```

### Panic handler on x86_64

```rust
fn panic_putchar(c: u8) {
    #[cfg(target_arch = "riscv64")] { let _ = crate::hal::sbi::console_putchar(c); }
    #[cfg(target_arch = "aarch64")] { crate::hal::uart_pl011::putchar(c); }
    #[cfg(target_arch = "x86_64")]  { unsafe { crate::hal::uart_16550::putchar(c); } }
}
```

### `hal_export` for `uart_16550::putchar`

`uart_16550::putchar` is currently gated `#[cfg(target_arch = "x86_64")]` inside
`hal/arch/x86/src/x86_64/uart_16550.rs`. The kernel needs to call it via `crate::hal::uart_16550::putchar`. Verify the re-export chain from `hal/core/src/lib.rs` exposes it when the `x86_64` feature is active.

---

## Related Code Files

| Action | File |
|--------|------|
| Modify | `kernel/src/main.rs` — x86_64 cfg blocks for UART, plic, interrupts, panic |
| Modify | `kernel/src/memory/paging.rs` — x86_64 MMIO identity-map block |
| Verify | `hal/core/src/lib.rs` — re-exports `uart_16550` for x86_64 feature |

---

## Implementation Steps

1. **`kernel/src/main.rs`** — add to top of `kmain`:
   - `#[cfg(target_arch = "x86_64")] crate::hal::uart_16550::init();` (COM1 early init)
   - Update `puts` closure: add x86_64 arm calling `crate::hal::uart_16550::putchar(c)`
   - Remove the `#[cfg(target_arch = "riscv64")] user_hello` block from x86_64 compilation
   - Replace inline `csrs sstatus` for interrupt enable with `hal::ARCH.enable_interrupts()`
     guarded to `#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]` OR just
     call `hal::ARCH.enable_interrupts()` after the existing RISC-V block

2. **`kernel/src/main.rs` panic_handler** — add `#[cfg(target_arch = "x86_64")]` arm

3. **`kernel/src/memory/paging.rs`** — add x86_64 MMIO block after the aarch64 block

4. **Verify** `hal-core` exports: check `hal/core/src/lib.rs` for `uart_16550` re-export
   under `x86_64` feature; add if missing

5. **cargo check** `-p vicell-kernel --target x86_64-unknown-none`

---

## Success Criteria

- `cargo check -p vicell-kernel --target x86_64-unknown-none` exits 0
- No `#[cfg(target_arch = "riscv64")]`-guarded code reaches x86_64 compilation
- LAPIC (0xFEE0_0000), IOAPIC (0xFEC0_0000), HPET (0xFED0_0000) are in the identity-map

---

## Risk Assessment

- **MED** — `puts` helper closure in `kmain` uses a local closure; x86_64 arm must add
  `unsafe` block around `uart_16550::putchar` if that function is `unsafe`
- **LOW** — paging MMIO block is additive; no existing code path changes
- Check: does `memory::paging::puts()` helper (used by paging.rs itself) also need x86_64 arm?
  Currently it only has riscv64+aarch64 arms. Add `#[cfg(target_arch = "x86_64")] crate::hal::uart_16550::putchar(c);`

---

## Security Considerations

- x86_64 LAPIC MMIO at 0xFEE0_0000 identity-mapped with RW (no execute). Correct.
- HPET at 0xFED0_0000: same.
