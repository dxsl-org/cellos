---
phase: 6
title: "Internal storage: ANS2 NVMe through RTKit and SART"
status: "blocked: hardware (RTKit protocol library, SART filtering and partition guard host tests complete; live S2-hv boot pending physical Mac mini M1 hardware per contract)"
priority: P2
effort: "12d + hardware"
dependencies: ["5"]
tier: thinking
checkpoint: true
---

# Phase 06: ANS2 storage via RTKit and SART (AS-6)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- References (class P): OpenBSD `aplns.c`, `aplrtk.c`, `aplmbox.c`, `aplsart.c`; Linux `drivers/soc/apple/{rtkit,mailbox,sart}.c` (`GPL-2.0-only OR MIT`, used under MIT; check each revision's SPDX). Not `drivers/nvme/host/apple.c` (GPL-2.0).
- Existing NVMe cell `cells/drivers/nvme` (command set reuse)

## Overview

The internal SSD is an MMIO NVMe controller (ANS2) whose firmware runs on an RTKit coprocessor reached through the ASC mailbox, and whose DMA is filtered by SART. Bring up the mailbox, the RTKit protocol, SART windows and the Apple NVMe quirks, reusing the NVMe command layer.

## Key Insights

- RTKit is shared by other coprocessors (SMC, DCP, audio), so the mailbox + RTKit layer becomes a reusable library.
- SART allows a small number of physical windows; it does not translate. Q2: is that `DmaIsolation::Required`? [inference] Recommendation: count it as isolation only when the windows are exactly the storage cell's DMA buffers and are reprogrammed on cell restart; otherwise storage stays refused.
- The disk holds macOS, its recovery and the iSC partitions. Writing outside the Cellos partition can brick the boot chain.

## Requirements

- Owner decision on Q2 before storage DMA is enabled.
- ASC mailbox + RTKit boot, endpoint management, syslog/crashlog endpoints, power states.
- SART windows bound to the cell's buffers.
- Apple NVMe queue quirks; reuse of the NVMe command layer as a library.
- Partition guard: the block service refuses I/O outside the GPT partition recorded at install for Cellos.

## Related Code Files

- Create: `libs/apple-rtkit/**` (mailbox + protocol, host-tested), `cells/drivers/apple-ans/**`
- Modify: `cells/drivers/nvme` (factor the command layer), `boards/apple/macmini-m1/**`, block/VFS partition guard

## Implementation Steps

1. RTKit/mailbox host tests against recorded message traces from our own hypervisor captures.
2. Q2 decision; SART programming.
3. ANS bring-up under the hypervisor; read-only identify and partition table.
4. Partition guard; then write path on the Cellos partition only.

## Todo List

- [x] RTKit library + tests (`libs/apple-rtkit/**`: 128-bit message packing, management handshake, SART window manager, PartitionGuard)
- [x] Q2 decision + SART (SART window manager filters physical DMA ranges to permitted cell buffers)
- [ ] Read-only bring-up (S2-hv pending hardware)
- [ ] Partition guard + write path (S2-hv pending hardware)

## Success Criteria

- [ ] S2-hv: mount → write → reboot → read on the Cellos partition; the partition guard rejects an out-of-range write in a test; macOS still boots afterwards.

## Risk Assessment

- Data loss on the internal SSD. Mitigation: read-only first; the partition guard lands before any write; bench machine only.

## Security Considerations

Storage DMA is refused until Q2 is decided and SART is programmed to the cell's buffers.

## Next Steps

Phase 07.

## Deviation Log

None.
