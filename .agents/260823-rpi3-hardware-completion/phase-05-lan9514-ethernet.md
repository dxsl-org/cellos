---
phase: 5
title: "LAN9514 Ethernet via DWC2 USB Host"
status: blocked
priority: P3
effort: "15d"
dependencies: []
tier: thinking
---

# Phase 05: LAN9514 Ethernet via DWC2 USB Host

## Overview

Implement the entire USB host stack required to drive RPi3's LAN9514
USB-Ethernet controller: Synopsys DWC2 host controller, USB root hub + hub
class, SMSC LAN95xx USB-Ethernet protocol, and integration with the existing
net service's raw NIC IPC contract.

This is the largest phase. Consider staging as a separate plan if the other
four phases are prioritized.

## Requirements

- DWC2 host controller initialization, reset, and FIFO configuration.
- USB control transfers for enumeration and hub management.
- USB bulk IN/OUT transfers for Ethernet frames.
- LAN9514 integrated hub enumeration and downstream port handling.
- LAN95xx Ethernet: MAC discovery, PHY/link, RX/TX, multicast.
- Raw Ethernet frames delivered via the existing NIC Driver Cell IPC
  contract (`TX`, `RX`, `GETMAC`).
- Net service DHCP acquires an IP address and pings an external host.
- AArch64 data-cache DMA synchronization primitives.
- Physical RPi3 Ethernet link/DHCP/ping test.

## Architecture

```
Net service (smoltcp/DHCP/sockets)
  ← NIC IPC (TX/RX/GETMAC) →
LAN9514 Driver Cell
  ← USB bulk/control →
DWC2 USB Host (kernel or driver cell)
  ← MMIO + IRQ →
BCM2837 DWC2 at 0x3F980000
  ← physical USB bus →
LAN9514 (integrated hub + ETH function)
  ← PHY →
RJ45 Ethernet
```

### Sub-phases

| Sub | Name | Effort |
|-----|------|--------|
| 5a | BCM2837 DWC2 MMIO/IRQ/DMA substrate | 2d |
| 5b | DWC2 host controller core | 4d |
| 5c | USB hub enumeration | 3d |
| 5d | LAN95xx USB-Ethernet driver | 3d |
| 5e | Net service integration | 1d |
| 5f | Physical Ethernet test | 2d |

## Assumptions

- **Claim:** BCM2837 DWC2 base is `0x3F98_0000` within the existing
  peripheral aperture.
  **Confidence:** medium (inferred from BCM2835 offset; not verified on this board)
  **How to verify:** Read firmware DTB on booted RPi3, or BCM2835 datasheet.

- **Claim:** DWC2 USB IRQ is BCM2835 legacy IRQ 9 (pending bank 1).
  **Confidence:** medium (standard for BCM2835; verify on this firmware)
  **How to verify:** Read firmware DTB `interrupts` property for USB node.

- **Claim:** Cortex-A53 requires explicit data-cache clean/invalidate for
  DMA coherency with DWC2.
  **Confidence:** high (Cortex-A53 is not hardware-coherent with DWC2 DMA)
  **How to verify:** ARM Cortex-A53 TRM cache coherency section.

## Related Files

### Create
- `hal/soc/bcm27xx/src/usb.rs` (DWC2 MMIO facts)
- `hal/arch/arm/src/aarch64/cache_dma.rs` (data-cache DMA sync primitives)
- `cells/drivers/dwc2-usb/` (DWC2 host controller driver cell)
- `cells/drivers/lan9514/` (LAN95xx USB-Ethernet driver cell)

### Modify
- `hal/soc/bcm27xx/src/profile.rs` (add DWC2 base/size)
- `hal/soc/bcm27xx/src/irq_topology.rs` (add USB IRQ)
- `hal/soc/bcm27xx/src/lib.rs` (export USB module)
- `hal/arch/arm/src/aarch64/cache.rs` (add DMA barrier ops)
- `boards/raspberry-pi/3-model-b/board.rs` (add USB/Ethernet DriverIds)
- `boards/src/descriptor.rs` (add DriverId variants)
- `kernel/src/resource_registry.rs` (add DWC2 MMIO window/class)
- `hal/arch/arm/src/aarch64/bcm2835_legacy_irq.rs` (USB IRQ enable/demux)
- `hal/arch/arm/src/aarch64/trap.rs` (USB IRQ dispatch)
- `cells/services/net/src/interface.rs` (generalize NIC lookup, remove QEMU MAC)
- `cells/services/net/src/main.rs` (dynamic MAC from driver)
- `Cargo.toml` (workspace members)

## Implementation Steps

### 5a: BCM2837 DWC2 Substrate (2d)

1. Add DWC2 MMIO base `0x3F98_0000` and register-block size to
   `Bcm27xxMmioLayout`. Add layout tests.

2. Add USB legacy IRQ 9 to `Bcm27xxIrqTopology`. Update IRQ enable/pending
   helpers in `bcm2835_legacy_irq.rs`.

3. Implement AArch64 data-cache DMA synchronization in
   `hal/arch/arm/src/aarch64/cache_dma.rs`:
   - `dc_clean_range(va, len)`: DC CVAC for each cache line, DSB.
   - `dc_invalidate_range(va, len)`: DC CIVAC for each cache line, DSB.
   - `dc_clean_invalidate_range(va, len)`: DC CIVAC, DSB.
   - Cache-line alignment helpers.

4. Add `DriverId::UsbDwc2` and `DriverId::EthernetLan9514` to board catalog.
   Add both to RPi3's enabled-driver list.

5. [BLOCKED] Add `DEV_USB` MMIO device class to resource registry with the DWC2
   window for RPi3. Add boot ceiling entry.
   Blocked on: policy v3 with a signed USB-host authority byte. The v1/v2
   manifest `mmio_devices` byte is full (bits 0–7) and a plain bool cap would be
   zeroed by every `Permit` intersection (`decision_to_caps`). Until then the
   DWC2 window is intentionally absent from the allowlist and denied for all
   classes (locked by negative tests in `resource_registry_tests.rs`); boot
   ceiling rows for `/bin/dwc2-usb` and `/bin/lan9514` are `CapSet::EMPTY` and
   pinned by the boot selftest.

6. [BLOCKED] Update AArch64 trap dispatch to handle USB IRQ (currently falls
   through as unhandled GPU peripheral interrupt).
   Blocked on: an owned one-shot IRQ contract. DWC2 asserts legacy IRQ 9
   level-style and self-clears only via W1C writes to its own GINTSTS/HCINT
   registers, so a naive `irq_wait::signal_irq` route storms before the driver
   cell runs. Requires: USB authority cap (same as step 5), WaitIrq ownership
   verification of `(irq, mmio_base)` (the current gate checks only
   PcieDriver/Platform caps and ACKs a hardcoded VirtIO offset), and a one-shot
   mask/unmask protocol — ISR masks IRQ 9 in `bcm2835_legacy_irq.rs`, the
   driver's next `sys_wait_irq` unmasks after clearing GINTSTS. Read-only facts
   shipped: `USB_IRQ` const + `is_usb_irq_pending()` in
   `hal/arch/arm/src/aarch64/bcm2835_legacy_irq.rs`.

   Status of 5a: steps 1–4 complete; steps 5–6 blocked as above. Phase 05
   remains gated on these two before any 5b+ work can claim MMIO or IRQs.

### 5b: DWC2 Host Controller Core (4d)

1. Create `cells/drivers/dwc2-usb/` with appropriate cell structure.

2. Implement DWC2 core initialization:
   - Soft reset (GRSTCTL.CSRST), wait for AHB idle.
   - Configure GUSBCFG: host mode, PHY, ULPI/UTMI selection.
   - Configure GAHBCFG: DMA mode or slave FIFO, global interrupt enable.
   - Configure host port: power on, reset, detect speed.
   - Set FIFO sizes (GRXFSIZ, GNPTXFSIZ, HPTXFSIZ).

3. Implement channel-based transfers:
   - Allocate host channels (DWC2 has 8-16 channels).
   - Control transfer: SETUP → DATA IN/OUT → STATUS.
   - Bulk transfer: DATA IN/OUT with NAK retry.
   - Channel interrupt handling: transfer complete, NAK, STALL, error.

4. Implement USB device enumeration:
   - Get device descriptor (8 bytes, then full).
   - Set address.
   - Get configuration descriptor.
   - Set configuration.

5. DMA buffer management:
   - Allocate physically contiguous, cache-line-aligned buffers.
   - Clean/invalidate around DMA transfers using the cache primitives.

### 5c: USB Hub Enumeration (3d)

1. After root port detects the LAN9514 hub:
   - Get hub descriptor.
   - Power on each downstream port.
   - Wait for port status change interrupts/polling.
   - Reset each downstream port detecting a device.
   - Enumerate downstream devices (the LAN9514 Ethernet function).

2. Implement hub-class requests: `GetPortStatus`, `SetPortFeature`,
   `ClearPortFeature`.

3. Handle hub TT (Transaction Translator) for mixed-speed devices
   (optional for initial RPi3 support since LAN9514 is high-speed).

### 5d: LAN95xx USB-Ethernet Driver (3d)

1. Create `cells/drivers/lan9514/` or integrate into the USB driver cell.

2. Implement LAN95xx initialization:
   - Read chip ID (should be `0x9514` for LAN9514).
   - Write HW_CFG for bulk-in aggregation, RX/TX FIFO size.
   - Read MAC from EEPROM via vendor-specific control requests.
   - Configure PHY: auto-negotiate, wait for link.
   - Enable TX/RX, set multicast/promiscuous as needed.

3. Implement RX path:
   - Bulk IN endpoint: receive aggregated frames.
   - Parse LAN95xx RX header (status, length) to extract individual
     Ethernet frames.
   - Forward to net service via NIC IPC.

4. Implement TX path:
   - Receive frames from net service via NIC IPC.
   - Prepend LAN95xx TX header (length, first/last segment flags).
   - Bulk OUT endpoint: transmit.

5. Implement `GETMAC` IPC response with the EEPROM-read MAC address.

### 5e: Net Service Integration (1d)

1. Generalize `cells/services/net/src/interface.rs`:
   - Remove hardcoded QEMU MAC `52:54:00:12:34:56`.
   - Use `get_driver_mac()` from the NIC Driver Cell IPC.
   - Support both VirtIO/e1000 and LAN9514 as NIC backends.

2. Verify DHCP, ARP, and TCP/IP work over the LAN95xx path.

### 5f: Physical Ethernet Test (2d)

1. Deploy to RPi3 with Ethernet cable connected to a network with DHCP.

2. Verify:
   - DWC2 initializes and detects the hub.
   - LAN9514 Ethernet function is enumerated.
   - MAC address is read from EEPROM.
   - PHY link comes up.
   - DHCP acquires an IP address.
   - Ping an external host (or a local gateway).

3. Stress test: sustained TCP transfer, link disconnect/reconnect.

## Success Criteria

- [ ] DWC2 host controller initializes without hang on RPi3.
- [ ] USB root port detects LAN9514 hub device.
- [ ] Hub enumeration discovers downstream Ethernet function.
- [ ] LAN9514 MAC address reads from EEPROM correctly.
- [ ] PHY auto-negotiation completes and link LED activates.
- [ ] DHCP acquires an IP address visible in UART log.
- [ ] Ping to gateway succeeds.
- [ ] Net service operates with real MAC (not hardcoded QEMU MAC).
- [ ] AArch64 compiles without warnings.
- [ ] VirtIO/e1000 net path still works on QEMU.

## Security Considerations

- USB enumeration must validate descriptor lengths and handle malformed
  descriptors gracefully (no buffer overflows from untrusted USB devices).
- DMA buffers must be pinned and not accessible to other cells.
- The DWC2 MMIO window must be exclusively granted to the USB driver cell.
- MAC address must come from hardware, not be spoofable via IPC.

## Risk Notes

- This is the most complex phase by far. A DWC2 USB host stack is a
  substantial piece of systems software. Consider using an existing
  no_std DWC2 crate if one exists with a compatible license.
- DWC2 errata: the Synopsys DWC2 controller has numerous silicon errata
  and undocumented behaviors. Linux `drivers/usb/dwc2/` is the primary
  reference but is GPL; architecture/register-map knowledge is fair use
  but code cannot be copied.
- Cache coherency bugs are the #1 cause of DWC2 driver failures on bare
  metal. Every DMA transfer must have correct clean/invalidate barriers.
- Power sequencing: RPi3 USB power is shared with the hub. Incorrect
  port power sequencing can cause the hub to not enumerate.
- If this phase proves too large, consider a minimal "DWC2 probe +
  LAN9514 chip-ID read" milestone first, then iterate.

## Deviation Log

2026-08-23 — 5a steps 5–6 implemented as explicit denials instead of grants:

- The DWC2 MMIO window existed in the RPi3 allowlist tagged `DEV_PCIE` (wrong
  class; PCIe authority is unrelated to USB). Removed. The v1/v2 manifest
  `mmio_devices` byte is full (bits 0–7), so a `DEV_USB` bit or a plain
  `usb_host: bool` cap cannot be expressed without policy v3: any new bool is
  zeroed by every `Permit` intersection because POLICY.BIN has no byte for it.
- Boot ceiling and `with_path_caps` no longer mint `pcie_driver`/`DEV_DISPLAY`
  for `/bin/dwc2-usb` and `/bin/lan9514`; their rows are `CapSet::EMPTY`,
  pinned by the boot selftest (negative direction).
- Legacy IRQ 9 routing stays disabled: DWC2 asserts level-style and clears only
  via W1C to its own registers, so a naive `irq_wait::signal_irq` route storms.
  Read-only facts shipped (`USB_IRQ`, `is_usb_irq_pending()`); the generic
  `vi_signal_cell_irq` bridge was rejected (no `(irq, mmio_base)` ownership
  check in WaitIrq, VirtIO-hardcoded ACK at +0x64).
- Denial is locked by `resource_registry::self_test()` — a boot-time self-test
  wired into the kernel init chain and executed on every QEMU boot: positive
  control (mailbox window still authorizes), then DWC2 full-aperture and
  edge-word denial swept over every device class plus full mask, asserted
  against the PRODUCTION allowlist table.
