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

The x86 guest now uses one pinned VirtIO-MMIO map and the shared block/network personalities. The dedicated `/virtio-e2e` init supplies bounded device evidence, while the normal pinned Alpine path separately retains strict `/bin/sh` boot evidence; the success claim is therefore aggregate QEMU evidence, not a shell reached by the dedicated evidence init.

## Requirements

- Freeze one x86 guest transport and address/interrupt map; do not expose two competing device models.
- Reuse the shared Phase 09 block backend and existing network backend; no per-architecture protocol fork.
- Preserve pinned Alpine strict-boot behavior and QEMU-TCG 10.2.0 selection.
- Bound MMIO/PIO decoding, queue configuration, descriptor access, interrupts, and backend failure.
- Keep physical AMD/Intel qualification external.

## Architecture

`x86 guest transport → shared VirtioDevice/virtqueue → persistent block adapter or net backend → Cellos services`. Architecture-specific code owns only transport decode and interrupt injection.

## Assumptions

- **Verified QEMU boundary:** The pinned guest discovers the fixed VirtIO-MMIO block and network mapping under QEMU-TCG 10.2.0; this does not select VirtIO-PCI or qualify physical hardware.
- **Verified QEMU boundary:** IRQ5 and IRQ6 are delivered through the existing interrupt model, with completion acknowledged by the guest.

## Related Files

- Modified: `cells/services/hypervisor/src/run_loop_x86.rs`
- Modified: x86 boot/configuration and fixed device-map code
- Reused: `virtio_mmio.rs`, `virtqueue.rs`, `virtio_blk.rs`, `virtio_net.rs`
- Modified: focused x86 strict-boot, block, network, and persistence scenarios
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

- [x] Pin one x86 VirtIO-MMIO transport: block `0xd0000000`/IRQ5, net `0xd0000200`/IRQ6.
- [x] Reuse shared block/network personalities and Phase 09 persistent-disk adapter without protocol forks.
- [x] Prove strict boot plus bounded block/network behavior on pinned QEMU 10.2.0.

## Success Criteria

- [x] Aggregate evidence shows that the normal pinned x86 strict guest reaches `/bin/sh` and the dedicated evidence guest discovers the selected block/network devices.
- [x] Persistent block FLUSH/reboot/read semantics match Phase 09.
- [x] Network traffic uses the shared backend with a distinct nested MAC and no hard-coded QEMU-only identity leak.
- [ ] Malformed transport/queue inputs fail without host panic or cross-guest/service corruption.
- [x] No physical AMD/Intel qualification claim is added.

## Security Considerations

Treat every guest transport field, GPA, queue index, descriptor, length, and interrupt request as hostile. Device-map collisions fail closed.

## Risk Assessment

The bounded two-boot path is proved only under pinned QEMU-TCG 10.2.0. Malformed transport/queue coverage and full Phase 06 hostile parity remain required before this phase can close; physical AMD/Intel qualification remains independent.

## Next Steps

Add malformed transport/queue evidence and run the supported Phase 06 hostile scenarios for parity. Keep physical x86 independently gated.

## Deviation Log

- Implemented the single VirtIO-MMIO mapping, architecture-correct PIC vector injection, shared block/network dispatch, and shared `/mnt/sd/guest_disk.img` persistence lookup. Both hypervisor targets and the x86 kernel compile.
- Queue completion IRQs are level-like rather than one-shot: HLT/Preempted retries one prioritized deliverable slot (UART, block, net, then PIT) while the VirtIO ISR bit remains pending; guest ACK clears it. Successful asynchronous net RX now sets the ISR bit. The focused ACK/retry test passes.
- Split the newly added x86 IRQ, MMIO, and legacy port dispatch plus the MMIO address/test helpers into focused kebab-case modules; those x86/MMIO files are below 200 lines. Pre-existing touched legacy files (`main.rs`, ARM `run_loop.rs`, and `virtio_blk.rs`) remain above the repository target and are not claimed as remediated by this split.
- The dedicated NPF/MMIO fixture now passes on pinned QEMU-TCG 10.2.0 and the outer Cellos x86 host still reaches `Cellos >`. QEMU reports the architectural final-translation bit `EXITINFO1[32]` for the fixture's direct MMIO access; the decoder requires that bit and rejects instruction fetches, out-of-window GPAs, and the guest-page-walk bit `EXITINFO1[33]`. It also rejects RSP operands because RSP is VMCB-owned, decodes a complete instruction prefix at the guest-RAM boundary, and leaves ASID allocation permanently exhausted rather than wrapping. `create_vcpu` no longer mutates guest memory under `test-hooks`; each smoke owns its fixture blob. QEMU 8.2.2 remains a known-incompatible runtime and still cannot provide parity evidence.
- At that checkpoint, the fixture closed only the executable MMIO decode/GPR/RIP prerequisite; strict guest block/network discovery and persistence had not yet been exercised, and malformed transport/queue evidence plus Phase 06 parity remained open.
- The full rebuilt runner passed in 79.40 seconds with `BOOT_WINDOW=180 QEMU_X86_BIN='/mnt/c/Program Files/qemu/qemu-system-x86_64.exe' bash scripts/qemu-x86-virtio-e2e.sh`: two fresh outer boots under QEMU-TCG 10.2.0 used one persistent 16 MiB backing and required explicit `[vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT)` evidence.
- Both boots reported block/network discovery, IRQ5/IRQ6, and shared network TX/RX with a distinct nested MAC. Run 1 reported block write/FLUSH and `VIRTIO_E2E_FIRST_RUN_PASS`; run 2 reported block readback and `VIRTIO_E2E_SECOND_RUN_PASS`.
- The E2E image selects its dedicated evidence init with `/virtio-e2e`; normal pinned strict boot and shell evidence remains separate. Linux modern VirtIO networking uses the 12-byte `virtio_net_hdr_v1`: TX strips 12 bytes, RX prepends 12 bytes, and RX sets `num_buffers=1`.
- Outer persistence required NVMe FLUSH to use namespace ID 1, the actual namespace, rather than invalid namespace ID 0.
- The reviewed active-IOMMU boundary instantiates `intel-iommu`, rejects DMA mapping unless isolation is active and the backend confirms the map, rolls back exactly one pin hold on failure, and enables BME and records quota only after mapping succeeds.
- This evidence ceiling is QEMU only. Malformed transport/queue inputs and full Phase 06 hostile parity remain open, so Phase 10 stays blocked; no physical AMD/Intel, service, or production qualification is claimed.
