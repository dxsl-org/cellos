---
phase: 19
title: "Intel X710/XL710 (i40e family)"
status: "blocked: hardware (driver code and host tests complete; capture pending physical machine)"
priority: P3
effort: "8d + physical capture"
dependencies: ["4b"]
tier: thinking
checkpoint: false
---

# Phase 19: Intel X710 (i40e)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Intel Ethernet Controller X710/XXV710/XL710 datasheet (Admin Queue chapter); FreeBSD `ixl(4)` (BSD) shared code

## Overview

The X710 family (`8086:1572` SFP+, XL710 QSFP+, X710-T variants) is common in current Intel servers. New `cells/drivers/i40e` cell.

## Key Insights

- Almost everything goes through the firmware Admin Queue (AQ): link, MAC address, switch/VSI configuration, and queue context via the HMC (host memory cache). It is the largest NIC phase.
- The firmware/NVM version gates AQ API versions. Record the NVM version in evidence and refuse an unsupported AQ major version.
- Ports are PFs. Use the first PF and its default VSI; one Rx/Tx queue pair.

## Requirements

- Functional:
  - Identity `1572` (+ captured variants) as `Target` → `Driveable` after capture.
  - PF reset, AQ init (ATQ/ARQ rings via `GrantDma`), get version, get MAC, get switch config, HMC setup for one LAN Rx/Tx queue, VSI default config, link status via AQ events, MSI-X (AQ + one queue vector).

## Assumptions

- **Claim:** the default VSI suffices for single-queue operation without adding switch rules. **Confidence:** medium. **How to verify:** datasheet AQ "get switch config" semantics + capture.
- **Claim:** FreeBSD `ixl(4)` shared code is BSD-licensed. **Confidence:** high. **How to verify:** headers.

## Related Code Files

- Create: `cells/drivers/i40e/**`
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (+ tests), `boards/**`, ISO scripts, CI build list, `NOTICE`

## Implementation Steps

1. Identity + AQ ring machinery with host tests against a fake AQ responder.
2. HMC + queue contexts with host tests (context encoding).
3. VSI/link/MAC; data path.
4. Physical capture.

## Todo List

- [X] AQ
- [X] HMC/queue contexts
- [X] VSI/link/data path
- [ ] Physical capture (pending physical machine capture per contract)

## Success Criteria

- [X] Host tests: AQ descriptor encoding, command/response matching, HMC context encoding.
- [ ] Physical capture per contract with the NVM version recorded; otherwise `blocked: hardware`.

## Risk Assessment

- NVM/AQ version drift between cards. Mitigation: refuse unsupported AQ majors by name.

## Security Considerations

The AQ is a privileged firmware channel. Only the commands listed above are sent; no NVM writes.

## Next Steps

Phase 20 (server, optional row).

## Deviation Log

None.
