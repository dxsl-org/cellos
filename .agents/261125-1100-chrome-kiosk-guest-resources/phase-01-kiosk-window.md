---
phase: 1
title: "Only Chromium in the window"
status: completed
priority: P1
effort: "1d"
dependencies: []
tier: medium
checkpoint: true
---

# Phase 1: Only Chromium in the window

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

The taskbar's Chrome must start Chromium, and the window must show Chromium filling it and no
other part of the Linux guest. A probe of the shipped path (one screen per second, then the
guest's own controls) found six things that are not Chromium; this phase removes them.

## Requirements

- Functional: from the click to Chromium's first frame the window shows the launcher's status
  line, never the guest's blank screen.
- Functional: one pointer over the window.
- Functional: no window buttons and no warning bar drawn by Chromium; no key combination that
  leaves Chromium for a Linux console.
- Functional: Chromium ending (quit from inside, or a crash) ends the guest and closes the
  window; a guest that stops before Chromium showed says so in the window.
- Non-functional: no syscall, wire byte or catalog row changes.

## Architecture

No new component. The launcher decides when a guest frame is worth showing; the hypervisor
stops drawing the guest's cursor into a launcher's window and gains the ACPI fixed hardware
that makes `poweroff` real; the guest init runs Chromium unprivileged, sandboxed, without
window buttons, under a cage that takes no VT switch.

## Assumptions

- **Claim:** before Chromium's first frame the scanout is entirely black (the hypervisor's
  zeroed Grant, then cage's empty output), and Chromium's first frame is not.
  **Confidence:** high. **Verify:** lane check `status-until-chromium-no-blank-guest-screen`
  and the probe frames. — Verified.
- **Claim:** Chromium's sandbox runs in the guest as an unprivileged user (user namespaces,
  seccomp). **Confidence:** medium. **Verify:** `/proc/<pid>/status` of the renderers under
  plain QEMU/KVM. — Verified (`Seccomp: 2`, renderer user namespace differs from init's).
- **Claim:** Linux powers off through `\_S5` and the PM1a control block alone.
  **Confidence:** medium. **Verify:** serial log. — Verified (`reboot: Power down`).

## Related Files

- Modify: `cells/apps/chrome/src/{app.rs,frame.rs}`,
  `cells/services/hypervisor/src/{virtio_gpu.rs,acpi.rs,x86-port-dispatch.rs,run_loop_x86.rs,main.rs}`,
  `scripts/chrome-guest/init.sh`, `scripts/tier3_chrome_check.py`, `scripts/tier3_vm.py`,
  `scripts/qemu-tier3-chrome-x86.sh`
- Create: `cells/services/hypervisor/src/acpi_pm.rs`
- Docs: `docs/guides/tier3b-linux-vm.md`, ADR-0017 §5e, `docs/project-changelog.md`, `CHANGELOG.md`

## Implementation Steps

1. Probe: startup frames, pointer crops, Ctrl+Alt+F2, Chromium's own buttons, quit from inside.
2. Guest experiments under plain QEMU/KVM microvm (virtio-mmio, like the hypervisor presents).
3. Guest init, hypervisor, launcher; guest image through the pinned builder.
4. Lane checks; Chrome lane; regression lanes.

## Success Criteria

- [x] `scripts/qemu-tier3-chrome-x86.sh` 21/21 on the i7-14700 (nested KVM, QEMU 11.1.3):
      Maximize followed in 1 s; a second session ended from inside Chromium powers the guest
      off and closes the window. Chromium's UI came 31 s after the click, and 23 s in the
      stored run, which has phase 03's quiet-console step in the tree
      (`evidence/phase01-chrome-lane.log`, `phase01-chrome-serial.log`, `phase01-kiosk-sheet.png`).
- [x] Before: `evidence/phase01-before-startup-frames.png` (black for 26 s),
      `phase01-before-two-pointers.png`, `phase01-before-guest-controls.png` (VT switch, shrunk
      Chromium).
- [x] Regression: `scripts/qemu-tier3-display-x86.sh` 41/41; `scripts/ci-local.py run`
      `qemu-x86-hypervisor-boot`, `qemu-x86-tier3-virtio-e2e`, `qemu-x86-tier3-hostile`,
      `qemu-x86-tier3-python`, `lint`, `x86_64-lint`, `unit-tests`, `f1-admission`: all success.
- [x] Host tests: `app-chrome` 5 passed, `service-hypervisor` 24 passed (two new: ACPI table
      checksums and `\_S5`, PM1a control).

## Security Considerations

Chromium's own sandbox is now on inside the guest, in addition to the VM boundary. The PM1a
blocks are three 16-bit registers with no event source; the only effect a guest can cause
through them is its own power-off. The launcher reads guest pixels only through the
owner-checked Grant copy it already used.

## Risk Notes

The blank-screen rule is a pixel test: a guest whose first real frame is entirely black would
keep the status line. Chromium's frame never is (its tab strip and toolbar are not black in
either theme).

## Deviation Log

- Surprise (CONFIRMED, fixed): the launcher's status line was never visible. The hypervisor
  announces its zeroed scanout Grant when the session binds, and the launcher painted it over
  the status at once.
- Surprise (CONFIRMED, fixed): `poweroff -f` did not power the guest off under the x86
  hypervisor (`reboot: Power off not available: System halted instead`): the FADT had no PM
  blocks and the DSDT no `\_S5`. Chromium quit from inside left a halted guest, a running
  hypervisor and a frozen window. Every x86 guest was affected; the lanes never noticed because
  they stop QEMU on a marker.
- Surprise (CONFIRMED, fixed): on a port-write power-off and on MMIO dispatch failures the x86
  run loop returned without stopping the presenter, so a launcher was never sent
  `VMM_EVT_EXITED`. The loop now has one way out.
- Decision: Chromium runs sandboxed as an unprivileged user instead of adding `--test-type` to
  hide the `--no-sandbox` bar. The account is created at guest boot (`adduser`), so the image's
  package set is unchanged.
- Decision: one pointer by not drawing the guest's (cursor-queue commands consumed for a
  launcher's window). The pointer therefore keeps the arrow shape over links and text; the
  guest's cursor image reaches the host in phase 08. This also removes a full scanout copy and
  a full-frame damage per pointer move.
- Decision: Chromium's "system title bar" preference alone removes its window buttons;
  `cage -d` alone does not (tried both under microvm), so cage's flags stay minimal.
- Decision: the window closes when the guest stops after Chromium was on screen, whatever the
  exit status; a status channel from the guest was not added.
- Note (not verified by running): after a resize the launcher used to drop its frame and wait
  for a new scanout; a size the guest sees as unchanged (width within the same 8 pixels, or a
  restore from minimized) brings none, which by reading the code left "Resizing..." in the
  window. The launcher now keeps the old picture until the guest redraws. No lane drives a
  minimize and restore.
- Note: `cargo clippy -D warnings` for `service-hypervisor` on `aarch64-unknown-none-softfloat`
  stops at `pl011.rs:235` (needless range loop), a file this phase does not touch; no CI job
  runs that combination. `cargo check` for that target passes.
- Environment: Docker Desktop was stopped; it was started to run the pinned guest builder.
