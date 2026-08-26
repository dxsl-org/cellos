# Phase 00 — hv-arm-gui Image & Display Sink (environment / integration)

## Context Links
- Plan: [plan.md](plan.md)
- HV-ARM disk builder (no GPU driver cell today): `scripts/format-disk-hv-arm.sh:18-29` (CELLS map — has `compositor`, NO gpu/driver entry).
- HV-ARM QEMU launcher (headless today): `run-hypervisor-arm.ps1:72-82` (`-nographic`, only `virtio-blk-device` + `virtio-net-device`; no GPU device, no SDL/VNC).
- Working GUI reference path (contrast): `gen_disk.ps1:94` (builds `driver-virtio-gpu`), `gen_disk.ps1:449` (`/bin/virtio-gpu=$virtio_gpu_bin` into the cell-store), `run-gui.ps1` (QEMU with a display + GPU device).
- Host virtio-gpu Driver Cell (the display sink for the compositor's own output): `cells/drivers/virtio-gpu/`.
- Compositor output side: `cells/services/compositor/src/render.rs:11` (`sys_gpu_flush`), `main.rs:122-129` (`sys_get_resolution` display hotplug poll).

## Overview
- **Priority:** P1 — **prerequisite for the phase-03 "pixels appear" milestone.** Without it there is
  no on-screen sink for the compositor, so nothing the guest draws can ever be seen.
- **Status:** complete (implementation; visual evidence remains in phase 05)
- **Description:** Build an `hv-arm-gui` image variant that bundles the host **virtio-gpu Driver
  Cell** (`cells/drivers/virtio-gpu`) alongside the existing hypervisor + compositor cells, and a
  QEMU launcher that attaches a GPU display device with SDL/VNC (or an FB-capture harness) instead of
  `-nographic`. This is the environment the guest's rendered pixels are ultimately displayed on: the
  Cellos compositor composites surfaces and flushes them through its own virtio-gpu Driver Cell to a
  real QEMU display.

## Key Insights
- The current HV-ARM image is **headless by construction**: `scripts/format-disk-hv-arm.sh` bundles
  `compositor` but NO virtio-gpu Driver Cell, and `run-hypervisor-arm.ps1` runs `-nographic` with
  only blk + net devices. The compositor has no output device to flush to, so even a correct
  guest→compositor pixel path would render to nothing.
- The **working GUI path already exists** for the normal (non-hypervisor) image: `gen_disk.ps1`
  builds `driver-virtio-gpu` and places `/bin/virtio-gpu` in the cell-store; `run-gui.ps1` launches
  QEMU with a display. Phase 00 mirrors that wiring into the HV-ARM image — it is assembly of an
  existing, proven configuration, not new device code.
- The compositor discovers its screen size dynamically via `sys_get_resolution()`
  (compositor/src/main.rs:127) and rebuilds its framebuffer on change — so the guest surface
  resolution must be reconciled against the ACTUAL host resolution, not a hard-coded constant (see
  phase 03, finding on screen dimensions).
- This phase adds NO device-model code and NO new syscall — it is a build-script + launcher +
  cell-manifest packaging change.

## Requirements
**Functional**
1. A disk-image builder variant (`format-disk-hv-arm-gui.sh` OR a `--gui` flag on
   `format-disk-hv-arm.sh`) whose CELLS map includes `driver-virtio-gpu` → `/bin/virtio-gpu`.
2. A QEMU launcher variant (`run-hypervisor-arm-gui.ps1` OR a `-Gui` switch on
   `run-hypervisor-arm.ps1`) that attaches a virtio-gpu display device and an SDL or VNC front-end
   (or a headless FB-capture front-end for the CI/automated lane).
3. The host virtio-gpu Driver Cell registers `service::BLOCK`-style — verify it registers as the
   display driver the compositor flushes to (mirror the gen_disk GUI image's cell set).
4. Boot the hv-arm-gui image; confirm the compositor comes up on a visible QEMU display (the Cellos
   desktop / cursor renders), independent of any guest.

**Non-functional**
- Do not regress the headless `run-hypervisor-arm.ps1` path (keep it for serial-only CI lanes).
- Keep the GUI and headless launchers as sibling scripts or one script + switch — no duplicated
  copy that drifts (DRY).

## Architecture
Data flow (host display path, guest-independent):
```
compositor render_frame → sys_gpu_flush → host virtio-gpu Driver Cell (/bin/virtio-gpu)
  → QEMU virtio-gpu device → SDL/VNC window (or FB capture)
```
The guest pixel path (phases 02-04) feeds INTO this: guest → VMM copy → Grant → compositor surface →
the flush above. Phase 00 stands up the right-hand half so phase 03's left-hand half has a sink.

## Related Code Files
- **Create:** a GUI disk-image builder (`scripts/format-disk-hv-arm-gui.sh` or a `--gui` branch in
  `scripts/format-disk-hv-arm.sh`), a GUI QEMU launcher (`run-hypervisor-arm-gui.ps1` or a `-Gui`
  switch in `run-hypervisor-arm.ps1`).
- **Reference (do not modify):** `gen_disk.ps1:94,449`, `run-gui.ps1` (the proven GUI wiring to
  mirror), `cells/drivers/virtio-gpu/`.

## Implementation Steps
1. Add `driver-virtio-gpu` → `/bin/virtio-gpu` to the HV-ARM CELLS map (new GUI builder variant);
   confirm the aarch64 `driver-virtio-gpu` artifact builds (it already builds for gen_disk).
2. Add a GUI QEMU launcher: replace `-nographic` with a virtio-gpu display device + SDL/VNC (mirror
   `run-gui.ps1`'s device flags), keep the blk + net devices.
3. For the automated lane, wire an FB-capture front-end (VNC-to-PNG or QEMU framebuffer dump) so
   phase-05 T5 has a candidate oracle.
4. Boot hv-arm-gui with NO guest graphics yet; confirm the Cellos compositor renders on the display
   (host cursor / desktop visible).
5. Document the GUI image + launcher in the phase-05 test harness section.

## Todo List
- [x] GUI disk-image builder variant bundling `/bin/virtio-gpu`
- [x] GUI QEMU launcher (GPU display device + SDL/VNC, drop `-nographic`)
- [ ] FB-capture front-end for the automated lane (optional; enables phase-05 T5)
- [x] Boot hv-arm-gui, confirm physical GPU Driver Cell and compositor initialize
- [x] Document GUI image + launcher for phase 05

## Success Criteria
- The hv-arm-gui image boots and the Cellos compositor is visible on a QEMU SDL/VNC display (or
  captured FB), with the host virtio-gpu Driver Cell bound — all BEFORE any guest graphics exist.
- The headless `run-hypervisor-arm.ps1` path still works for serial-only CI.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| aarch64 `driver-virtio-gpu` not built into the HV-ARM image → compositor has no sink | M×H | Explicit build + presence check (mirror `gen_disk.ps1`'s `Test-Path` gate at :449); fail the build loudly if the artifact is missing (same pattern as the hypervisor presence check at format-disk-hv-arm.sh:45). |
| SDL/VNC unavailable in the CI QEMU build → no automated pixel oracle | M×M | CI stays serial-only (phase-05 T1-T3/T10-T11); the GUI display is the interactive/real-HW lane. FB-capture front-end is the optional automated bridge. |
| Host resolution differs from the guest's advertised 1024×768 → guest surface clipped | M×M | Phase 03 reconciles guest surface size against `sys_get_resolution()`; do not hard-code. |

## Security Considerations
- No new capability or syscall — packaging only. The host virtio-gpu Driver Cell already ships in the
  gen_disk image with its established manifest; reuse it unchanged.

## Next Steps
Phase 03's "pixels appear" milestone runs ON this image. Until Phase 00 lands, **phase 02 (host-side
resource correctness, serial-observable) is the only demonstrable Track A milestone** — there is no
display for phase 03 to target.
