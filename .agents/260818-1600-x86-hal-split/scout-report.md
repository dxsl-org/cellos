# Scout Report: x86 HAL Separation

## Relevant files

- `boards/src/lib.rs`, `boards/src/descriptor.rs`: six-board catalog; `Architecture::X86_64` exists but no x86 SoC or module.
- `hal/arch/x86/src/x86_64/uart_16550.rs`: reusable port-I/O mechanism currently owns COM1 `0x3f8` and IRQ4 facts.
- `kernel/src/main.rs`: x86 gate order, ACPI firmware-window admission, direct COM1 probe, PCIe fail-closed path.
- `kernel/src/acpi.rs`: validated firmware discovery for LAPIC, IOAPIC, HPET, and MCFG; this remains kernel integration, not board data.
- `scripts/check-board-configs.sh`, `scripts/check-hal-boundaries.sh`, `.github/workflows/ci.yml`: current six-board build and ownership enforcement.

## Patterns and precedents

- `9ffbbc30` introduced typed integration-only board descriptors.
- `a74569ef` pairs board `SocId` with a SoC profile at the kernel boundary.
- `efcb4e54` keeps mechanisms shared while SoC crates own platform policy.
- `15b940de` merged the x86 BIOS/UEFI hardware lane; its file footprint includes build media, ACPI, paging, platform scan, and QEMU gates.

## Prior constraints

- Preserve full RSDP v2 checksum, segment/bus-aware MCFG, and bounded firmware mapping.
- Keep COM1 available before ACPI; keep interrupt/timer/PCIe closed when firmware evidence is missing.
- Do not claim physical Dell/other PC validation without matching logs.

## Blast radius

- Public types: `SocId`, `FirmwareInterface`, `BootProtocol`, `DriverId`, descriptor validation.
- Runtime: x86 UART configuration and early firmware range admission.
- Build: workspace membership, kernel x86 target dependencies, board matrix target installation.

