---
title: "Tier 3b VirtIO-GPU backend — Linux guest graphics on the Cellos compositor"
description: "2D virtio-gpu device model in the ARM64 hypervisor Cell; guest framebuffer copied into a Grant and driven through the existing compositor path."
status: partial
priority: P2
effort: ~27-43 eng-days (Track A phases 00-05, re-baselined post-red-team); Track B/x86 deferred
branch: fix/ci-followups-srv-lua-qemu
tags: [hypervisor, tier3b, virtio-gpu, compositor, arm64, graphics]
created: 2026-07-24
---

# Tier 3b VirtIO-GPU Backend

Add a 2D virtio-gpu device model to the ARM64 hypervisor Cell so a Linux guest gets
`/dev/dri/card0` (DRM dumb buffers), and its rendered pixels appear in the Cellos host
compositor. Guest-side 3D via Mesa llvmpipe (software) is in scope; host-accelerated 3D
(virgl) is a deferred gap analysis.

## Locked architecture (do not re-litigate)

- **Integration = "hypervisor as compositor client via copy" (Path A).** On `RESOURCE_FLUSH`
  the VMM copies the guest backing rect (via `read_guest_memory`) into a Grant buffer it OWNS,
  then drives the existing compositor path (`CREATE_SURFACE` → `ATTACH_GRANT` → `DAMAGE_NOTIFY`,
  `COMPOSITOR_ENDPOINT=5`). The guest→host trust boundary is crossed exactly once, at the copy,
  which is the natural clamp/validate point. Compositor stays pure LBI (`forbid(unsafe)`) and
  sees a normal Cell Grant. No new "external framebuffer / page-pinning" compositor opcode.
- **The VMM must NOT reuse `ostd::display::ViSurface` verbatim** (red-team C3/C4). ViSurface does
  BLOCKING `sys_send`+`sys_recv(0)` (wildcard) for CREATE_SURFACE/ATTACH_GRANT
  (display.rs:206-252) — inside the single-fiber MMIO-exit run loop that also injects timers and
  polls net-RX (run_loop.rs:129-141), a blocking rendezvous parks the whole VM, and a second
  wildcard `recv(0)` collides with the net req/reply consumer (net_backend.rs:30,50) →
  cap-poisoning. The VMM does the surface handshake ONCE at bring-up (before the run loop) and
  uses a **masked** recv (sender == comp_tid) with a timeout; per-flush notify is fire-and-forget.
  There is exactly ONE host copy of the scanout pixels — in the owned Grant (no separate
  `host_pixels` buffer; see C5).
- **ARM64 first.** ARM64 EL2 has working virtio-mmio (run_loop.rs). x86 MMIO dispatch is
  unwired (run_loop_x86.rs:161-164, awaiting the external effort `HV-X86-MMIO`) → x86 is a gated
  follow-up (phase 06).
- **New device = MMIO slot 3** → `virtio_mmio@a000600`, SPI 19 (pattern SPI = 16 + slot,
  dtb.rs:164-189; slot dispatch run_loop.rs:83-88).

## Phases

| # | Phase | Status | Effort | Depends |
|---|-------|--------|--------|---------|
| 00 | [hv-arm-gui image & display sink](phase-00-hv-arm-gui-image-and-display-sink.md) | complete | 2-4 d | — |
| 01 | [Wiring & capabilities](phase-01-wiring-and-capabilities.md) | complete | 3-5 d | — |
| 02 | [2D resource + transfer model](phase-02-resource-transfer-model.md) | complete | 6-9 d | 01 |
| 03 | [Compositor scanout bridge (copy path)](phase-03-compositor-scanout-bridge.md) | complete | 8-12 d | 00, 02 |
| 04 | [Cursor plane](phase-04-cursor-and-display-info.md) | complete | 3-5 d | 03 |
| 05 | [Test & verification matrix](phase-05-test-verification-matrix.md) | partial | 5-8 d | 00, 03, 04 |
| 06 | [x86 follow-up (gated on x86-MMIO-exit work)](phase-06-x86-followup.md) | deferred | 4-6 d | 02, [x86-MMIO-exit] |
| 07 | [Track B: 3D/virgl gap analysis (decision-only)](phase-07-track-b-3d-gap-analysis.md) | pending | 1-2 d | — |

MVP deliverable = phases 00-05 (Track A). Sequencing: **P00 (display sink, prerequisite) → P01 →
P02 (serial-demonstrable in isolation: host-side resource correctness, no display) → P03 (the
"pixels appear" milestone, DEPENDS ON P00 for a display) → P04 → P05**. Until P00 lands, P02 is the
only demonstrable Track A milestone (there is no on-screen sink for P03 to target). P03's effort is
re-baselined up (was 6-9 d) to cover the non-blocking handshake state machine (C3/C4), the
single-copy-into-Grant redesign (C5), and the VMM-side `NotifyOnExit` registration (C6). P05 depends
on P03 **and** P04 because its completion gate includes the cursor test (T9), a phase-04 deliverable.

Implementation closed on 2026-07-25: device wiring, bounded wire decoding, resource/cursor model,
Grant-backed compositor bridge, teardown/reconnect handling, pinned Alpine test image, and the
dedicated serial-token lane are present and compile/test cleanly. Track A remains `partial` only
because QEMU-TCG hits the documented ARM nested-translation fault before Linux emits the strict
T1/T2/T12 tokens; phase 05 records the exact evidence and the ARM64 KVM/real-hardware gate.

## Cross-cutting concerns

- **Capability change (interface/security review):** the hypervisor Cell currently has NO grant
  rights. It must add `GrantRegister`/`GrantShare`/`GrantSlice`/`GrantUnregister` to
  `declare_syscalls!` (main.rs:22-46), which turns on GrantCap allowlist bit 39. **Note (red-team
  m5): enabling GrantCap exposes ALL SIX grant ops, not four** — `GrantAlloc`, `GrantShare`,
  `GrantSlice`, `GrantFree`, `GrantRegister`, `GrantUnregister` all map to bit 39
  (syscall.rs:528-533), even though the ABI docs for `GrantSlice=210` / `GrantFree=211`
  (syscall.rs:206-212) omit the bit-39 note the others carry. The review must account for the Cell
  gaining `GrantAlloc`/`GrantFree` too. Grant caps are userspace capabilities, not kernel
  mechanism → stays within the Kernel Boundary Law. No new kernel syscall or SPI number is required
  (SPI 19 is a guest-visible GIC line the VMM already owns via `inject_irq`).
- **Compositor owner-death cleanup is a security PREREQUISITE (red-team C6), not future work.**
  SAS/LBI has no Drop-unwind on abnormal cell death (OOM-kill / panic-abort skip any
  Shutdown-time teardown), and the SAS frame-identity invariant means a freed Grant frame stays
  identity-mapped and is reused by another cell — so if the compositor keeps a cached Grant pointer
  after the VMM cell dies, it paints another cell's memory (an LBI-boundary break). The compositor
  already has the cleanup primitive as dead code: `SurfaceTable::caps_owned_by(tid)`
  (surface_table.rs:240-247, `#[allow(dead_code)] // used by future NotifyOnExit cleanup path`) and
  the cached grant pointer stored via `attach_grant` (main.rs:290-298; `PixelSource::Grant { ptr,
  reg_id }`, reg_id flagged `#[allow(dead_code)]` at surface_table.rs:22). The fix: wire a
  `NotifyOnExit` handler in the compositor that calls `caps_owned_by(dead_tid)` → `remove(cap)` per
  cap (dropping the cached pointer); the VMM registers kernel `NotifyOnExit` for itself so the
  compositor is told when the VMM dies. Compositor-side cleanup lands as a prerequisite in phase 01;
  VMM-side `NotifyOnExit` registration lands in phase 03. [Finding referenced `remove_by_owner`; the
  actual dead-code symbol is `caps_owned_by` + `remove`.]
- **Hostile-guest hardening (phases 02 + 03):** every field a hostile guest controls is validated
  at the copy boundary — resource id table lookup, ATTACH_BACKING scatter-gather entry count and
  per-entry length, TRANSFER_TO_HOST_2D rect + offset within resource bounds, RESOURCE_FLUSH rect
  within scanout, and destination bytes within the owned Grant size. See each phase's Security
  Considerations.
- **Coding Laws:** no `mod.rs` (new files `virtio_gpu.rs` + `virtio_gpu/` submodules); owned
  buffers; `VAddr`/`PAddr`; document all `// SAFETY:`; hypervisor Cell is NOT `forbid(unsafe)`
  (it uses inline-asm syscalls) but new device code should avoid raw guest-pointer deref and go
  through `vmm::read/write_guest_memory` (Law 4). Keep each file <200 LOC.

## Open questions

**Research-status caveat (red-team M7): the `research/` directory is EMPTY** — every
"research-confirmed" marker in the phase files is currently **unbacked**, and one such claim (the
pixel-format claim) was demonstrably wrong (see C1). Before or at impl, EITHER land actual research
notes under `research/` (driver refs: `virtgpu_vq.c`, `virtgpu_display.c`,
`virtio_gpu_translate_format`, the virtio-gpu format enum, `virtio_gpu_ctrl_hdr` struct, cursorq
semantics, descriptor sizing) OR treat every "research-confirmed" marker as **"to-verify-at-impl."**
Each phase's "Key Insights (research)" block is downgraded accordingly. The command set, struct
layouts, and minimal feature set (0 bits) are load-bearing assumptions to verify, not settled facts.

Remaining unresolved / to-confirm-at-impl:

1. **Config-change path** (phase 01/04): `events_read=0` permanently is *assumed* fine for a
   fixed-resolution probe — verify `drivers/gpu/drm/virtio/virtgpu_drv.c` `config_changed` vector
   before hard-coding. Not a probe-time blocker.
2. **Cursor rendering** (phase 04): whether the compositor exposes a client-facing cursor-overlay op,
   or the cursor must be CPU-composited into the scanout Grant. Fallback B (composite) always works;
   resolve by inspecting compositor `main.rs`/`render.rs` at impl time.
3. **Per-flush copy cost under TCG** (phase 03): damaged-rect copy + DamageNotify per FLUSH — measure
   whether damaged-rect copying + the 10 ms preempt budget keeps other cells live; throttle if not.
   The controlq used-ring entry + `inject_irq(19)` must complete INDEPENDENT of the compositor send
   so the guest ring frees even when the compositor is throttled (M5).
4. **Alpine guest carries the virtio-gpu driver** (phase 05): the real gate is confirming
   `CONFIG_DRM_VIRTIO_GPU` is present in the Alpine initramfs (built-in or module). The wire
   protocol is the **virtio-gpu WIRE protocol** — stable, spec-defined little-endian bytes off the
   virtqueue — NOT a shared C-struct ABI, so there is no "struct ABI match" concern across kernel
   versions; drop that framing.
5. **x86 transport** (phase 06): virtio-mmio hole vs virtio-pci BAR — decided by the external
   x86-MMIO-exit effort (tracking ref `HV-X86-MMIO`, see phase 06), not now.

## Red Team Review

Review applied 2026-07-24; all findings below are folded into the phase files. Codes are internal to
this plan (never used in code comments).

### Critical (7) — all mitigated-in-plan or made prerequisite
| # | Finding | Resolution | Where |
|---|---------|-----------|-------|
| C1 | Format-1-only rejects Linux DRM default XRGB8888 (maps to format **2**) → first CREATE_2D fails, zero pixels | **Mitigated:** accept formats 1 AND 2, both → `Bgra8888`, X = opaque alpha, no swap; 3/4 rejected-by-design | phase-02 (Key Insights, req, Security), phase-05 |
| C2 | No display sink: HV-ARM image is headless (`-nographic`, no gpu driver cell) | **New prerequisite phase 00** (hv-arm-gui image + display); P03 depends on it | phase-00 (new), plan table |
| C3 | Blocking `sys_send`+`sys_recv` handshake inside the FLUSH MMIO-exit parks the single run-loop fiber | **Mitigated:** handshake ONCE at bring-up (or `try_send` state machine); never `sys_recv(0)`; use `sys_recv_timeout`; do NOT reuse ViSurface verbatim | phase-03 (Key Insights, Steps, Risk) |
| C4 | Second wildcard `recv(0)` collides with net req/reply → cap poisoning | **Mitigated:** masked recv (sender == comp_tid), validate reply opcode/shape; no unsolicited inbound while a handshake reply is awaited | phase-03 (Security, Steps) |
| C5 | 4 MiB blk + 3 MiB host_pixels + 3 MiB Grant = 10 MiB > ~8 MiB heap → infallible `vec!` aborts | **Mitigated:** eliminate `host_pixels`; TRANSFER copies guest backing → Grant directly (ONE host copy); total-byte budget (≤~5 MiB) enforced; `try_reserve` → `ERR_OUT_OF_MEMORY` | phase-02 (req, Risk, Non-func), phase-03 (copy path) |
| C6 | Stale-grant cross-cell leak: compositor keeps cached Grant ptr after VMM death → paints reused frames | **Prerequisite:** compositor `NotifyOnExit` → `caps_owned_by`+`remove` (P01); VMM registers `NotifyOnExit` (P03) | plan cross-cutting, phase-01, phase-03 |
| C7 | Geometry OOB writes into the Grant with guest-controlled dims (SET_SCANOUT larger-than-Grant; cursor `pos - hotspot` underflow) | **Mitigated:** independent src/dst strides, per-row bound by grant_len+width; require res dims == Grant dims or realloc; signed origin, clip negatives | phase-03 (Security), phase-04 (Security) |

### Major (8) — folded into phases
| # | Finding | Resolution |
|---|---------|-----------|
| M1 | Scanout binding not invalidated on UNREF/reset/rebind; per-reboot 3 MiB leak; SET_SCANOUT id=0 = disable | Invalidate on UNREF-of-bound / device-reset / rebind; refuse UNREF of live scanout; re-derive dims each flush; idempotent teardown+release. phase-02, phase-03 |
| M2 | ATTACH_BACKING must REPLACE prior backing, not append | Replace + cap cumulative entries. phase-02 |
| M3 | Vec-indexed resource table → CREATE_2D(id=0xFFFFFFFF) forces ~4 B-entry alloc | Mandate `BTreeMap<u32,Resource>`; cap distinct live resources. phase-02 |
| M4 | Transfer arithmetic can overflow / miss `usize::MAX` sentinel | `checked_add`/`checked_mul`, validate offset+bytes ≤ backing sum and dst ≤ resource bytes, check `read_guest_memory == usize::MAX`. phase-02 |
| M5 | Full-frame copy default; ring free coupled to compositor send | Damaged-rect copy default; cap bytes/flush; used-ring entry + `inject_irq(19)` INDEPENDENT of the send. phase-03 |
| M6 | Real `OK_DISPLAY_INFO` encoder placed in phase 04, but driver's init-time GET_DISPLAY_INFO needs it | Move real 280 B display-info response to phase 01/02; phase-01 success = "driver binds; card0 registers"; usable modeset belongs to the phase that returns a real payload. phase-01, phase-02, phase-04 |
| M7 | `research/` empty → every "research-confirmed" marker unbacked; format claim was wrong | Land research notes OR downgrade all markers to "to-verify-at-impl"; format claim corrected. plan open-questions, all phases |
| M8 | VMM never MOVE/RAISE_SURFACE → guest surface at (0,0)/default z, focus routing undefined; 1024×768 hard-coded | Define explicit position/z-order + focus-routing behavior for the guest region; verify screen dims via `sys_get_resolution()`. phase-03 |

### Minor (8) — folded in
m1 config offsets pinned (num_scanouts@8) · m2 assert `device_features_lo()==0` + comment why ·
m3 SET_SCANOUT id=0 = disable (folded into M1) · m4 clamp outgoing DamageNotify.rect to
surface/Grant dims · m5 single grant helper (perm=RO, target=comp_tid asserted !=0, checked_mul) +
correct phase-01 "4 syscalls / bit 39" claim to "GrantCap exposes all six grant ops" · m6 fix
open-question #4 (wire protocol, not struct ABI) · m7 P05 depends on 03+04 (T9 cursor in its gate) ·
m8 x86 effort renamed to tracking ref `HV-X86-MMIO` (removes "task P06" vs "phase 06" collision).

### Refuted (accepted as sound — no change)
- Integration architecture **Path A** (copy into owned Grant, compositor stays `forbid(unsafe)`) is sound.
- `process_notify` DOES support control-queue response write-back (the phase-02 model fits).
- MMIO slot 3 / SPI 19 / region `0x0a000600` — no collision with console/blk/net slots.
- Compositor defensively re-clamps damage in `render.rs` (memory-safe even if VMM over-reports).
- `read_guest_memory` is all-or-nothing (returns `usize::MAX` on bad GPA, never a partial raw deref).
- Track B (host-accelerated 3D) verdict — G3-class, deferred — is sound.

### Accepted risk (not fully closed in plan)
- Per-flush copy cost under TCG (open question #3) — measured at impl, throttle if needed.
- No automated FB-capture oracle in CI historically — pixel-appearance tests stay interactive/real-HW.

## Assumptions to verify before/at impl
- Pixel formats accepted: **1 (B8G8R8A8) AND 2 (B8G8R8X8)** — the Linux DRM default XRGB8888 maps to
  format 2 (C1). Both map to compositor `PixelFormat::Bgra8888`, X treated as opaque alpha, no swap.
- `virtio_gpu_config` field offsets (m1): `events_read@0, events_clear@4, num_scanouts@8,
  num_capsets@12` — `num_scanouts=1` at **offset 8**.
- Device offers **zero** low feature bits (m2) — `process_notify` handles only NEXT/WRITE, not
  INDIRECT/EVENT_IDX; offering a feature bit lets the guest negotiate indirect descriptors and
  corrupt the chain walk.
- Compositor screen dimensions come from `sys_get_resolution()` (compositor/src/main.rs:127), NOT a
  fixed constant — verify actual host resolution before hard-coding 1024×768 (M8; no
  `FALLBACK_WIDTH/HEIGHT` constant exists).
