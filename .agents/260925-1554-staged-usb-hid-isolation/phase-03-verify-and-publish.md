# Verify and publish

## Requirements

- Run focused host tests for DWC2 HID and Input.
- Build the AArch64 DWC2 host, LAN front-end, and Input cell.
- Run the signed RPi3 cell packaging flow.
- Build the embedded RPi3 kernel and verify the generated uImage.
- Publish the image with exact SHA-256 and payload size.
- Treat physical keyboard input, key release, lock LEDs, mouse input, and concurrent Ethernet as the hardware gate.

## Result

- Focused host tests pass: DWC2 HID 12/12; AArch64 DWC2 and Input checks pass.
- Signed RPi3 package contains 26 cells and excludes `/bin/dwc2-hid`.
- Published uImage verifies at 10,129,408 payload bytes, SHA-256
  `dec6d94199401abef9d2c7106fa774ebbe3f026dec1601db10177c67b9783f45`.
- Physical RPi3 on the published rollback image: `help` and `ls` execute from
  USB keyboard input, USB mouse movement emits compositor cursor logs, and
  DWC2 Ethernet TX continues. Earlier hardware runs cover Caps/Num LEDs and
  left/right/middle mouse-button press/release.
- Runtime HID reconnect is intentionally unsupported and requires reboot; full
  plug-and-play remains separately designed and unqualified.

## Risk assessment

The published image can be rolled back by restoring the prior uImage. No
persistent data migration occurred.
