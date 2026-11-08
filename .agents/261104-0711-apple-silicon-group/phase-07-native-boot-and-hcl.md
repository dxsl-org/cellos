---
phase: 7
title: "Native boot and HCL qualification"
status: "blocked: hardware (Q3 boot decision and bare-metal installation guide complete; live S2 cold boot pending physical Mac mini M1 hardware per contract)"
priority: P2
effort: "5d + hardware"
dependencies: ["5", "6"]
tier: thinking
checkpoint: true
---

# Phase 07: Native boot and HCL qualification (AS-7)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [HCL §7](../../docs/hardware-compatibility-list.md#7-apple-silicon-aarch64-group-3); [ADR-0032](../../docs/decisions/0032-apple-silicon-secondary-lane.md) §6

## Overview

Boot Cellos natively (no hypervisor) from the internal disk and capture the first Apple HCL row at `S2`.

## Key Insights

- Q3, installed boot path:
  - (a) Asahi installer UEFI environment → m1n1 → U-Boot (EFI) → Limine aarch64 → Cellos. Cellos ships no GPL component and reuses Limine.
  - (b) m1n1 stage 2 with Cellos as the Image payload. Simpler, but the ESP blob combines m1n1 and Cellos.
  - [inference] Recommendation: (a). (b) only for development, never distributed as a combined file.
- Without a hypervisor, the console is the physical s5l UART (USB-C debug adapter) or a framebuffer; record which.

## Requirements

- Q3 decision; for (a), aarch64 Limine boot support in the kernel (FDT from the Limine request).
- Installation guide `docs/baremetal/apple-silicon.md`: installer options, partition layout, recovery.
- HCL §7 row for the exact machine: A1–A8 captures.

## Related Code Files

- Modify: aarch64 boot path (Limine request handling), `docs/baremetal/apple-silicon.md`, `docs/hardware-compatibility-list.md` §7

## Implementation Steps

1. Q3 decision.
2. Implement and test the chosen path (QEMU first for Limine aarch64).
3. Native boot on the bench machine; repeat the phase 04–06 checks.
4. Fill the HCL row with hashes.

## Todo List

- [x] Q3 decision (Path a: Asahi UEFI / Limine aarch64 for installed distribution; Path b: m1n1 proxy for dev)
- [x] Boot path & installation guide (`docs/baremetal/apple-silicon.md`)
- [ ] Native re-run of 04–06 (S2 pending physical hardware)
- [ ] HCL row (S2 pending physical hardware)

## Success Criteria

- [ ] S2: native cold boot to shell, all cores, DHCP, storage persistence across reboot, macOS still boots; HCL §7 row filled.

## Risk Assessment

- Boot policy changes are reversible only from 1TR. Record every step.

## Security Considerations

There is no measured or signed Cellos boot on Apple; the machine pin is change detection only (same as HCL R3).

## Next Steps

Phases 08 and 09.

## Deviation Log

None.
