---
phase: 0
title: "Governance, machine selection, firmware pin"
status: in-progress
priority: P1
effort: "1d + owner review + procurement"
dependencies: []
tier: thinking
checkpoint: true
---

# Phase G0: Governance, machine selection, firmware pin

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [ADR-0033](../../docs/decisions/0033-x86-intel-igpu-native-compute.md); [evidence](research/vendor-and-platform-evidence.md)
- x86 breadth [phase 20](../261103-2218-x86-platform-breadth/phase-20-physical-qualification-hcl.md) (first machine = Intel mini PC with I219/I226)

## Overview

Record the decision, register the plan, pick one exact machine that serves both the HCL row and the GPU work, and pin the GuC firmware.

## Key Insights

- `xe`-official iGPU platforms: Lunar Lake, Panther Lake, Wildcat Lake. N100/N150 (ADL-N), MTL and ARL are force-probe in `xe`.
- One machine for both plans halves procurement and keeps the GPU on a qualified host.

## Requirements

- ADR-0033 written; amendment notes in ADR-0022 and ADR-0031; portfolio, roadmap, HCL checklist, changelog updated.
- Machine record before purchase: CPU/iGPU generation and PCI ID, NIC PCI ID (I219/I226), DMAR graphics DRHD/RMRR presence (from vendor data or a Linux capture), RAM size (the iGPU uses system memory).
- Firmware pin: file name, GuC API version, SHA-256, `LICENSE.xe` text hash.

## Architecture

Docs only.

## Related Code Files

- Create: `docs/decisions/0033-x86-intel-igpu-native-compute.md`, this plan directory.
- Modify: ADR-0022, ADR-0031 (amendment notes), `.agents/plan-portfolio.md`, `docs/roadmap/{hardware-tracks,current-focus,haily-personal-assistant}.md`, `docs/hardware-compatibility-list.md` §4, `docs/project-changelog.md`.

## Implementation Steps

1. ADR, amendments, registrations (this change).
2. Owner picks the machine; capture its facts under Linux before Cellos work.
3. Pin firmware and record hashes in `research/`.

## Todo List

- [x] ADR-0033 and amendment notes
- [x] Portfolio, roadmap, HCL checklist, changelog
- [x] Machine class approved by the owner (2026-11-04): mini PC on Panther Lake, Wildcat Lake or Lunar Lake with an I219/I226 NIC. Shortlist: [research/machine-candidates.md](research/machine-candidates.md)
- [ ] Exact model selected and procurement approved (owner)
- [ ] Linux capture of the machine (`lspci -nn`, DMAR, `xe` dmesg)
- [x] Firmware pinned: `xe/ptl_guc_70.bin` GuC 70.72.1 ([firmware-pin.md](research/firmware-pin.md))

## Success Criteria

- [ ] An exact machine record with iGPU and NIC PCI IDs and DMAR facts.
- [ ] Firmware file, version and hashes recorded.

## Risk Assessment

- A force-probe platform raises G2 risk (less mature `xe` path); the approved class avoids it.
- None of the shortlisted models has I219, so the NIC is I226 (breadth phase 15 `igc`). Most have no COM port, so HCL R1 waits for breadth phase 10, a COM add-in, or the Avalue EPC-PTL.

## Security Considerations

None beyond recording the firmware licence's no-reverse-engineering clause.

## Next Steps

G1.

## Deviation Log

None.
