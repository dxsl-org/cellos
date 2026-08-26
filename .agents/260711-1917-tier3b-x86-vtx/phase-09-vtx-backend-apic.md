# Phase 09 — Intel VT-x backend bring-up (KVM/HW lane) + optional LAPIC/APICv upgrade

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-06](phase-06-virtio-mmio-x86.md), [phase-03](phase-03-vmcs-worldswitch-exit.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-09-vgic-gich-gicv.md` (the "hardware IRQ
  upgrade" analog — trap-emulate MVP → hardware-assisted path).
- Verified: `hal/arch/x86/src/x86_64/vmcs.rs` (P03 skeleton), `hal/arch/x86/src/x86_64/apic.rs:11`
  (LAPIC 0xFEE00000 / IOAPIC 0xFEC00000), `hal/arch/x86/src/hypervisor.rs` (vendor dispatch).

## Overview
- **Priority:** P2 · **Status:** pending · **Depends on:** 03,06
- Bring the **Intel VMX backend** to life on a real-Intel-hardware / nested-KVM lane (it only compiled +
  returned NotSupported through P03-P08, since TCG cannot run VMX), and — optionally — upgrade the
  no-LAPIC MVP to an emulated LAPIC (and APICv/virtual-APIC page where available) for interrupt
  throughput. This is the x86 analog of ARM P09's "trap-emulate MVP → hardware vGIC upgrade": both are
  **non-blocking** performance/coverage phases layered on a working correct-but-minimal base.
- **Gate:** requires a CI/dev lane with real VMX (KVM-enabled Linux runner or Intel hardware). If no such
  lane exists, VMX bring-up stays a documented TODO and the SVM path remains the shipped target.

## Key Insights
- **VMX backend reuses the P03 scaffold:** VMCS field setup, world-switch, and the exit decoder are
  already written to a vendor-neutral contract in P03. Bring-up = fill the VMX-specific control-field
  computation (true-MSR masking) + VMLAUNCH/VMRESUME resume-state handling + VMX exit-reason table, then
  run the SVM-validated M1→M4 milestones under `-accel kvm -cpu host`.
- **spike step 0 (mirror ARM m4):** before any VMX code, verify the target lane actually exposes VMX
  (`/dev/kvm` + nested virt, or bare Intel HW). Do not assume; ARM's P09 added a GICH-availability spike
  for exactly this reason. TCG is confirmed VMX-incapable (plan Validation Log) — never gate CI on VMX.
- **LAPIC upgrade (optional):** the P05 MVP boots with `nolapic noapic acpi=off`. To drop those and get
  a real interrupt controller: emulate the LAPIC — **x2APIC** (MSRs 0x800-0x8FF via `Msr` exits, cheaper
  than MMIO) is preferred over xAPIC (MMIO at 0xFEE00000 via EPT-violation). This also unlocks the
  **VMX-preemption timer** as a cleaner budget mechanism and enables **APICv / virtual-interrupt
  delivery** (virtual-APIC page) where the hardware supports it — reducing exits under heavy virtio
  traffic. Requires building a minimal ACPI RSDP+MADT (Firecracker pattern, RSDP at 0xE0000) since
  dropping `acpi=off` re-enables ACPI probing.
- **TSC offsetting:** with a real timer path, set the TSC-offset VMCS field / SVM VMCB TSC offset so the
  guest sees a monotonic TSC from 0; keep the PIT fallback for guests that still calibrate via it.
- **Do not regress SVM:** all upgrades are behind the vendor dispatch + a cmdline/feature toggle; the
  SVM+no-LAPIC+TCG CI path stays the default green lane.

## Requirements
**Functional**
- VMX backend runs M1 (smoke) → M2 (Alpine shell) → M3 (blk) → M4 (net) under `-accel kvm -cpu host`
  (or Intel HW), reusing the P05-P08 cell personality unchanged.
- (Optional) x2APIC LAPIC emulation + minimal MADT/RSDP; drop `nolapic acpi=off`; VMX-preemption-timer
  budget; APICv where available.
- TSC-offset field set on both vendors.

**Non-functional**
- No regression to the SVM/TCG CI lane. VMX gated on a HW/KVM lane; absent → documented TODO.

## Architecture
```
vendor dispatch (P01/P03):
  Svm  → VMCB path (shipped, TCG CI)                       [default green lane]
  Vmx  → VMCS path (this phase, KVM/HW lane)               [non-blocking]
optional APIC upgrade (feature-gated):
  no-LAPIC MVP (nolapic acpi=off)  ──toggle──►  x2APIC emul + MADT/RSDP + (APICv if HW)
```

## Related Code Files
**Modify**
- `hal/arch/x86/src/x86_64/vmcs.rs` — complete VMX control-field computation + resume-state handling
- `hal/arch/x86/src/x86_64/vmexit_decode.rs` — VMX exit-reason table (shares SVM decode targets)
- `hal/arch/x86/src/hypervisor.rs` — activate the VMX arm (remove NotSupported once HW-validated)
**Create (optional APIC upgrade)**
- `cells/services/hypervisor/src/lapic_x2apic.rs` — x2APIC MSR model
- `cells/services/hypervisor/src/acpi_tables.rs` — minimal RSDP + MADT builder
**Modify**
- `cells/services/hypervisor/src/boot_info.rs` — set rsdp_paddr, drop `nolapic acpi=off` (feature-gated)

## Implementation Steps
1. **spike 0:** confirm the lane exposes real VMX (`/dev/kvm` nested / Intel HW). Abort to TODO if not.
2. Complete VMX control fields via true-MSRs; VMLAUNCH first entry, VMRESUME thereafter.
3. Run M1 smoke under KVM; then M2-M4 with the unchanged cell personality.
4. Set TSC-offset on both vendors.
5. (Optional) x2APIC MSR model + minimal MADT/RSDP; feature-toggle to drop `nolapic acpi=off`; enable
   VMX-preemption-timer budget; enable APICv/virtual-APIC page if the HW advertises it.
6. Regression-run the SVM/TCG lane to confirm no drift.

## Todo List
- [ ] spike 0: verify lane exposes real VMX (else document TODO, stop)
- [ ] VMX control-field computation (true-MSRs) + VMLAUNCH/VMRESUME
- [ ] VMX exit-reason decode table (shares SVM targets)
- [ ] M1→M4 pass under `-accel kvm -cpu host`
- [ ] TSC-offset set (both vendors)
- [ ] (optional) x2APIC + MADT/RSDP + drop nolapic/acpi=off + APICv
- [ ] SVM/TCG lane regression-clean

## Success Criteria
- On a KVM/Intel lane, the VMX backend boots Alpine to `/ #` and passes blk mount + apk (M2-M4) with the
  P05-P08 cell personality unchanged.
- SVM/TCG CI stays green (no regression).
- (If APIC upgrade done) guest boots WITHOUT `nolapic acpi=off`, uses the LAPIC timer, and virtio IRQ
  throughput improves under a sustained download.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| No VMX lane available at all | Med×Med | Spike 0 gate; ship SVM-only; VMX = documented TODO (title still "VT-x-capable") |
| VMX entry consistency failures differ from SVM | Med×High | Reuse P03 error-logging; true-MSR masking; bisect guest-state |
| APIC upgrade destabilizes the working MVP | Med×High | Feature-gated; SVM+no-LAPIC stays default; upgrade opt-in |
| APICv unsupported under nested KVM | Med×Low | Fall back to trap-emulate LAPIC; APICv is pure optimization |

## Security Considerations
- VMX backend inherits the same EPT isolation + host-state discipline as SVM (P02/P03) — no new trust
  surface. LAPIC/ACPI emulation (if added) is trap-mediated; no passthrough of real LAPIC MMIO.

## Next Steps
- P10 documents both vendor lanes (SVM/TCG CI + VMX/KVM) and finalizes the trait shape.
