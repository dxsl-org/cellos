# Phase 01 — Identity facts and catalog API

**Status:** done 2026-11-01 (QEMU/TCG ceiling; uncommitted working tree). No Law-1 delta: no `libs/api`/`libs/types` edit, no syscall.

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
- [x] ACPI OEM export + test
- [x] SMBIOS request + parser + tests
- [x] catalog API + early-facts assertion
- [x] boot identity log

## Implementation
- **A1 verified** against the vendored `limine/limine-8.7.0` (the ISO's bootloader, `scripts/x86/make-iso-ci.sh`). `limine.h:574` defines `LIMINE_SMBIOS_REQUEST`, and `PROTOCOL.md` ("SMBIOS Feature") says `entry_32`/`entry_64` are physical for base revision >= 3, which the kernel requests. `common/protos/limine.c:987-1011` fills them from `acpi_get_smbios` on BIOS and UEFI. The executable-file request (`limine.h:492`, `struct limine_file.cmdline`) carries the command line phase 02 needs.
- `kernel/src/acpi.rs`: `AcpiInfo.root_oem: Option<AcpiOem { oem_id, oem_table_id, oem_revision }>`, taken from the checksummed XSDT/RSDT header. The plan's three fields are grouped under one `Option`, so "no validated root" is distinct from zero bytes. `AcpiInfo.dmar_present` is set when a DMAR passes its SDT checksum, whether or not it has a usable unit. New end-to-end `parse` tests build RSDP v1/v2 + RSDT/XSDT + DMAR images.
- `kernel/src/boot/limine.rs`: SMBIOS request with GUID/layout from the vendored header. `get_smbios_entry_points()` reads the response only after `mp_record_valid` places it in bootloader-reclaimable memory.
- `kernel/src/smbios.rs` (new): `discover(entry_32, entry_64, map_physical) -> Option<SmbiosSystem>`.
  - Prefers `_SM3_` (declared length 0x18..=0x20, checksum, entry-point revision 1, table <= 1 MiB) and falls back to `_SM_` (0x1E..=0x20 per the 2.1 erratum, both checksums, `_DMI_` anchor, structure count).
  - Walks the structures to the first type 1. Any overrun, a formatted length below 4, end-of-table, or a string index outside the set gives `None`.
  - Strings are truncated to 64 bytes. UUIDs are normalised to RFC 4122 order for SMBIOS >= 2.6; the all-0 and all-1 values are `None`.
- `kernel/src/board.rs`: `PlatformIdentity { acpi_oem, smbios, hypervisor }` (plain data). Its `Display` quotes firmware bytes and escapes `"`, `\` and non-printables as `\xNN`. `report_identity` writes `[identity] …` through `early_uart`, because the `log` facade is not installed at this point of boot. A `log::info!` here is silently dropped, and so is the existing `[acpi] gates` line.
- `kernel/src/main.rs`: `smbios::discover` reuses the ACPI `map_physical` closure, which admits only the legacy BIOS window, the RSDP exception, or Reserved/ACPI memory. The rejection log tag is now `[firmware]`. The identity is assembled after `acpi::parse` with the existing `cpu_features::hypervisor_present()` (CPUID.1:ECX[31]); the plan's "helper" already existed, so none was added.
- `boards/src/lib.rs`: `x86_catalog()` returns `[x86_64-pc, qemu-q35-x86_64]`. `find_by_slug(catalog, slug)` does an exact byte match. **Deviation:** it takes the catalog explicitly, so a slug from another architecture can never resolve through the x86 lookup.
- `hal/soc/x86/src/lib.rs`: `X86EarlyFacts`, `X86_EARLY_FACTS`, `X86_PROFILES = [&QEMU_Q35, &GENERIC_X86_PC]`, `const fn has_early_facts`, plus a const assertion over `X86_PROFILES`. A mutation check (GENERIC_X86_PC BIOS window size changed) failed to compile with `E0080: evaluation panicked: an x86 profile diverges from X86_EARLY_FACTS`.

## Evidence (2026-11-01)
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 363 passed, 0 failed. The run includes 9 `smbios::tests`, 4 new `acpi::tests` (OEM/DMAR presence) and 2 `board::tests` (identity line, escaping).
- `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`: 15 + 7 passed.
- x86 kernel release, RUSTFLAGS from `scripts/build-x86_64-prod-refusal-ci.sh` plus `-D warnings` (isolated `CARGO_TARGET_DIR=target/p01-dwarn`, unset afterwards): default and `--features policy-required` both `Finished`, no warnings.
- `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc --features policy-required && bash scripts/x86/make-iso-ci.sh`: `ISO_READY=build/vicell-x86.iso`. Then `cargo test … --test nvme-x86 -- --test-threads=1`: `11 passed`. Four tests ran (boot/shell, driver registers, restart, FAT32 mount). The seven `nvme_09c_*` tests self-skip because `VICELL_AHCI_FAULT_ISO` is unset.
- QEMU 8.2.2 q35/TCG/SeaBIOS boots of that ISO (logs in `build/p01-identity/`), all reaching `Cellos >`:
  - plain q35: `[identity] acpi_oem="BOCHS " acpi_table="BXPC    " acpi_rev=0x1 smbios=manufacturer="QEMU" product="Standard PC (Q35 + ICH9, 2009)" uuid=none hv=1`. QEMU's default UUID is all-zero.
  - `-device intel-iommu`: same identity line, plus `[vtd] Intel VT-d: DMA isolation ACTIVE`.
  - `-machine smbios-entry-point-type=32`: same line through the 32-bit entry point.
  - `-uuid 00112233-4455-6677-8899-aabbccddeeff -smbios type=1,manufacturer=Evil"\Corp,product=Box`: `smbios=manufacturer="Evil\"\\Corp" product="Box" uuid=00112233-4455-6677-8899-aabbccddeeff`.
- Not evidenced: UEFI. An OVMF boot (256M and 1G) panics before ACPI at `kernel/src/memory/frame.rs:744` ("memory map has more usable ranges than the allocator tracks (9 > 8)"). That frame-allocator limit is reached before any phase-01 code runs.

## Success Criteria
- `cargo test -p cellos-boards -p hal-soc-x86` and the host tests for the SMBIOS parser pass.
- The x86 kernel builds. The q35 QEMU boot log prints the identity line with OEM `BOCHS ` and `hv=1`.

## Risk Assessment
- Limine SMBIOS absence (A1): the parser returns `None`, and phase 03 fingerprints without it.
- Parsing firmware memory is a potential fault source. Mitigation: bounds checks, and mapping goes through the existing admitted-range check.

## Security Considerations
SMBIOS and ACPI strings are attacker- or firmware-controlled. They are length-bounded, never used as format strings, and never grant authority.

## Next Steps
Phase 02 binds the profile using these facts. Phase 02 needs:
- an executable-file request for the `cmdline`;
- the binding table using `find_by_slug(x86_catalog(), slug)` and `PlatformIdentity.hypervisor`;
- pre-ACPI callers moved from `selected_x86_64_soc()` to `hal_soc_x86::X86_EARLY_FACTS` (`main.rs` COM setup and the ACPI/SMBIOS `map_physical` closure);
- a host test that every `x86_catalog()` entry maps to an `X86_PROFILES` entry with the same slug. Today the mapping is the `SocId` match in `board::selected_x86_64_soc`.
