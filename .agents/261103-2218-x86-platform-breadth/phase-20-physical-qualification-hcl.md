---
phase: 20
title: "Physical qualification and HCL rows"
status: "blocked: hardware (documentation, KVM/VMware guide and software artifacts complete; physical machine captures pending procurement)"
priority: P1
effort: "3d per machine + procurement"
dependencies: [5, 6, "7b", 9, 10, 11, 12, 13]
tier: medium
checkpoint: true
---

# Phase 20: Physical qualification and HCL rows

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [HCL](../../docs/hardware-compatibility-list.md) §2-§6; `.agents/261004-1957-x86-pc-lane/phase-07-physical-lane.md` (the existing physical-lane procedure, which this phase extends)
- ADR-0022 §Authorization (no purchase implied); ADR-0031 (phase 00)

## Overview

Capture S2 rows for two Intel machines: one desktop/mini PC and one server. Update the HCL requirements to the new platform capabilities, and record the KVM and VMware support claims in the docs at their evidence ceilings. Closes the plan.

## Key Insights

- First machine (owner decision, validation 2026-11-03): an Intel mini PC with I219 or I226, so phase 13 (I219) or phase 15 (I226) is the NIC prerequisite, whichever matches the purchased unit. The server (machine B) follows; its NIC phase (14 or 17) is chosen from its `lspci -nn`.
- This phase supersedes nothing in pc-lane phase 07. It uses the same capture rules with the relaxed R1/R2 and the new R4 constraints.

## Requirements

- Functional:
  - Procurement decision recorded before any purchase (owner).
  - Machine A (desktop/mini PC): Intel CPU with VT-x/EPT/VT-d, NVMe (VMD off) or AHCI, an admitted NIC, console via COM/SOL or framebuffer + USB keyboard.
  - Machine B (server): Intel Xeon, BMC with SOL, I350 or BCM5720 (or a 10G NIC from phases 18/19), NVMe/AHCI or RAID controller in HBA mode exposing NVMe/AHCI.
  - Per machine, captures:
    - UART/SOL or framebuffer boot log;
    - ACPI gates line;
    - clock/apic/ir/irq lines;
    - storage mount → write → reboot → read;
    - DHCP + 10-minute ping;
    - VT-d isolation before first DMA (Required profile);
    - `reboot`, `poweroff` and power-button results;
    - `online=N` CPU count;
    - USB keyboard (direct and BMC virtual);
    - machine pin digest.
  - Docs:
    - HCL R1/R2/R4/R6 final wording;
    - machine table rows (S2) or §6 rejections;
    - `docs/system-architecture.md` x86 platform section;
    - `docs/specs/04-hardware.md` §7;
    - `docs/roadmap/hardware-tracks.md`;
    - `CHANGELOG.md`;
    - `boards/pc/x86_64-pc/README.md`;
    - a new "Running on KVM / VMware" guide under `docs/guides/` with the exact libvirt XML and vmx settings, including the vIOMMU options.
- Non-functional: every claim states its evidence ceiling (QEMU, KVM-local, VMware-local, physical S2).

## Assumptions

- **Claim:** procurement for two machines will be approved. **Confidence:** low. **How to verify:** owner decision. Without it the phase is `blocked: hardware`, and S1-S4 remain claimed only at their virtual ceilings.

## Related Code Files

- Modify: the docs listed above; `boards/pc/x86_64-pc/board.rs` (`enabled_drivers` reflects shipped cells)
- Create: `docs/guides/running-on-kvm-and-vmware.md`, `.agents/261103-2218-x86-platform-breadth/evidence/<machine>/…`

## Implementation Steps

1. Choose machines with the HCL §4 checklist; record the exact NIC and storage IDs; pick the matching NIC phase(s); obtain the procurement decision.
2. Run the capture checklist on machine A; fix machine-specific findings in the owning phase (logged as Surprises there).
3. Same for machine B, including BMC SOL + virtual keyboard + NC-SI reachability.
4. Write HCL rows or rejections; update the docs; write the KVM/VMware guide from the phase 05/09 lane configurations.
5. Final review: `haily-reviewer` over the evidence and doc claims (no claim above its ceiling).

## Todo List

- [ ] Machine selection + procurement decision (pending hardware acquisition)
- [ ] Machine A captures (pending hardware acquisition)
- [ ] Machine B captures (pending hardware acquisition)
- [X] HCL + docs + KVM/VMware guide
- [X] Final review
## Success Criteria

- [ ] Two HCL rows at S2 (or a §6 rejection with the reason, plus a replacement machine).
- [ ] The KVM/VMware guide reproduces the phase 05 and phase 09 acceptance runs when followed literally.
- [ ] No document claims physical support for a NIC/controller without a capture.

## Risk Assessment

- Machine-specific firmware bugs may need fixes in earlier phases. Those reopen the owning phase rather than adding workarounds here.

## Security Considerations

Secure Boot must still be disabled (HCL R3) until the signed-boot track ships. Record it per machine.

## Next Steps

Second-node C2C qualification under the pc-lane plan.

## Deviation Log

None.
