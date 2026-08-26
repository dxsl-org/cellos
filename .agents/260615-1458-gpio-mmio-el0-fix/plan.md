# GPIO MMIO EL0 Permission Fix — hoàn thiện peripheral suite

**Status:** In Progress  
**Priority:** P0 — blocks integration test `aarch64_pwm_demo`

## Root Cause

`mmio_flags` in `kernel/src/memory/paging.rs:126` maps ALL MMIO without `PageFlags::USER`.
AArch64 page table code (`hal/arch/arm/src/aarch64/paging.rs:56-65`) sets `PTE_UXN`
(EL0 cannot execute/read) when USER bit is absent.

Result: Cells running at **EL0** receive `DFSC=0xF` (permission fault level 3) on first GPIO MMIO write.

```
[aarch64] trap ec=0x24 esr=0x9200000F far=0x9030400  ← GPIODIR register
```

Affects: periph-demo, sensor-demo, spi-demo, **pwm-demo** (all use BitBangGpio rlib which directly dereferences PL061 MMIO).

## Phases

| Phase | Status | Description |
|-------|--------|-------------|
| [01 — Fix peripheral MMIO EL0 permissions](phase-01-gpio-mmio-fix.md) | **In Progress** | Add USER flag to 0x09000000-0x09040000 mapping |
| [02 — Rebuild + verify](phase-02-verify.md) | Planned | QEMU run; all 3 GPIO demos complete; integration test passes |

## Key Files

- `kernel/src/memory/paging.rs:126-171` — mmio_flags + aarch64 identity-map block
- `hal/arch/arm/src/aarch64/paging.rs:56-65` — PTE_AP_EL0 conditional
- `tests/integration/tests/periph-can-pwm-adc.rs` — aarch64_pwm_demo probe

## Why USER flag is safe here

ViCell uses Language-Based Isolation (LBI), not hardware MMU isolation between cells.
The GIC (0x08000000-0x09000000) remains EL1-only (cells must not change interrupt routing).
GPIO/UART peripheral range (0x09000000-0x09040000) is safe to expose because:
- Only cells with GPIO manifest capability can construct a `BitBangGpio` driver object
- Rust type system prevents unauthorized driver construction (LBI invariant)
