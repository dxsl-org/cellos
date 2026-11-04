# Shared contract for NIC phases 13–19

Every physical-NIC phase follows this contract. The phase file adds only family-specific facts.

## Driver cell shape

- One cell per controller family under `cells/drivers/<family>/`, files under 200 lines: `main.rs`, `identity.rs`, `regs.rs`, `controller.rs`, `phy.rs` (if any), `rings.rs`, `irq.rs`.
- `identity.rs`: exact `vendor:device` table. Each entry is `Driveable` (an implemented, tested path) or `Target` (known SKU, not yet admitted). Only `Driveable` IDs are queried. Unknown IDs are refused by name. This is the same pattern as `cells/drivers/igb/src/identity.rs`.
- Discovery uses `FindPcieDeviceByVendor` with the cell's own IDs (no class-triple sharing).
- `pcie_ecam.rs` `ethernet_id_has_driver` and its tests list every `Driveable` ID with its owning cell.
- DMA only through `GrantDma`; interrupts through `AllocPcieIrq` (MSI-X preferred).
- Net-bridge registration and the DHCP data path are identical to igb.
- `declare_syscalls!` lists only the syscalls the cell uses.

## Evidence ladder per phase

1. Host unit tests: register encoding, descriptor layout, identity classification, PHY/state machines against a fake register file.
2. Compile for `x86_64-unknown-cellos`; image composition + CI build list updated (the cell ships, even if no QEMU model exists).
3. **Physical capture** on a machine carrying the NIC: bind log, link up, DHCP lease, 10-minute ping without loss, VT-d isolation active before first DMA. Without that capture the phase is `blocked: hardware`, never `done`.
4. An ID is promoted from `Target` to `Driveable` only after step 3 on a device with that ID.

## Licensing

Linux drivers are GPL-2.0 and may be read as references for hardware behaviour, but no code or comment is copied. FreeBSD/Intel BSD-licensed drivers may be reused only with attribution recorded in `NOTICE` and a check against `LICENSING.md`. Firmware blobs are not loaded unless the phase records their license and redistribution terms.

## Out of scope for every NIC phase

Offloads beyond checksum (TSO, RSS, VLAN filtering), SR-IOV, wake-on-LAN, EEE tuning, multiple queues beyond one Rx + one Tx. Each is a later, separately justified change.
