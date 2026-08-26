# Phase 07 — virtio-blk → VFS Cell → mount rootfs (M3)

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-06](phase-06-virtio-mmio-x86.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-07-virtio-blk-vfs.md`
- Verified: `cells/services/hypervisor/src/virtio_blk.rs:1-...` (arch-generic virtio-blk device model —
  reused), `run_loop.rs:83` (blk slot 1 dispatch already generic); VFS Cell via `sys_lookup_service`.

## Overview
- **Priority:** P1 · **Status:** pending · **Depends on:** 06
- Attach a **virtio-blk** device (slot 1) whose backend forwards sector I/O to the ViCell **VFS Cell**,
  so the guest mounts a real rootfs from a ViCell-hosted disk image. Reuses the arch-generic
  `virtio_blk.rs` unchanged — the only x86-new work is wiring the slot into the x86 run loop and cmdline.
  Success = **M3**: Alpine mounts `/dev/vda` and runs from a disk rootfs.

## Key Insights
- **Fully reused device model:** `virtio_blk.rs` is arch-generic; `run_loop.rs:83` already dispatches
  blk on slot 1 through `blk_vmio.mmio_write(...)`. The x86 run loop routes `MmioRead`/`MmioWrite` in the
  virtio window to the same code. No new device logic.
- **VFS Cell backend (existing pattern):** the blk backend resolves the VFS Cell via
  `sys_lookup_service(service::VFS)` and issues read/write-sector IPC — the same forwarding the ARM P07
  used. Sector I/O crosses the cell boundary with owned buffers (Law 2).
- **cmdline:** add `virtio_mmio.device=0x1000@0xd0001000:6` (slot 1, IRQ6) and `root=/dev/vda
  rootfstype=ext4 rw` (drop `rdinit=/bin/sh` once a real root exists). Keep initramfs as the early
  environment that pivots to `/dev/vda`.
- **Backpressure + budget (C-x2):** blk I/O to VFS happens while the guest may be spinning; the
  `Preempted` yield point (P03/P05) ensures VFS Cell stays responsive during large transfers (analog of
  the ARM C2 concern that made blk/net possible at all).

## Requirements
**Functional**
- virtio-blk slot 1 wired into the x86 virtio-mmio window with an 8259 IRQ.
- Backend forwards sector read/write to the VFS Cell (reuse existing IPC path); owned buffers.
- Guest cmdline mounts `/dev/vda`; a ViCell-hosted disk image is exposed as the block backend.

**Non-functional**
- Law 2 owned buffers across IPC; Law 4 cell `#![forbid(unsafe_code)]`.

## Architecture
```
guest mount /dev/vda → virtio-blk slot 1 QueueNotify
  → run_loop MmioWrite @0xd0001000 → blk_vmio → virtio_blk.process()
  → backend: sys IPC to VFS Cell (read_sector/write_sector, Box<[u8]>)
  → used-buffer ready → pic.deliver(IRQ6) → sys_inject_irq
```

## Related Code Files
**Modify**
- `cells/services/hypervisor/src/run_loop.rs` — ensure blk slot 1 routed in x86 arms + IRQ delivery
- `cells/services/hypervisor/src/boot_info.rs` — blk `virtio_mmio.device=` entry + `root=` cmdline
- blk backend module — confirm VFS-Cell forwarding is arch-neutral (should already be)
**Reuse unchanged**
- `virtio_blk.rs`, `virtqueue.rs`
**Verify**
- P02 virtio window has a slot-1 sub-range unmapped; ViCell disk image available to VFS Cell

## Implementation Steps
1. Wire slot 1 (blk) in the x86 run-loop virtio dispatch + assign IRQ6.
2. Confirm the blk→VFS backend IPC is arch-neutral; adjust only if it referenced ARM specifics.
3. Add blk cmdline entry + `root=/dev/vda rootfstype=ext4 rw` in `boot_info`.
4. Provide a ViCell-hosted rootfs disk image for the VFS Cell to serve.
5. Boot: initramfs → mount `/dev/vda` → pivot root → shell from disk rootfs.

## Todo List
- [ ] virtio-blk slot 1 wired in x86 run loop + IRQ6
- [ ] blk→VFS backend confirmed arch-neutral (owned buffers)
- [ ] blk cmdline + root= entry in boot_info
- [ ] ViCell-hosted rootfs image served by VFS Cell
- [ ] **Alpine mounts /dev/vda and runs from disk rootfs (M3)**

## Success Criteria
- Guest boots, mounts `/dev/vda`, and `df` shows the disk rootfs; a file written in the guest persists
  across reads (round-trips through the VFS Cell).
- Large sequential read does not starve the VFS Cell (Preempted yield keeps it responsive).
- No regression to console (P06) or serial (P05).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| blk backend had latent ARM assumptions | Low×Med | Review backend for arch specifics; it forwards to VFS generically |
| VFS Cell starvation on large I/O | Med×Med | Preempted budget yield (C-x2); chunk transfers |
| rootfs image format mismatch (ext4 vs guest) | Low×Med | Build image with a guest-supported fs; verify mount |
| IRQ6 collision with console/net slots | Low×Med | Distinct IRQ per slot (5/6/7); document map |

## Security Considerations
- Every virtqueue descriptor GPA is bounds-checked against the carve before the backend reads/writes
  guest memory (reuse virtqueue validation).
- The guest's disk is a ViCell-mediated image via the VFS Cell — no raw host block device exposure.

## Next Steps
- P08 (virtio-net) may run in parallel; P10 CI matrix asserts blk mount.
