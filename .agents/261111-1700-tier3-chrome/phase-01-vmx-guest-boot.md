---
phase: 1
title: "ADR amendment and VMX guest boot"
status: completed
priority: P1
effort: "2-4d"
dependencies: []
tier: thinking
checkpoint: false
---

# Phase 1: ADR amendment and VMX guest boot

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Record the decision to unpark Tier 3 Chrome, then prove the existing x86 hypervisor boots the
pinned Alpine console guest on **Intel VT-x** (nested KVM, `-accel kvm -cpu host`). Every
later phase depends on this, and no lane has ever run the VMX backend.

## Requirements

- Functional: `qemu-hypervisor-smoke-x86.sh` (or a VMX variant of it) reaches the guest
  `/ #` prompt with `-accel kvm -cpu host` on an Intel host; the hypervisor log shows the
  VMX backend selected (`[vmx]` / `X86Virt::Vmx`), not SVM.
- Functional: the SVM/TCG lane still passes unchanged.
- Non-functional: no change to the SVM path; any kernel VMX fix is minimal and isolated.

## Architecture

No new components. The lane gains an accel switch: `HV_ACCEL=kvm` → `-accel kvm -cpu host`
(VMX visible to CellOS via nested), default stays TCG `qemu64,+svm`.
ADR-0017 gets a dated status block: Phase 4 unparked by the maintainer for the desktop,
scope = this plan; ADR-0025 §3.2 is unchanged (Chrome is the approved Tier 3 workload).

## Assumptions

- **Claim:** WSL2 nested KVM exposes VMX to the CellOS guest with `-cpu host`.
  **Confidence:** medium. **Verify:** boot CellOS, check `cpu_features` log for VMX.
- **Claim:** the VMX backend (`hal/arch/x86/src/x86_64/vmx_vcpu.rs`, `init_pvh`) runs a PVH
  guest to a prompt. **Confidence:** low (never exercised). **Verify:** the lane itself.
- **Claim:** the 8259/PIT/UART emulation is backend-neutral. **Confidence:** medium.
  **Verify:** guest console output under VMX.

## Related Files

- Modify: `scripts/qemu-hypervisor-smoke-x86.sh` (accel switch, VMX marker check)
- Modify: `docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md` (status block)
- Modify: `docs/guides/tier3b-linux-vm.md` (Intel VT-x row, only with evidence)
- Modify (only if a fault is found): `hal/arch/x86/src/x86_64/vmx*.rs`, `kernel/src/hypervisor/svm_registry.rs`

## Implementation Steps

1. Amend ADR-0017 (status block, link this plan).
2. Add `HV_ACCEL=kvm` to the smoke lane; require `vCPU ready` + the VMX selection marker + prompt.
3. Run it. On a fault, capture the VMX exit diagnostics (`shutdown_diagnostics`,
   `npf_diagnostics`), root-cause, fix the smallest thing, rerun. Log each in the Deviation Log.
4. Rerun the SVM/TCG lane once to prove no regression.
5. Update the guide's Intel VT-x row with the evidence path; changelog entry.

## Success Criteria

- [x] `HV_ACCEL=kvm bash scripts/qemu-hypervisor-smoke-x86.sh` prints its PASS line with VMX selected. — `HV_SMOKE_MODE=boot` PASS in 8 s (Deviation Log, Evidence).
- [x] Default (TCG/SVM) run of the same lane still PASSes. — TCG/SVM boot PASS on QEMU 10.2.0.
- [x] Kernel diff (if any) reviewed and committed on its own. — Reviewed (plan.md: "done (reviewed)"). **Not committed on its own:** the 3-line `kernel/src/hypervisor/svm_registry.rs` change went in with `e6486ea38b`.

## Security Considerations

VMX fixes touch the EPT/VMCS boundary: any change needs an independent review
(`haily-reviewer`) for guest-escape risk (EPT permissions, MSR/IO bitmaps, CR/VMCS guest state).

## Risk Notes
If nested VMX in WSL2 is unusable (e.g. no unrestricted guest / EPT exposure), stop and
report: the plan has no VMX-free path for an Intel host.

The smoke lane requires exactly QEMU 10.2.0 (`qemu-hypervisor-smoke-x86.sh`); this host has
11.1.3. The KVM/VMX mode must qualify its own QEMU version (the 10.2.0 pin exists for TCG
SVM triple-faults) or the lane must point `QEMU_X86_BIN` at a 10.2.0 build; log which.

## Deviation Log

- Surprise: the VMX backend had never run Linux: first exit `RDMSR EFER` (reason 31) stopped the VM.
  Why: no MSR, EFER-control, CR0.NE/CR4.VMXE or MMIO-decode handling existed.
  Impact: `hal/arch/x86/src/x86_64/vmx_vcpu*`, `guest_mem.rs`, `world_switch.rs`.
  Revert: revert those files; SVM path unchanged except the shared page walk.
- Surprise: nested under KVM the guest saw the outer hypervisor's CPUID leaves (kvm-clock #DE) and
  INVPCID (#UD without the VMX control).
  Why: `guest_cpuid` passed 0x4000_0000 and leaf 7 through.
  Impact: `guest_cpuid.rs` hides 0x4000_0000..=0x4FFF_FFFF, INVPCID, WAITPKG (both backends).
  Revert: restore the filter arms.
- Decision: QEMU 10.2.0 pin applies only to `HV_ACCEL=tcg`; KVM mode ran on 11.1.3.
  Why: the pin exists for TCG SVM triple-faults.
  Impact: `scripts/qemu-hypervisor-smoke-x86.sh`.
  Revert: re-apply the pin to both modes.
- Evidence: `HV_ACCEL=kvm HV_SMOKE_MODE=boot` PASS (8 s); TCG/SVM boot PASS on 10.2.0; hal-x86 39 tests.
