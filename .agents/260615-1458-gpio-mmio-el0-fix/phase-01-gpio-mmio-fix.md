# Phase 01 — Fix Peripheral MMIO EL0 Permissions

**Status:** In Progress  
**File:** `kernel/src/memory/paging.rs`

## Change

In the `#[cfg(target_arch = "aarch64")]` block (lines 153-171), split MMIO mapping into:
- **GIC** (0x0800_0000-0x0900_0000): keep `mmio_flags` (EL1-only — cells must not touch interrupt controller)  
- **Peripherals** (0x0900_0000-0x0904_0000, 0x0A00_0000-0x0A00_4000): add `PageFlags::USER`

## Success Criteria

- `cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat` succeeds
- No `mmio_flags` with USER on the GIC range
- Peripheral range (0x0900_0000+) has USER in flags
