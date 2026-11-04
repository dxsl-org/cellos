---
phase: 3
title: "x2APIC and multi-IOAPIC"
status: completed
priority: P1
effort: "3d"
dependencies: [1]
tier: thinking
checkpoint: false
---

# Phase 03: x2APIC and multi-IOAPIC

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `hal/arch/x86/src/x86_64/apic.rs:21-24,72,92-96,229-245`; `kernel/src/boot/limine.rs:209-230` (MP request `flags: 0`)
- `kernel/src/main.rs:596-602` (IOAPIC + override binding)
- Intel SDM Vol 3 §11.12 (x2APIC); Limine protocol MP feature (x2APIC flag)

## Overview

The local APIC driver gets an xAPIC (MMIO) backend and an x2APIC (MSR) backend, and the IOAPIC layer supports every IOAPIC from MADT. Firmware that hands over in x2APIC mode, and CPUs with IDs above 255, stop closing the timer gate.

## Key Insights

- Once firmware enables x2APIC, the OS cannot go back to xAPIC without a full disable, so the kernel has to support x2APIC to boot there at all.
- APIC IDs above 255 need interrupt remapping for IOAPIC/MSI destinations (phase 12). Until phase 12 ships, those CPUs stay offline and are logged.
- Policy choice: use x2APIC whenever CPUID.1:ECX[21] is set. Every KVM `-cpu host` boot then exercises the x2APIC path, and TCG `-cpu qemu64` (no x2apic) keeps the xAPIC path covered.
- Hypervisor guest code (`svm_vcpu.rs`, `vmx_vcpu.rs`) emulates xAPIC for its guests and is not touched.

## Requirements

- Functional:
  - `apic` backend trait/enum: `read(reg)`, `write(reg)`, `send_ipi(dest: u32, vector, mode)` (64-bit ICR in x2APIC), `id() -> u32`, `eoi()`, timer registers.
  - x2APIC enable sequence: set IA32_APIC_BASE.EXTD after EN. Set the Limine MP x2APIC flag when CPUID supports it, so Limine starts APs in the same mode.
  - `register_cpu(logical, apic_id: u32)` accepts 32-bit IDs in x2APIC mode. IDs above 255 without IR are refused with `[x86-smp] cpu apic_id=… offline: needs interrupt remapping`.
  - IOAPIC table from `AcpiInfo.ioapics`: per unit base, GSI base, redirection count (IOAPICVER[23:16]). `route_gsi(gsi, vector, dest, polarity, trigger)` picks the unit by GSI range. Override flags honoured (ACPI MPS INTI flags).
  - The COM1 RX IRQ and HPET/PM SCI routes go through `route_gsi`.
- Non-functional: no behaviour change for TCG xAPIC lanes; no added lock on the EOI path.

## Architecture

`hal/arch/x86/src/x86_64/apic/{mod.rs, xapic.rs, x2apic.rs}` and `ioapic.rs`, split out of `apic.rs` (291 lines). The mode is selected once at BSP init and stored in an `AtomicU8`; APs read it. Interrupt-remapping hooks are left as one function boundary (`irq_destination(dest) -> RteDest`) that phase 12 implements.

## Assumptions

- **Claim:** Limine 8.7.0 honours the MP request x2APIC flag and reports x2APIC IDs in `lapic_id`. **Confidence:** medium. **How to verify:** read `limine/limine-8.7.0` PROTOCOL.md (MP feature).
- **Claim:** QEMU TCG `-cpu qemu64` does not advertise x2APIC. **Confidence:** high. **How to verify:** boot log `apic=xapic`.

## Related Code Files

- Create: `hal/arch/x86/src/x86_64/apic/{mod,xapic,x2apic}.rs`, `hal/arch/x86/src/x86_64/ioapic.rs`
- Modify: `hal/arch/x86/src/x86_64.rs:259,382-384`, `kernel/src/boot/limine.rs:229`, `kernel/src/main.rs:596-602`, `hal/arch/x86/src/x86_64/uart_16550.rs:216-240` (route via `route_gsi`), `kernel/src/task/smp.rs` (`x86_secondary_main` passes a u32 ID)

## Implementation Steps

1. Move the current MMIO code into `xapic.rs` behind the backend enum. No behaviour change; TCG lanes stay green.
2. Add `x2apic.rs` (MSR 0x800 + reg>>4; ICR 0x830 64-bit; SELF_IPI optional). Add enable logic and the Limine flag.
3. Widen `register_cpu`/`init_secondary` to u32 IDs. Add the >255 refusal.
4. Build the IOAPIC table from `AcpiInfo`; add `route_gsi` with polarity/trigger; move the existing call sites.
5. Unit tests: ICR encoding per mode; GSI → (unit, pin) lookup across two units; override flag decoding.
6. Lanes: KVM `-cpu host -smp 2` (x2APIC) and TCG `-cpu qemu64,+pdpe1gb -smp 2` (xAPIC) for `x86_64-boot`, `serial-x86` and `scripts/x86/qemu-domain-test.sh` (topology line).

## Todo List

- [ ] Backend split (xAPIC)
- [ ] x2APIC backend + enable
- [ ] u32 APIC IDs
- [ ] Multi-IOAPIC routing
- [ ] Tests + lanes

## Success Criteria

- [x] KVM shows `apic=x2apic` with `online=2` (manual `-smp 2` boot and the domain lane's log); TCG `qemu-domain-test.sh` PASS with `apic=xapic online=2`. The KVM domain lane stalls at the VMM smoke step, identically at baseline HEAD (pre-existing, [evidence](reports/phase-03-evidence.md)).
- [x] `serial-x86` RX-IRQ input passes in both modes (tcg xAPIC, kvm x2APIC).
- [x] Unit tests for ICR encoding and two-unit GSI routing pass (plus INTI flag decoding).

## Risk Assessment

- Every existing KVM lane changes APIC mode. Run all x86 suites under KVM before closing the phase.
- Spurious-vector and LVT differences between the modes (e.g. LDR is read-only in x2APIC). Covered by the unit tests plus the boot lanes.

## Security Considerations

None. No cell-facing surface.

## Next Steps

Phase 04a allocates MSI vectors against this backend; phase 07 scales CPU count; phase 12 adds remapped destinations.

## Deviation Log

- Deviation: no `apic/{mod,xapic,x2apic}.rs` split. `apic.rs` dispatches each register access on a latched mode flag (`lr`/`lw` → MSR `0x800 + reg/16` or MMIO), and the ICR is special-cased. IOAPIC routing moved to the new `ioapic.rs`.
  Why: the two backends differ only in the access primitive and the ICR, so three files would duplicate every register helper.
  Impact: `apic.rs` ≈ 330 lines.
  Revert: split if a third backend appears.
- Decision: the Limine MP request always sets the x2APIC flag (`flags: 1`).
  Why: the request is a compile-time static. Limine enables x2APIC only where the CPU supports it, and `apic::init_mode` latches whatever each CPU has (enabling it itself if firmware left xAPIC).
  Impact: every image, including the G1 private lane, runs x2APIC on x2APIC-capable CPUs. G1 latches the mode in `init_timers_inner`.
  Revert: `flags: 0` plus `init_mode` without the enable write.
- Decision: CPUs with APIC ID > 255 are refused in `register_cpu` and skipped (logged) in the MP walk.
  Why: IOAPIC/MSI physical destinations are 8-bit until interrupt remapping (phase 12).
  Impact: no change on ≤255-thread machines.
  Revert: phase 12 lifts it.
- Deviation: the legacy `apic::set_ioapic_phys`/`IOAPIC_GSI_BASE` were removed. kmain registers every MADT IOAPIC (`register_x86_ioapics`, mapping all but the legacy unit with `map_mmio_x86`), and `uart_16550::init_input_irq` now returns `false` (IER untouched) when no unit serves the GSI.
  Why: the single-unit API cannot route GSIs above the first IOAPIC.
  Impact: `x86_ioapic_base` remains only as the gate and for the paging map.
  Revert: n/a.
