# Phase 01 — Wiring & Capabilities

## Context Links
- Plan: [plan.md](plan.md)
- MMIO transport: `cells/services/hypervisor/src/virtio_mmio.rs:22-36` (VirtioDevice trait), `:170-186` (base/slots), `:52-150` (register dispatch).
- Run loop slot dispatch: `cells/services/hypervisor/src/run_loop.rs:34-40` (device constructors), `:83-88` / `:109-113` (slot match).
- Guest DTB: `cells/services/hypervisor/src/dtb.rs:164-189` (existing virtio_mmio nodes, slot→SPI mapping).
- Manifest + allowlist: `cells/services/hypervisor/src/main.rs:12-46`.
- Grant syscalls: `libs/api/src/abi/syscall.rs:200-235` (GrantCap bit 39).

## Overview
- **Priority:** P1 (foundation for all Track A phases)
- **Status:** complete (implementation; guest bind proof remains in phase 05)
- **Description:** Register the virtio-gpu device as MMIO slot 3, advertise it to the guest via a
  new DTB node (SPI 19), grant the hypervisor Cell the Grant capabilities it needs, and stand up
  `virtio_gpu.rs` (correct device_id, config space at pinned offsets, 0 feature bits, controlq that
  answers the init-time `GET_DISPLAY_INFO` with a REAL 280 B response so `card0` registers with a
  usable output; empty cursorq). Also lands the compositor owner-death cleanup prerequisite (C6).
  No guest pixels yet — the resource/transfer/flush path is phases 02-03.

## Key Insights
- Slot→SPI is `SPI = 16 + slot` (console slot0=SPI16, blk slot1=SPI17, net slot2=SPI18 →
  dtb.rs:169/178/187). GPU slot3 = **SPI 19**, MMIO region `0x0a000600` (base + slot*stride,
  virtio_mmio.rs:171-172, stride 0x200).
- `MAX_QUEUES = 2` (virtio_mmio.rs:7) exactly fits virtio-gpu's controlq(0) + cursorq(1). No
  transport change needed.
- The VMM already owns arbitrary guest GIC SPI injection via `vmm::inject_irq(vm_id, vcpu_id, 19)`
  (vmm.rs:121-123). **No new kernel syscall or SPI allocation** — SPI 19 is guest-internal.
- Grant syscalls (`GrantRegister=215`, `GrantShare=209`, `GrantSlice=210`, `GrantUnregister=216`)
  gate on GrantCap allowlist bit 39. **Enabling GrantCap exposes ALL SIX grant ops, not four** —
  `GrantAlloc=208`, `GrantShare`, `GrantSlice`, `GrantFree=211`, `GrantRegister`, `GrantUnregister`
  all map to bit 39 (syscall.rs:528-533), even though the ABI docs for `GrantSlice`/`GrantFree`
  (syscall.rs:206-212) omit the bit-39 note the others carry. Adding the needed ops to
  `declare_syscalls!` turns on bit 39, which grants the whole set; the security review accounts for
  the Cell also gaining `GrantAlloc`/`GrantFree`.
- `config_read(offset)` returns `u32` at 4-byte granularity (virtio_mmio.rs:33,71). Pin the
  `virtio_gpu_config` field offsets: `events_read@0, events_clear@4, num_scanouts@8,
  num_capsets@12`. Return `num_scanouts=1` at **offset 8** (not 0), and 0 for every other offset.
  Assert the offsets in comments (contract, not code).
- **Offer ZERO device feature bits** (`device_features_lo()==0`). Assert and comment the invariant:
  `process_notify` (virtqueue.rs:21-23,62-85) walks only NEXT/WRITE descriptor chains, NOT
  INDIRECT or EVENT_IDX — offering any low feature bit would let the guest negotiate indirect
  descriptors and corrupt the chain walk.
- **GET_DISPLAY_INFO must return a real payload from this phase.** The driver's init-time
  GET_DISPLAY_INFO (before any resource exists) needs the full `virtio_gpu_resp_display_info`
  (24 B hdr + 16×16 B pmodes = 280 B, `pmodes[0].enabled=1`, fixed 1024×768) or it gets zero usable
  outputs. A NODATA reply is not enough. (The encoder is shared with phase 02; cursor stays phase
  04.)

## Requirements
**Functional**
1. `virtio_gpu.rs` implements `VirtioDevice`: `device_id()=16` (VIRTIO_GPU),
   `device_features_lo()==0` (assert + comment; pure 2D, no INDIRECT/EVENT_IDX), `config_read`
   returning `num_scanouts=1` at **offset 8** and 0 elsewhere, `notify()` that drains queues.
2. GET_DISPLAY_INFO returns the real `virtio_gpu_resp_display_info` (280 B, `pmodes[0].enabled=1`,
   1024×768) so the driver's init-time probe gets a usable output. (Shared encoder with phase 02.)
3. `run_loop.rs` constructs a `GpuDev` + its own `VirtioMmio`, dispatches slot 3 read/write.
4. `dtb.rs` emits `/virtio_mmio@a000600` with `interrupts = <0 19 1>`, `reg = <0 0x0a000600 0 0x200>`.
5. `main.rs` adds the Grant syscalls to `declare_syscalls!` (turns on GrantCap bit 39 = all six grant ops).
6. **Compositor owner-death cleanup prerequisite (see prerequisite block below).**

**Non-functional**
- File `virtio_gpu.rs` < 200 LOC; split command tables into `virtio_gpu/` submodules in phase 02.
- All feature-bit / config-offset constants named and commented (contract, not code).

## Architecture
Data flow (probe path): guest DRM driver writes MMIO handshake → `VirtioMmio::mmio_write`
(slot 3) → feature negotiation (device offers 0 low bits) → sets up controlq/cursorq → reads config
space (`config_read`, num_scanouts=1 @ offset 8) → issues `GET_DISPLAY_INFO` on controlq →
`notify(0,...)` → **real 280 B `resp_display_info` (single enabled scanout, 1024×768)** so the driver
registers `card0` with a usable output. This phase wires the whole probe path AND the real
GET_DISPLAY_INFO response; the full resource/transfer command set is phase 02.

### Prerequisite — compositor owner-death cleanup (red-team C6, security)
Before the VMM is wired as a compositor client (phase 03), the compositor's owner-death cleanup must
be REAL, or a dead VMM cell's freed Grant frame (reused by another cell under SAS frame-identity)
gets painted by the compositor — an LBI-boundary break. The primitive exists as dead code:
`SurfaceTable::caps_owned_by(tid)` (surface_table.rs:240-247) and the cached Grant pointer stored by
`attach_grant` (main.rs:290-298; `PixelSource::Grant { ptr, reg_id }`). Wire a `NotifyOnExit` handler
in the compositor that, on a client cell's death, calls `caps_owned_by(dead_tid)` then `remove(cap)`
per cap (dropping the cached pointer and the reg_id). This is idempotent with the normal
DETACH_GRANT + DESTROY_SURFACE path. The VMM-side `NotifyOnExit` registration is a phase-03 step.

## Related Code Files
- **Create:** `cells/services/hypervisor/src/virtio_gpu.rs` (device model + config space + real
  GET_DISPLAY_INFO), `cells/services/hypervisor/src/virtio_gpu/command.rs` (the shared display-info
  encoder used here and in phase 02).
- **Modify:** `cells/services/hypervisor/src/run_loop.rs` (construct + slot-3 dispatch),
  `cells/services/hypervisor/src/dtb.rs` (GPU node), `cells/services/hypervisor/src/main.rs`
  (module decl + Grant syscalls in allowlist).
- **Modify (compositor prerequisite, C6):** `cells/services/compositor/src/main.rs` (NotifyOnExit
  handler), `cells/services/compositor/src/surface_table.rs` (use `caps_owned_by` + `remove`; drop
  the `#[allow(dead_code)]` on `caps_owned_by` and on `PixelSource::Grant.reg_id` at :22).

## Implementation Steps
1. Add `#[cfg(target_arch="aarch64")] mod virtio_gpu;` to main.rs; add `GrantRegister`,
   `GrantShare`, `GrantSlice`, `GrantUnregister` to `declare_syscalls!` (this turns on GrantCap
   bit 39 = all six grant ops — note in the security review).
2. Write `virtio_gpu.rs`: `GpuDev` struct (queue counters), `VirtioDevice` impl with
   `device_id()=16`, `device_features_lo()==0` (asserted + commented), config space
   (num_scanouts=1 @ offset 8, 0 elsewhere), `notify()` that runs `process_notify`.
3. `command.rs`: encode the real `virtio_gpu_resp_display_info` (280 B, one enabled 1024×768
   scanout); GET_DISPLAY_INFO returns it (init-time probe needs a usable output).
4. Wire run_loop.rs: `let mut gpu = GpuDev::new(); let mut gpu_vmio = VirtioMmio::default();` and
   add `3 => gpu_vmio.mmio_write/read(..., &mut gpu, ...)` to both slot matches.
5. Add the DTB node in dtb.rs after the net node.
6. **Compositor prerequisite (C6):** add a `NotifyOnExit` handler in compositor `main.rs` that calls
   `SurfaceTable::caps_owned_by(dead_tid)` → `remove(cap)` per cap.
7. Build the hv-arm image, boot Alpine with the virtio-gpu kernel module; confirm the driver
   binds and `/dev/dri/card0` appears (dmesg `virtio_gpu` + `[drm]` lines).

## Todo List
- [x] main.rs: module decl + Grant syscalls (GrantCap = all six ops)
- [x] virtio_gpu.rs device model (device_id, features_lo==0, config space @ correct offsets)
- [x] command.rs: full 408 B GET_DISPLAY_INFO response (16 scanout entries, 1 enabled)
- [x] run_loop.rs slot-3 construction + dispatch
- [x] dtb.rs GPU node (SPI 19, 0x0a000600)
- [x] compositor: NotifyOnExit → caps_owned_by + remove (C6 prerequisite)
- [ ] Boot Alpine, confirm driver bind + /dev/dri/card0

## Success Criteria
- Guest dmesg shows `virtio_gpu` probe succeeding and **`card0` registers with a usable output**
  (GET_DISPLAY_INFO returned a real payload); no guest panic, no VMM "unknown MMIO" logs for the
  0x0a000600 region. (Usable modeset/scanout drawing is demonstrated in phase 02/03, once resources
  and the flush path exist.)
- Compositor NotifyOnExit cleanup wired and unit-exercised (a client death drops its cached Grant).
- `cargo build` clean; hypervisor allowlist shows GrantCap enabled.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Guest driver refuses to bind due to a feature bit it expects | M×H | Target: 0 device features (to-verify-at-impl; offering any low bit risks INDIRECT/EVENT_IDX negotiation, m2). Fall back to offering EDID=0 only if the driver requires it. |
| Config-space offset/size mismatch → driver reads garbage num_scanouts | M×M | Match `struct virtio_gpu_config` layout exactly (to-verify-at-impl — offsets `events_read@0, events_clear@4, num_scanouts@8, num_capsets@12`); assert offsets in comments. |
| DTB node malformed → silent guest hang (per dtb.rs:4 warning) | L×H | Mirror the exact net-node property set; validate with a guest `dmesg` DTB dump. |

## Security Considerations
- Adding GrantCap widens the hypervisor Cell's authority, and **enabling bit 39 grants all six grant
  ops** (GrantAlloc/GrantFree too — syscall.rs:528-533), not just the four in the declare list. Flag
  as an **interface/security-review item**: the Cell already holds HypervisorCap (the highest-trust
  capability), so GrantCap is not a meaningful escalation, but the review must confirm the Cell still
  cannot grant-share to arbitrary third parties in a way that bypasses the compositor ownership check
  (it shares read-only to COMPOSITOR_ENDPOINT only — phase 03) and must note the GrantAlloc/GrantFree
  exposure.
- **C6 (security prerequisite):** the compositor NotifyOnExit cleanup closes an LBI-boundary break —
  without it, a freed VMM Grant frame (reused under the SAS frame-identity invariant) is painted by
  the compositor. Must land in this phase, before the VMM becomes a compositor client (phase 03).
- Confirm the real gating of each grant syscall at review time (m5): the six ops all require bit 39
  in `required_cap_bit` (syscall.rs:528-533), regardless of the ABI-doc omission on GrantSlice/GrantFree.

## Next Steps
Phase 02 adds the full 2D command set and the host-side resource table (reusing this phase's
display-info encoder). Phase 03 adds the VMM-side NotifyOnExit registration matching this phase's
compositor cleanup.
