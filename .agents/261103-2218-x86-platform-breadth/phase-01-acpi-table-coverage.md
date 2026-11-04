---
phase: 1
title: "ACPI table coverage (FADT, SPCR, multi MADT/MCFG/DMAR)"
status: completed
priority: P1
effort: "2d"
dependencies: [0]
tier: medium
checkpoint: false
---

# Phase 01: ACPI table coverage

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `kernel/src/acpi.rs:16-59` (`AcpiInfo`), `:383-398` (dispatch), `:419-712` (parsers), `:720-947` (tests, `firmware_image` builder)
- ACPI 6.5 §5.2.9 FADT, §5.2.12 MADT; Microsoft SPCR spec; Intel VT-d spec §8 (DMAR)

## Overview

Parse every fact later phases need, without changing behaviour yet: FADT, SPCR, all IOAPICs and processor entries, all MCFG allocations, all DRHD and RMRR records. Data only — consumers arrive in phases 02, 03, 06, 10 and 12.

## Key Insights

- `parse_madt` overwrites `ioapic_base` per entry (`acpi.rs:469-472`). Xeon machines have one IOAPIC per socket/IIO, so the current code keeps only the last.
- The `flags` field of interrupt source overrides (polarity/trigger) is read but dropped (`acpi.rs:480-487`). SCI and some ISA routes need it.
- FADT `IAPC_BOOT_ARCH` bit 3 ("MSI not supported") must be honoured by phase 04a.
- `acpi.rs` is 947 lines. The new parsers go into a module directory instead of growing the file.

## Requirements

- Functional:
  - `AcpiInfo` gains:
    - `fadt: Option<FadtInfo>`: PM timer GAS + 32/24-bit flag, reset register GAS + value, SCI GSI, SMI_CMD, ACPI_ENABLE, PM1a/b EVT/CNT, X_DSDT, IAPC_BOOT_ARCH, HW_REDUCED flag.
    - `spcr: Option<SpcrInfo>`: interface type, GAS, baud, GSI, PCI BDF.
    - `ioapics: [IoApicEntry; 16]` + count.
    - `cpus: [CpuEntry; 256]` + count, built from LAPIC and x2APIC records: APIC ID u32, ACPI UID, enabled/online-capable.
    - `overrides` with flags.
    - `nmis`.
    - `mcfg: [McfgAlloc; 8]`.
    - `drhds: [DrhdEntry; 16]` with device scopes (fixed capacity 32 per unit).
    - `rmrrs: [RmrrEntry; 16]`.
    - DMAR `intr_remap` and `x2apic_opt_out` flags.
  - Capacity overflow is counted and logged as `[acpi] <table> truncated at N`. It never panics.
  - The existing fields (`ioapic_base`, `ecam_base`, `dmar_*`, `hpet_base`) keep their current values: first IOAPIC, segment-0 MCFG, selected DRHD. No consumer changes.
- Non-functional: no heap allocation (parsing runs before heap init — confirm in step 1); every read bounds-checked against the validated table length; a checksum failure rejects that table only.

## Architecture

`kernel/src/acpi.rs` → `kernel/src/acpi/{mod.rs, madt.rs, mcfg.rs, hpet.rs, dmar.rs, fadt.rs, spcr.rs, tests.rs}`. `mod.rs` keeps `parse()`, the RSDP/XSDT walk and `dispatch_sdt`. Each table module exposes `parse(virt, len, &mut AcpiInfo)`. The fixed-capacity arrays use a small `BoundedVec<T, N>` (copy type, `len` + array) defined in `mod.rs`.

## Assumptions

- **Verified (2026-11-03):** `acpi::parse` runs at `kernel/src/main.rs:435`, before `memory::heap::init_heap` at `main.rs:734`, so the fixed-capacity arrays are required.
- **Claim:** `AcpiInfo` stays small enough for a stack copy, or is moved to a static. **Confidence:** medium. **How to verify:** `size_of::<AcpiInfo>()` in a unit test. If it exceeds 16 KiB, store it in a `static` written once.

## Related Code Files

- Delete/move: `kernel/src/acpi.rs` → `kernel/src/acpi/*.rs`
- Modify: `kernel/src/main.rs` (log line `[acpi] gates:` gains `fadt= spcr= ioapics= cpus= drhd= rmrr=`)

## Implementation Steps

1. Confirm the heap/parse order (Assumptions). Move the code into the module layout with no logic change; run `cargo test -p cellos-kernel acpi` to stay green.
2. Add `BoundedVec`. Convert the MADT parser to fill `ioapics`, `cpus` (types 0, 9), `overrides` with flags, and `nmis` (types 4, 0xA). Keep the legacy fields mapped to the first entry.
3. MCFG: record all allocations; keep the segment-0 selection.
4. DMAR: record all DRHDs with device scopes (type, bus, path) and RMRRs (base, limit, scopes), plus the header flags. Keep the current selection rule for `dmar_*`.
5. FADT: handle revision 1-6 lengths and prefer X_ fields when non-zero. Handle the HW_REDUCED flag (no PM1, no PM timer, reset register mandatory).
6. SPCR: revision 1-4; interface types 0 (16550), 1 (16450), 0x12 (16550-compatible with parameters).
7. Tests: extend the `firmware_image` builder with FADT/SPCR/multi-IOAPIC/x2APIC/multi-DRHD/RMRR fixtures, plus truncation and checksum-failure cases.

## Todo List

- [ ] Module split, no behaviour change
- [ ] MADT, MCFG, DMAR full records
- [ ] FADT, SPCR
- [ ] Fixture tests

## Success Criteria

- [x] `cargo test -p cellos-kernel acpi` passes, including new cases: two IOAPICs kept in order; an x2APIC entry with ID 300 recorded; override flags preserved; FADT rev 1 and rev 6 lengths (rev 2 boot-arch case included); HW_REDUCED FADT; SPCR MMIO and IO variants; DMAR with two DRHDs + one RMRR; truncation counted without panic. (408/408, [evidence](reports/phase-01-evidence.md))
- [x] `x86_64-boot` (TCG, KVM) and `iommu-dmar-x86` (3/3) pass unchanged. The boot log shows the extended `[acpi] gates:` line (`fadt=true`) and a new `[acpi] topology:` line (`ioapics=1`).

## Risk Assessment

- A behaviour change hidden in the "no-op" split. Mitigation: step 1 lands as its own commit with an identical boot log diff, apart from the new log fields.

## Security Considerations

The ACPI tables come from firmware and are untrusted input. Every length and offset gets checked, and a malformed record stops that table's walk (same rule as the existing DMAR walk).

## Next Steps

Phases 02, 03, 06 and 10 consume these facts.

## Deviation Log

- Deviation: the large lists live in a write-once static `AcpiTopology` (`acpi::with_topology`), not in `AcpiInfo`.
  Why: the kernel stack is 64 KiB (`hal/arch/x86/src/x86_64/boot.rs:10`), and `AcpiInfo` is returned by value and copied in `kmain`. The topology is about 10 KiB.
  Impact: `AcpiInfo` gains only `irq_override_flags`, `fadt` and `spcr`. Consumers in phases 03, 07 and 12 read the lists through `with_topology`. Host tests use `parse_into` with a boxed topology.
  Revert: move the lists into `AcpiInfo` if a heap-backed copy becomes available.
- Deviation: legacy `ioapic_base` keeps its **last**-entry meaning, not "first IOAPIC" as Requirements said.
  Why: the existing code is last-wins (`acpi.rs:469-472` before the split), and this phase is data-only.
  Impact: none on a single-IOAPIC machine. Phase 03 replaces the field with the IOAPIC table.
  Revert: n/a.
- Deviation: capacities are 16 device scopes per DRHD and 8 RMRRs × 8 scopes, not 32 and 16.
  Why: keeps the static near 10 KiB. Real Xeon DRHD scope lists observed in public DMAR dumps are short [INFERENCE, not measured here].
  Impact: overflow is counted and logged (`BoundedVec::dropped`, `[acpi] DMAR record has more than …`). Phase 12 must refuse a device whose unit has dropped scopes.
  Revert: raise the consts in `acpi/dmar.rs`.
- Decision: MADT processor entries that are neither enabled nor online-capable are not recorded.
  Why: firmware lists absent sockets that way, and they can never be started.
  Impact: `topo.cpus` holds startable CPUs only.
  Revert: drop the `usable()` filter in `madt::push_cpu`.
- Decision: new files `acpi/bytes.rs` (bounds-checked reads), `acpi/records.rs` (shared records), `acpi/fadt.rs` and `acpi/spcr.rs` (each with its own record type), and `acpi/tests/tables.rs` (new fixtures).
  Why: the 200-line file guideline.
  Impact: none on behaviour.
  Revert: n/a.
