# Staged USB HID isolation

## Decision

Restore synchronous HID parsing and decoding inside `/bin/dwc2-usb` for the current RPi3 embedded profile. Preserve device-identified events and per-device key cleanup. Keep `/bin/lan9514` isolated. Remove the incomplete `/bin/dwc2-hid` lifecycle and packaging.

Full USB class-driver isolation and full HID plug-and-play remain the
production-security target. Plug-and-play starts only after CellOS has reliable
bounded transport and dedicated input-producer authority; its lifecycle design
and physical qualification matrix are in `docs/roadmap/usb-isolation.md`.

## Acceptance criteria

- DWC2 polls and decodes every HID interface in one ordered host loop.
- HID reports no longer cross a lossy host-to-worker IPC boundary.
- Input receives device-identified events from the kernel-authenticated DWC2 host.
- Lock LEDs and per-device pressed-key cleanup remain functional.
- `/bin/dwc2-hid` is absent from build, signing, packaging, boot ceilings, and launch profiles.
- `/bin/lan9514` remains capability-free and supervised by `/bin/dwc2-usb`.
- The long-term isolation roadmap explicitly handles reliable transport, host-controller failure, hostile USB protocol traffic, and DMA confinement.

## Phases

| Phase | File | Status | Depends on |
|---|---|---|---|
| 1 | `phase-01-single-cell-cutover.md` | complete | — |
| 2 | `phase-02-policy-packaging-cleanup.md` | complete | Phase 1 |
| 3 | `phase-03-verify-and-publish.md` | complete | Phases 1–2 |

## Permanent architecture record

- `docs/decisions/0020-staged-usb-hid-isolation.md`
- `docs/roadmap/usb-isolation.md`

## Evidence

- Host tests: `driver-dwc2-usb` 12/12; AArch64 DWC2 and Input checks pass.
- RPi3 package: 26 signed cells; no `/bin/dwc2-hid`.
- Published uImage: 10,129,408 payload bytes; SHA-256
  `dec6d94199401abef9d2c7106fa774ebbe3f026dec1601db10177c67b9783f45`.
- Physical RPi3 on the published rollback image: USB keyboard executes `help`
  and `ls`; USB mouse emits compositor cursor-motion logs; DWC2 Ethernet TX
  remains active. Prior physical evidence covers Caps/Num LEDs and left/right/
  middle mouse button press/release.
- Runtime reconnect is intentionally unsupported in the embedded profile; it
  requires reboot. Full plug-and-play is a separately specified future design.

## Assumptions

- Current RPi3 deployments use controlled embedded peripherals rather than hostile hot-plug USB devices.
- A DWC2 reset necessarily interrupts every function on the shared LAN9514 controller; software isolation cannot remove this physical availability coupling.
