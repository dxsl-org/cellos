---
phase: 10
title: "Pin and Wire x86 Tier 3 VirtIO Parity"
status: blocked
priority: P2
effort: "4d"
dependencies: [1, 6, 9]
tier: thinking
---

# Phase 10: Pin and Wire x86 Tier 3 VirtIO Parity

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `.agents/TODO.md:64-75`
- `cells/services/hypervisor/src/run_loop.rs`
- `cells/services/hypervisor/src/run_loop_x86.rs`
- `cells/services/hypervisor/src/virtio_blk.rs`
- `cells/services/hypervisor/src/virtio_net.rs`

## Overview

Define and wire the x86 guest's block/network device transport to reuse the Phase 09 backend contracts instead of leaving those personalities ARM64-only.

## Key Insights

The strict x86 guest boots from initramfs, while `run_loop_x86.rs` handles legacy ports and does not dispatch the shared VirtIO-MMIO block/network personalities used by ARM64. The transport mapping must be pinned before implementation.

## Requirements

- Freeze one x86 guest transport and address/interrupt map; do not expose two competing device models.
- Reuse the shared Phase 09 block backend and existing network backend; no per-architecture protocol fork.
- Preserve pinned Alpine strict-boot behavior and QEMU-TCG 10.2.0 selection.
- Bound MMIO/PIO decoding, queue configuration, descriptor access, interrupts, and backend failure.
- Keep physical AMD/Intel qualification external.

## Architecture

`x86 guest transport → shared VirtioDevice/virtqueue → persistent block adapter or net backend → Cellos services`. Architecture-specific code owns only transport decode and interrupt injection.

## Assumptions

- **Claim:** A fixed VirtIO-MMIO transport is acceptable for the current minimal x86 guest.
  **Confidence:** low
  **How to verify:** inspect the pinned guest kernel configuration and boot command line; if it requires PCI, approve a separate PCI transport contract rather than emulating both.
- **Claim:** The existing interrupt model can inject the selected transport's block/network interrupts.
  **Confidence:** medium
  **How to verify:** trace APIC/PIC handling and pin the exact vectors before Build.

## Related Files

- Modify after contract approval: `cells/services/hypervisor/src/run_loop_x86.rs`
- Modify: x86 boot/configuration and fixed device-map code
- Reuse: `virtio_mmio.rs`, `virtqueue.rs`, `virtio_blk.rs`, `virtio_net.rs`
- Modify: focused x86 strict-boot, block, network, and persistence scenarios
- Emit: evidence to Phase 08; consume Phase 06 runners without taking their ownership

## Implementation Steps

1. Prove the pinned x86 guest's supported VirtIO transport and select exactly one mapping.
2. Freeze transport addresses, feature bits, interrupt vectors, and boot parameters in a reviewed child contract.
3. Route x86 exits to the existing shared block/network personalities; add only architecture-specific decode/injection glue.
4. Connect block to Phase 09 persistence and network to the existing bounded net backend.
5. Exercise strict boot, sector read/write/FLUSH/reboot/read, DHCP or bounded network smoke, malformed queues, reset, and backend unavailability.
6. Confirm ARM64 behavior remains unchanged and no duplicate block/network state machine exists.
7. Run the Phase 06 hostile runners, emit x86 QEMU parity evidence, and keep physical x86 independently gated.

## Todo List

- [ ] Pin one x86 VirtIO transport and interrupt map.
- [ ] Reuse shared block/network personalities without protocol forks.
- [ ] Prove strict boot plus bounded block/network behavior on QEMU 10.2.0.

## Success Criteria

- [ ] x86 strict guest discovers the selected block/network devices and still reaches `/bin/sh`.
- [ ] Persistent block FLUSH/reboot/read semantics match Phase 09.
- [ ] Network traffic uses the shared backend with no hard-coded QEMU-only identity leak.
- [ ] Malformed transport/queue inputs fail without host panic or cross-guest/service corruption.
- [ ] No physical AMD/Intel qualification claim is added.

## Security Considerations

Treat every guest transport field, GPA, queue index, descriptor, length, and interrupt request as hostile. Device-map collisions fail closed.

## Risk Assessment

If the pinned guest requires VirtIO-PCI, this phase needs a separately approved transport design and may exceed the estimate. Do not silently substitute ad hoc port I/O or duplicate ARM64 logic.

## Next Steps

After x86 parity passes, run the Phase 06 scenario suite for parity evidence; physical x86 remains independently gated.

## Deviation Log

- Blocked on Phase 09's shared persistent-backend contract and on the unsupported Phase 06 hostile-input scenarios. No x86 transport is pinned or emulated before those dependencies provide a stable contract.
