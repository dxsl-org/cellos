# USB isolation roadmap

## Outcome

Provide two explicit profiles rather than one compromised hybrid:

- **Embedded/controlled USB:** synchronous HID decode in the DWC2 host, qualified devices, minimal memory and scheduling cost.
- **Secure/hot-plug USB:** minimal privileged HCD plus isolated class services over reliable bounded transport.

## Full HID plug-and-play design

Full plug-and-play is deliberately not a periodic rescan of ports that happened
to hold HID devices at boot. It is a complete hub-topology and device-lifecycle
implementation, gated on the secure profile's reliable transport and
generation-scoped Input authority.

- Observe connection changes for every product-authorized hub port; debounce
  connect and disconnect before control traffic.
- Keep an explicit per-port state machine: `Absent`, `Debouncing`, `Resetting`,
  `Addressing`, `Describing`, `Configuring`, `Running`, `Removing`, and
  `Backoff`. Every transition has a bounded timeout, retry ceiling, and
  observable failure reason.
- Allocate an address, endpoint/channel lease, and monotonic device generation
  only after reset succeeds. A detach atomically stops endpoint traffic, revokes
  the Input producer generation, releases only that generation's held
  keys/buttons, and returns the leases after completion.
- Classify descriptors under explicit product policy. A new HID device may be
  admitted; a non-HID device must not disturb the LAN9514 function or consume
  HID resources. Restore the current LED state only after the interface is
  configured and ready.
- Retry transient enumeration failures through bounded backoff without blocking
  input polling or Ethernet. A reconnect must remain pending until its deadline,
  rather than depending on another physical connection-change latch.
- Qualify the state machine on physical detach during held keys/buttons, rapid
  reconnect, composite receivers, hub reset, Input restart, address exhaustion,
  malformed descriptors, and concurrent Ethernet traffic.

The RPi3 embedded profile intentionally omits this feature: it enumerates HID
at boot and requires reboot after a physical reconnect. No runtime HID
plug-and-play claim is made until the above design and qualification gate land.

## Why a HID worker alone is insufficient

A class worker contains HID parser failure, but it cannot solve two lower-layer risks:

1. **Shared controller fault domain.** DWC2 and the LAN9514 hub physically couple HID and Ethernet. A controller reset interrupts both. Software can protect the kernel and recover the controller, but cannot promise independent availability.
2. **Hostile traffic before class dispatch.** Enumeration, generic descriptors, hub/split scheduling, endpoint state, and DMA are processed by the HCD before HID sees a report. These paths need their own validation and confinement.

## Secure profile target

```text
kernel policy / DMA confinement
            |
       minimal dwc2-hcd
       /             \
reliable HID rings   reliable LAN transport
       |                     |
  usb-hid service       lan9514 service
       |
dedicated InputProducer capability
       |
     Input
```

## Phase S1 — Harden the single-cell baseline

- Remove `/bin/dwc2-hid` and duplicated decoder state.
- Keep device ID and generation-aware pressed-key cleanup.
- Make parser limits explicit: descriptor bytes, field count, report IDs, collection depth, events per report, and CPU budget.
- Replace data-dependent panic paths with typed rejection.
- Fuzz descriptor parsing, report decoding, and LED report construction.
- Qualify keyboard, mouse, composite receiver, disconnect/reconnect, lock LEDs, and concurrent Ethernet.

**Gate:** controlled-device RPi3 image passes physical input and recovery tests.

## Phase S2 — Reliable bounded device transport

- Add kernel-supported bounded SPSC rings or equivalent reliable mailboxes.
- Preserve report order with monotonically increasing sequence numbers.
- Tag every message with controller/device/interface generation.
- Require worker-ready ACK before polling.
- On a full HID report queue, stop polling that interface; never discard a report already accepted from hardware.
- Keep the DWC2 loop non-blocking when a class service or Input stalls.
- Define explicit overflow policy for relative pointer traffic; key/button transitions remain lossless.

**Gate:** stress tests prove ordered press/release delivery, bounded memory, crash recovery, and no NIC starvation.

## Phase S3 — Dedicated authority and lifecycle

- Add a USB-host identity separate from `NIC_DRIVER`.
- Delegate a generation-scoped `InputProducer` capability to the HID service.
- Revoke it atomically on detach, worker crash, controller reset, or policy change.
- Replace stale TID source tables with service generation/revocation tracking.
- Release exactly the keys owned by the revoked device generation.

**Gate:** negative tests reject forged device IDs, stale generations, unauthorized producers, and replay after detach.

## Phase S4 — Minimal HCD / isolated class services

- Move HID report-descriptor parsing, report decoding, state tracking, and LED report construction completely out of the HCD.
- Keep only bounded generic USB descriptor parsing in the HCD.
- Default to one HID service for all interfaces; introduce per-interface workers only if a product requires one malicious HID device not to interrupt another.
- Keep LAN protocol policy outside the HCD.

**Gate:** malformed HID descriptors can terminate/restart the HID service without stopping DWC2 or LAN traffic.

## Phase S5 — Host-controller and hostile-device containment

- Validate USB addresses, endpoint types/MPS/intervals, descriptor lengths, hub routes, split timing, transfer sizes, retry ceilings, and per-device bandwidth before programming DWC2.
- Use only kernel-owned bounded DMA bounce/grant buffers; verify ownership, direction, length, and cache-sync completion for every transfer.
- On IOMMU/SMMU platforms, restrict the HCD DMA domain to its rings and bounce pool.
- Apply per-device control/interrupt/bulk quotas and watchdog deadlines.
- On controller fault, revoke all generations, quiesce DMA, perform a bounded reset, rebuild topology, and restart class services.
- Treat Pi 3 without an IOMMU as controlled-peripheral production; hostile USB qualification requires stronger hardware or an external USB firewall/controller.

**Gate:** fuzzed enumeration and fault-injection cannot corrupt kernel/other-cell memory; recovery is bounded and auditable.

## Phase S6 — Product profiles

| Profile | USB trust | Architecture | Availability claim |
|---|---|---|---|
| Embedded Pi 3 | allowlisted/controlled peripherals | single-cell DWC2 + HID | joint NIC/HID restart accepted |
| Secure appliance | authenticated or physically controlled ports | minimal HCD + isolated HID service | kernel and unrelated services survive class failure |
| Hostile hot-plug | arbitrary peripherals | isolated HCD + class services + IOMMU/SMMU or external confinement | qualified only on hardware that can constrain DMA |

No profile may claim independent NIC/HID availability on the shared Pi 3 controller.
