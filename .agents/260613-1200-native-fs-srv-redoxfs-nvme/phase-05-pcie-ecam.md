# Phase 05 — PCIe ECAM Walker (G2 Deferred)

**Status**: Deferred
**Gate**: G2 hardware (RK3588 or QEMU q35 target)
**Blocks**: Phase 06 (NVMe driver)

---

## Context Links

- `docs/specs/04-hardware.md:88-110` — PCIe + IOMMU strategy
- `docs/specs/09b-vfs-native-fs-adr.md` — confirmed NVMe as G2 block transport for `/srv`
- `pci_types` crate: `github.com/rust-osdev/pci_types` — type definitions (MIT)

---

## Overview

Implement PCIe ECAM (Enhanced Configuration Access Mechanism) enumeration in the kernel.
This is a prerequisite for the NVMe driver (Phase 06); it is not needed for G1 VirtIO-BLK.

**Do not start this phase until:**
- G2 hardware is available (QEMU q35 for development, RK3588 for validation)
- G1 (`/srv` with VirtIO-BLK + RedoxFS) is complete and CI-gated

---

## Architecture

```
kernel init (arch/x86 or arch/arm64)
  └─ pcie_ecam::enumerate()
       ├─ base: 0xB0000000 (QEMU q35) or from DTB (real board)
       ├─ scan bus 0..255 / dev 0..31 / func 0..8
       │   ├─ read vendor/device ID at config_base(bus, dev, func)
       │   ├─ check class code (01:08 = NVMe)
       │   └─ collect PciDevice { bus, dev, func, bar0, bar1, irq }
       └─ register via ResourceRegistry → drivers can claim by class/vendor
```

**ECAM address calculation**:
```rust
fn config_base(ecam_base: usize, bus: u8, dev: u8, func: u8) -> *mut u8 {
    (ecam_base + ((bus as usize) << 20) | ((dev as usize) << 15) | ((func as usize) << 12)) as *mut u8
}
```

---

## Key Decisions

- **`pci_types` crate**: use for `PciAddress`, BAR parsing, capability list walker.
  Add as kernel dependency (not a Cell — needs unsafe MMIO access).
- **BAR mapping**: request MMIO region via Resource Registry before accessing.
  32-bit BAR: `bar_addr & !0xF`; 64-bit: combine BAR0 (lower) + BAR1 (upper).
- **IOMMU**: RISC-V IOMMU required before real NIC/NVMe on physical RISC-V board.
  ARM64 (RK3588) uses SMMU — different implementation. Defer IOMMU until G2 board confirmed.
- **Interrupt model**: MSI-X preferred for NVMe. PCIe capability walk to find MSI-X table.

---

## Files to Create

| File | Purpose |
|------|---------|
| `kernel/src/task/drivers/pcie_ecam.rs` | ECAM base probe, bus scan, device registry |
| `hal/traits/src/pcie.rs` | `HalPcie` trait: `ecam_base() -> usize`, `request_mmio(addr, size)` |
| `hal/arch/x86/src/pcie.rs` | x86_64 impl (QEMU q35 base `0xB0000000`) |
| `hal/arch/arm/src/pcie.rs` | aarch64 impl (DTB parse or RK3588 base) |

---

## Success Criteria

- QEMU q35 boot: `[pcie] found NVMe controller at 00:04.0, BAR0=0x...` in log
- `cargo check -p vicell-kernel --target x86_64-unknown-none` clean
- Resource Registry prevents double-claiming of the same BAR region
