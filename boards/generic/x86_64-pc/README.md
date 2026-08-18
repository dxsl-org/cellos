# Generic x86_64 ACPI PC

This board contract covers PC-compatible x86_64 machines booted by Limine via
BIOS or UEFI. Firmware supplies the memory map and ACPI tables; Cellos does not
invent fallback LAPIC, IOAPIC, HPET, or PCIe ECAM addresses.

Static PC platform facts such as COM1 wiring and the bounded legacy firmware
windows live in `hal/soc/x86`. CPU, paging, APIC, and port-I/O mechanisms remain
in `hal/arch/x86`; PCIe, NVMe, and e1000 drivers remain shared.

```sh
cargo build -p cellos-kernel --release --target x86_64-unknown-none
```

QEMU is an integration witness only. Physical PC support remains hardware-gated.
