# Single-cell HID cutover

## Requirements

- Make `usb_hid::poll_interface` the sole normal HID report path.
- Encode every decoded event with its `HidDeviceId` before sending it to Input.
- Remove worker attach, report, event-relay, retirement, and diagnostic state.
- Keep descriptor-driven decode, boot fallback, split polling, and LED output behavior unchanged.

## Files

- `cells/drivers/dwc2-usb/src/main.rs`
- `cells/drivers/dwc2-usb/src/usb_hid.rs`
- `cells/drivers/dwc2-usb/src/hid_ipc.rs`
- `cells/drivers/dwc2-usb/src/hid_worker.rs`
- `cells/services/input/src/main.rs`
- `libs/api/src/services/ipc.rs`

## Implementation

1. Poll and decode each interface directly in the DWC2 host loop.
2. Forward events using the existing 14-byte device event format.
3. Delete worker lifecycle and retirement control protocol.
4. Delete the worker binary and worker-only IPC module when no caller remains.
5. Preserve per-device held-key tracking in Input.

## Success criteria

- Host tests cover device event framing and HID decoding.
- No source reference to `dwc2-hid`, worker lifecycle fields, or worker retirement opcode remains.
- AArch64 DWC2 host and Input cells build.

## Risk assessment

Rollback is the current worker implementation. No persistent data or wire compatibility is irreversible. The main risk is accidentally falling back to legacy 9-byte events and losing per-device cleanup; verification must assert the 14-byte device format.
