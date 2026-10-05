---
phase: 3
sub: b
title: "USB HID producer role — key delivery to the input service"
status: completed
priority: P2
effort: M (kernel syscall + service role + two driver cells + policy)
dependencies: [3]
tier: thinking
ceiling: qemu
abi: append-only syscall 423 + service id 17 + shared allowlist bit 50 (owner-approved 2026-10-05)
---

# Phase 03b — USB HID producer role

## Evidence (2026-10-05, `qemu` ceiling)

- Syscall `RegisterUsbHidProducer = 423`, `service::USB_HID_PRODUCER = 17`,
  dispatch-gated on the `usb_driver` cap; it **shares allowlist bit 50** with the
  driver-registration family because the u64 bitmap is full (0–62 assigned, 63 is
  the VFS-mutate declaration) — the owner approved that, and the cap gate means
  bit 50 alone cannot publish the role.
- `cells/services/input` accepts `USB_HID_HOST` from the producer role instead of
  the singleton NIC role; `driver-xhci` never touches the NIC role;
  `driver-dwc2-usb` publishes both (on RPi3 the LAN9514 *is* the NIC), with
  nothing else changed.
- Gate: `xhci-x86` **4/4** — the QMP-injected keystroke is echoed by the shell
  (`shell: command not found: q`) — with zero `[kernel] syscall denied` lines in
  the xhci lane and in a production SATA boot. Regressions against a **rebuilt
  production ISO**: `x86_64-boot` 9/9, `ahci-x86` 5/5, `nvme-x86` 3/3,
  `pcie-multibus-x86` 2/2, `driver-registration-contract` 3/3, `cellos-kernel`
  187/187, `cellos-boards` 13/13, HAL boundaries, both ARM checks.
- Image composition: the x86 image now packages `/bin/input` and builds init with
  `--features input` (init source untouched), and the `qemu-x86_64-boot` CI job
  mirrors the build/sign/fat32 lists so CI can run the shell-receipt assertion.
- Evidence log: `evidence/phase-03b-usb-hid-producer.log`.

## Review (2026-10-05) — four findings, all closed

1. the CI job did not mirror the new composition (no `service-input`, no
   `/bin/input`, init without `input`) — fixed;
2. `/bin/xhci` declared no syscall allowlist, so the kernel left it at
   `u64::MAX` — an explicit `declare_syscalls!` now narrows it (section present,
   mask `0x000501e02200040f`);
3. the same defect existed in `cells/drivers/ahci` (our own shipped cell, which
   uses `run_app!` — that macro does not emit the section) — fixed for it too;
4. a dead input-service route was never re-resolved (the forward result was
   ignored) — now cleared so the next poll re-resolves and re-registers.
   Follow-up hygiene, not this phase: `nvme`/`e1000` still run undeclared.


## Target

Deliver decoded USB keys to the input service on x86 **without** touching the
singleton NIC role. Phase 03 brings the controller up, enumerates the QEMU
keyboard, decodes a keystroke in-cell and passes 4/4 tests — but it must not call
`sys_register_nic_driver()`, because that call is not a capability proof: it
overwrites the system's single NIC owner and publishes that TID as
`service::NIC_DRIVER` (`kernel/src/task/drivers/driver_cell.rs`). On a PC that
already has `e1000`/`virtio-net`, whichever cell registers last wins — if xHCI
wins, network IPC is delivered to the xHCI handler and dropped; if the NIC wins,
the input service's producer gate refuses xHCI. The `dwc2-usb` precedent is safe
only because that cell is *also* the LAN9514 NIC host.

## Change

1. **New append-only syscall** `RegisterUsbHidProducer` (next free number after
   422), cap-gated on `usb_driver`, publishing a new service id
   `service::USB_HID_PRODUCER` (next free id; ids are sequential `u16` — VFS 1 …
   NIC_DRIVER 10). Add the `ViSyscall` variant, its `from_u64` mapping, and a
   free allowlist bit; document the ABI next to `RegisterNicDriver`.
2. **Input service gate** (`cells/services/input/src/main.rs`, the
   `EventSources` USB_HID_HOST arm): accept a sender whose
   `sys_lookup_service(service::USB_HID_PRODUCER) == tid` instead of
   `NIC_DRIVER`. Do **not** widen what kinds are accepted, and do not touch
   `dispatcher.rs` (carries unrelated WIP).
3. **`cells/drivers/xhci`**: call the new registration; never the NIC one.
4. **`cells/drivers/dwc2-usb`**: call the new registration **in addition to** its
   existing NIC registration (on RPi3 the LAN9514 *is* the NIC). Behaviour
   otherwise byte-identical — that cell has exact-device evidence that cannot be
   re-run cheaply.
5. **Policy**: the new allowlist bit for `/bin/xhci` and `/bin/dwc2-usb` in
   `scripts/sign-policy.py`.
6. **Docs**: record the new syscall + service id where the repo documents them
   (`docs/specs/03-runtime.md` or the ABI comment), and note the Law-1
   confirmation in the plan.

## QEMU-first gate

- `xhci-x86` grows the end-to-end assertion: the QMP-injected keystroke is
  decoded in the cell **and echoed by the shell** (`Cellos >` receives the
  character) — phase 03 stops at "decoded in cell".
- Unchanged-behaviour checks that must stay green: `x86_64-boot` 9/9 (COM1 input
  still works), `ahci-x86` 5/5, `nvme-x86` 3/3, `pcie-multibus-x86` 2/2,
  `driver-registration-contract` 3/3, `cellos-boards` 13/13, `cellos-kernel`
  187/187, HAL boundaries.
- **Networking must not be affected**: the e1000/virtio-net registration path is
  untouched, and `nic-x86` + the q35 DHCP lane stay green (this is the whole
  point of the sub-phase).
- **ARM**: `cargo check -p driver-dwc2-usb --target aarch64-unknown-none-softfloat`
  and `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat
  --features board-rpi3` still pass.

## Acceptance

- The injected key reaches the shell through the input path on QEMU, and the
  xHCI cell never registers as the NIC owner (assert no `service::NIC_DRIVER`
  publication from `/bin/xhci`).
- Every lane listed above stays green, and the ARM checks pass.
- The ABI addition is append-only, documented, and its allowlist bit is the next
  free one.

## Out of scope

- Hubs beyond enumeration, USB3 tuning, isochronous, USB storage, usb-serial,
  power management; any change to how the input service dispatches events.

## Risk assessment

- **ABI/Law-1**: a new syscall number + allowlist bit is an ABI change. The plan
  records that the repository owner confirms it before merge; the implementation
  is append-only and reversible by deleting the variant and its mapping.
- **Undo**: revert the four call sites; xHCI then decodes keys in-cell but does
  not deliver them, and networking is unaffected either way.
- **Not undoable**: none — no persistent state is written.
- **dwc2**: if the added registration cannot be proven behaviour-neutral on the
  ARM build, keep dwc2 as it is and have only xHCI use the new role (record it),
  rather than risking the board lane.
