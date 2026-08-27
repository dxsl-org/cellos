---
phase: 9
title: "Implement ARM64 Tier 3 Persistent QEMU Storage"
status: blocked
priority: P1
effort: "4d"
dependencies: [1, 6]
tier: thinking
---

# Phase 09: Implement ARM64 Tier 3 Persistent QEMU Storage

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `.agents/TODO.md:64-75`
- `.agents/260821-0642-app-tiers-completion/phase-04-tier3-qualification.md`
- `cells/services/hypervisor/src/virtio_blk.rs`
- `cells/services/hypervisor/src/run_loop.rs`

## Overview

Replace the ARM64 QEMU guest's 4 MiB volatile heap disk with a bounded persistent backing path and prove write/flush/reboot/read behavior without claiming physical qualification.

## Key Insights

`virtio_blk.rs` currently stores every sector in a `Vec<u8>` and acknowledges FLUSH without durability. This is an in-repo software gap, not an external dependency.

## Requirements

- Use the existing ARM64 VirtIO-MMIO block personality as the reference lane.
- Back it with one exact policy-approved persistent file or volume; the guest never selects the host path.
- Bound capacity, sector arithmetic, queue work, I/O size, retries, and memory use.
- Define FLUSH success as durable completion from the backing service; errors return `VIRTIO_BLK_S_IOERR`.
- Preserve the volatile backend only as an explicit development/test profile, never as persistence evidence.

## Architecture

`guest virtio-blk → bounded descriptor parser → persistent block adapter → Cellos VFS/block service → mounted QEMU disk image`. The adapter owns serialization and maps service errors to VirtIO status.

## Assumptions

- **Claim:** The mounted VFS path supports bounded grant-backed writes and commit-before-acknowledge.
  **Confidence:** high
  **How to verify:** reconcile `.agents/260825-sdk-delivery/phase-04-vfs-grant-write.md` with current service code before Build.
- **Claim:** One fixed ARM64 QEMU volume is sufficient for the first persistence contract.
  **Confidence:** medium
  **How to verify:** approve the exact path, capacity, image lifecycle, and cleanup policy before implementation.

## Related Files

- Modify: `cells/services/hypervisor/src/virtio_blk.rs`
- Modify: `cells/services/hypervisor/src/main.rs`, `run_loop.rs`
- Modify: hypervisor manifest/policy and image-generation scripts only for the exact backing path
- Create/modify: focused ARM64 QEMU persistence scenario
- Emit: evidence to Phase 08; expose the backend contract to Phase 10 and consume Phase 06 runners

## Implementation Steps

1. Pin exact backing path, capacity, access policy, image creation, and cleanup contract.
2. Replace direct `Vec` ownership with a bounded persistent adapter while retaining explicit volatile-profile selection.
3. Implement checked sector reads/writes and durable FLUSH; reject partial, overflowed, out-of-range, or service-failed requests.
4. Keep descriptor validation and guest-memory copying bounded; avoid per-request whole-volume allocation.
5. Boot ARM64 QEMU, write a unique marker, FLUSH, terminate the VM, restart from the same backing image, and read the marker.
6. Inject backing unavailable, short I/O, full volume, failed FLUSH, malformed descriptors, and restart during write.
7. Run the reusable Phase 06 hostile/recovery scenarios, emit persistence evidence, and expose the backend contract to Phase 10.

## Todo List

- [ ] Approve exact persistent volume policy and lifecycle.
- [ ] Implement bounded read/write/FLUSH semantics.
- [ ] Prove ARM64 QEMU write/flush/reboot/read and failure recovery.

## Success Criteria

- [ ] Data acknowledged after FLUSH survives a fresh VM/service restart using the same image.
- [ ] Failed or partial durability returns IOERR and is never reported as persisted.
- [ ] Out-of-range and malformed requests cannot access another file, volume, or guest range.
- [ ] Volatile mode produces no persistence evidence or qualification claim.

## Security Considerations

The backing path is host-policy-owned, not guest-controlled. Enforce exact volume authority, size ceilings, offset checks, and no path traversal.

## Risk Assessment

VFS acknowledgement may not imply media durability. If no durable commit primitive exists, stop at a documented service contract gap rather than treating close/write acknowledgement as FLUSH.

## Next Steps

After ARM64 QEMU persistence passes, expose the backend contract to Phase 10 and retain Phase 06 as runner owner.

## Deviation Log

- Decision: the user approved a dedicated `build/tier3-arm64-persistent.img` fixed at 8 MiB, created once, reused across restart evidence, and removed only by an explicit cleanup command. The guest never selects the path. Implementation remains blocked only on supported Phase 06 hostile scenarios.
