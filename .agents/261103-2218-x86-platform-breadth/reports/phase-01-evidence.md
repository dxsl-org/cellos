# Phase 01 evidence — ACPI table coverage (2026-11-04)

Base HEAD `64ad34027`; working tree uncommitted. Evidence ceiling: host + QEMU TCG + QEMU/KVM (local).

## Host

- Baseline, before edits: `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` → 400 passed.
- After the module split alone: 400 passed.
- After phase 01: 408 passed. The 8 new tests are in `kernel/src/acpi/tests/tables.rs`:
  - two IOAPICs kept in order, with legacy last-wins unchanged;
  - an x2APIC CPU with ID 300; absent sockets filtered;
  - override flags; xAPIC and x2APIC NMIs;
  - a bad MADT length stops the walk; IOAPIC overflow is counted (16 kept, 4 dropped);
  - MCFG keeps every allocation, including segment 1;
  - DMAR: two DRHDs with scopes, a truncated 5-deep path, an RMRR with a scope, header flags; a short RMRR does not stop the walk;
  - FADT rev 1 (legacy ports, no reset, a 100-byte table rejected);
  - FADT rev 6 (X_ fields win, fallback to legacy on a zero X_ block, reset reg, MSI-forbidden flag, HW_REDUCED, sleep control; boot-arch ignored before rev 3);
  - SPCR IO (rev 2) and MMIO (rev 4 with clock);
  - full RSDP→XSDT walk reaching APIC/FACP/SPCR, and a corrupt FACP dropped.
- `cargo fmt -p cellos-kernel -- --check`: clean.
- `cargo clippy -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings`: clean (the CI lint target).

## QEMU

| Lane | Command | Result |
|---|---|---|
| Baseline TCG | `BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso` | PASS |
| Phase 01 TCG | same | PASS |
| Phase 01 KVM | `X86_ACCEL=kvm X86_CPU_MODEL=host …` | PASS |
| `iommu-dmar-x86` | `cargo test --test iommu-dmar-x86` (needs `build/x86-pc-lane/vicell-x86-pc.iso` from `X86_LIMINE_CONF=scripts/x86/limine-pc.conf scripts/x86/make-iso-ci.sh`) | 3/3 |
| KVM + `-device intel-iommu` | manual 45 s boot | lines below |

New log lines (TCG and KVM identical):

```
[acpi] FADT: rev=3 pm_timer=0x608/24 reset=true hw_reduced=false msi_forbidden=false sci=9
[acpi] gates: madt=true hpet=true mcfg=true dmar=false fadt=true spcr=false
[acpi] topology: cpus=1 ioapics=1 nmis=1 mcfg=1 drhd=0 rmrr=0 ir=false x2apic_opt_out=false
```

With `intel-iommu`: `dmar=true` and `topology: … drhd=1 rmrr=0`.

**Behaviour diff (TCG, baseline vs phase 01).** I diffed every `INFO [..]` and gate line other than the `[acpi]` lines. The only change: the usable RAM range ends 48 KiB earlier (`0xe03e000` → `0xe032000`; 57238 → 57226 frames). That is the larger kernel image (parsers plus the ~10 KiB topology static). Nothing else differs.
