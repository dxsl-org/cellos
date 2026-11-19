---
phase: 3
title: "Launcher window and session protocol"
status: pending
priority: P1
effort: "4-6d"
dependencies: [2]
tier: thinking
checkpoint: true
---

# Phase 3: Launcher window and session protocol

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Replace the fake browser with a thin launcher that owns a real window and shows the guest's
pixels, and give `vmm_session` the operations that need: spawn per launch, scanout Grant
hand-off, damage, resize, input, terminate.

## Requirements

- Functional: opening Chrome spawns one hypervisor Cell bound to that launcher; LAUNCH
  carries the window content size; the guest scanout is created at that size.
- Functional: the hypervisor shares the scanout Grant read-only with the bound launcher
  (`SCANOUT_GRANT`: reg id, width, height, stride) and posts `DAMAGE` rects; it creates no
  compositor surface of its own when a launcher is bound.
- Functional: the launcher copies damaged rects into its `ViSurface` (`WindowLifecycle`,
  `ClosePolicy::Accept`), title "Chrome", `LineIcon::Globe`.
- Functional: input reaching the launcher's window is forwarded: pointer as **absolute**
  window-local coordinates (virtio-input tablet `EV_ABS`), buttons, wheel, keys translated
  from `KeySym` to Linux keycodes by one table. The hypervisor stops reading global input.
- Functional: window resize -> `RESIZE` -> virtio-gpu display-change event; the guest's new
  scanout replaces the Grant (`SCANOUT_GRANT` again).
- Functional: Close -> `TERMINATE` -> hypervisor stops the vCPU, releases Grants and exits;
  the launcher exits after the reply or a bounded timeout.
- Functional: while booting, the window shows the hypervisor's real stage text
  (`STATUS` events: "loading kernel", "guest booting", errors). No fabricated page content.
- Non-functional: only the bound launcher may drive a session (sender == client tid);
  unknown ops get an error reply, not silence.

## Architecture

New `vmm_session` ops (ABI record): `SCANOUT_GRANT`, `DAMAGE`, `STATUS` (hypervisor ->
launcher), `RESIZE` reply. Existing LAUNCH/FORWARD_INPUT/TERMINATE gain the size and
absolute-pointer fields. Launcher split into focused modules (<200 lines each):
`session.rs` (protocol), `frame.rs` (Grant -> surface copy), `input_bridge.rs` (keymap,
pointer), `main.rs` (window loop).

## Assumptions

- **Claim:** a Grant shared with `GrantShare` can be read by the launcher with `GrantSlice`
  while the hypervisor keeps writing it. **Confidence:** medium. **Verify:** host test + lane.
- **Claim:** Linux `virtio_gpu` honours `VIRTIO_GPU_EVENT_DISPLAY` + new `GET_DISPLAY_INFO`
  for resize. **Confidence:** medium. **Verify:** resize step in the lane.
- **Claim:** the launcher manifest can spawn `/bin/hypervisor` (needs `spawn=true`, already declared).
  **Confidence:** high.

## Related Files

- Modify: `libs/api/src/services/vmm_session.rs`, `docs/abi-boundary-records.md`
- Modify: `cells/services/hypervisor/src/run_loop_x86.rs`, `virtio_gpu/scanout.rs`,
  `virtio_gpu.rs`, `virtio_input.rs` (tablet mode), hypervisor `main.rs` (spawned mode)
- Rewrite: `cells/apps/chrome/src/main.rs` -> `main.rs`, `session.rs`, `frame.rs`, `input_bridge.rs`
- Delete: fake UI code (tabs, URL bar, `navigate`, `ChromeTab`); `cells/apps/vm-runner` if
  nothing else uses it (check `lsp` references and image profiles first)

## Implementation Steps

1. Host tests: ops encode/decode round trip and length checks; keysym -> Linux keycode
   table for letters, digits, modifiers, arrows, F-keys; damage-rect clipping to the surface;
   session refuses ops from a non-bound sender.
2. ABI ops + record. 3. Hypervisor: spawned mode, LAUNCH sizing, Grant hand-off, DAMAGE,
   STATUS, RESIZE, TERMINATE-exits, tablet input, drop global input polling.
4. Launcher rewrite. 5. Extend the phase-02 lane: window shows the guest pattern at the
   window's origin, a click lands at the right guest coordinate (guest logs it), resize
   changes the guest mode, Close removes the window and the hypervisor exits.
6. Changelog entry (new entry; older entries stay as history).

## Success Criteria

- [ ] Host tests above pass (api, hypervisor, chrome).
- [ ] Lane PASS: pattern in the Chrome window, click coordinate, resize, Close -> both Cells exit.
- [ ] `grep` finds no `navigate(`/`ChromeTab`/"Loaded:" in `cells/apps/chrome`.
- [ ] Stack & Tile lane and phase-04 GUI proof still PASS (Chrome is in the taskbar).

## Security Considerations

The launcher is the only input path into the guest; it must forward only events routed to
its own window. Grant is read-only to the launcher. The hypervisor validates every
launcher-supplied size (non-zero, <= screen, <= max scanout).

## Risk Notes

Copying full frames at 60 Hz in software may be costly; copy damage rects only.

## Deviation Log

- Decision: no STATUS event; the launcher shows "Starting Chromium in a Linux guest..." until the first
  frame and "The Linux guest has stopped." on EXITED. Why: both states are known to the launcher already.
- Decision: compositor (`hv`) presentation kept beside the client presenter (`Presenter` enum), so the
  console lane and `hv` still work; global host-input polling in the hypervisor removed (input only
  through the bound session).
- Deviation: resize is implemented (RESIZE -> display-change config event -> new Grant -> SCANOUT) but
  verified in phase 4: a fbdev framebuffer never changes size, only a DRM client (cage) does.
- Surprise: device reset from the guest driver must not end the session (first run showed "stopped").
  Impact: `GpuDev::reset` keeps the client Grant.
- Surprise (root cause, CONFIRMED): guest XMM registers were clobbered at every exit back to the Cell:
  the RunVcpu syscall entry saves the Cell's FXSAVE image, `fninit`s and restores the Cell's image on
  return. Seen as an intermittent guest `modprobe` segfault in musl malloc; `fpu-check.S` prints
  `X86_FPU_CORRUPT:01` on HEAD. Fix: per-vCPU `GuestFpu` image around both world switches.
- Decision: ostd gets `sys_grant_copy_rows_owned_by` so the launcher keeps `#![forbid(unsafe_code)]`.
- Decision: kernel launch edge `chrome -> /bin/hypervisor` (Path only, hypervisor's own ceiling).
- Deviation: `vm-runner` kept (wired into kernel catalog/cellbox); its duplicate api test removed.
- Surprise: removing host-input polling broke the console `hv` lane (pattern not on screen; log
  "scanout teardown deferred"): compositor frames were no longer drained, so the bridge read one as
  its DESTROY reply. Fix: console mode drains `ostd::input::poll_events` each idle again.
- Risk (open): session binds to the first valid LAUNCH sender; no parent/own-tid syscall exists to
  verify it is the spawning launcher.
