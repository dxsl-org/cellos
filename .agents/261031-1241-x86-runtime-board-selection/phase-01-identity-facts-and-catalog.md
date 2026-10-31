# Phase 01 — Identity facts and catalog API

## Context Links
[plan](plan.md) · [scout](scout-report.md) · ADR-0029 §1–2

## Overview
Expose the boot-time identity facts and a runtime-queryable x86 catalog. Selection behaviour does not change yet.

## Key Insights
- RSDP and SDT OEM fields are already in memory and validated. Exporting them costs only a few bytes in `AcpiInfo`.
- SMBIOS type 1 (manufacturer, product, UUID) is the only stable machine identity. ACPI OEM IDs identify firmware vendors, not machines.

## Requirements
- `AcpiInfo` gains `oem_id: [u8; 6]`, `oem_table_id: [u8; 8]` and `oem_revision: u32` from the XSDT/RSDT header, plus a `dmar_present` that is already derivable.
- Add a Limine SMBIOS request. Bounded parse of the 32- and 64-bit entry points and the type 1 structure only. The entry point is mapped under the same firmware-window admission as ACPI. Malformed tables yield `None`, never a panic.
- `cellos_boards::x86_catalog() -> &'static [&'static BoardDescriptor]` and `find_by_slug(&str)`.
- `hal_soc_x86::X86_EARLY_FACTS`, plus a const assertion that every x86 profile's `com1`, `serial_ports` and windows equal it.
- CPUID leaf 1 ECX bit 31 (hypervisor) helper.

## Architecture
Facts are plain data (`PlatformIdentity { acpi_oem, smbios: Option<SmbiosSystem>, hypervisor: bool }`). They carry no policy and are not stored in the descriptor.

## Related Code Files
`kernel/src/acpi.rs`, `kernel/src/boot/limine.rs`, new `kernel/src/smbios.rs`, `boards/src/lib.rs`, `boards/src/catalog_tests.rs`, `hal/soc/x86/src/lib.rs`, `hal/soc/x86/src/tests.rs`.

## Implementation Steps
1. Extend `acpi::parse` to retain the header OEM fields. Add a unit test on the existing synthetic-table fixtures.
2. Add the SMBIOS request and parser, with host tests for truncated and oversized tables and bad checksums.
3. Add the catalog API and the `X86_EARLY_FACTS` const assertion.
4. Log the identity line at boot (`[identity] acpi_oem=… smbios=… hv=…`).

## Todo List
- [ ] ACPI OEM export + test
- [ ] SMBIOS request + parser + tests
- [ ] catalog API + early-facts assertion
- [ ] boot identity log

## Success Criteria
- `cargo test -p cellos-boards -p hal-soc-x86` and the host tests for the SMBIOS parser pass.
- The x86 kernel builds. The q35 QEMU boot log prints the identity line with OEM `BOCHS ` and `hv=1`.

## Risk Assessment
- Limine SMBIOS absence (A1): the parser returns `None`, and phase 03 fingerprints without it.
- Parsing firmware memory is a potential fault source. Mitigation: bounds checks, and mapping goes through the existing admitted-range check.

## Security Considerations
SMBIOS and ACPI strings are attacker- or firmware-controlled. They are length-bounded, never used as format strings, and never grant authority.

## Next Steps
Phase 02 binds the profile using these facts.
