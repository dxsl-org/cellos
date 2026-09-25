# ADR-0020 — Stage USB HID isolation behind a reliable transport

> **Status**: Accepted 2026-09-25.
> **Supersedes**: The incomplete per-interface `/bin/dwc2-hid` split described in the Unreleased changelog.

## 1. Context

The RPi3 DWC2 cell owns the only USB MMIO/DMA authority and transports both LAN9514 Ethernet and HID traffic through one physical controller. A first isolation attempt spawned one capability-free HID parser per interface. The host still parsed the HID report descriptor and retained a fallback decoder, while each interrupt report crossed a one-shot `TrySend` boundary. Physical boots proved enumeration and worker attachment but lost keyboard input.

The split therefore paid for extra cells, IPC, lifecycle state, and duplicated decoders without removing untrusted HID parsing from the privileged host or guaranteeing ordered report delivery.

Two risks also remain even with a correct HID worker:

1. A DWC2 transport or controller failure still interrupts both NIC and HID because the LAN9514 functions share one physical controller.
2. A hostile USB device can attack enumeration, transfer scheduling, hub state, and DMA before a class worker receives a report.

## 2. Decision

### 2.1 Current embedded profile

The RPi3 embedded profile enumerates controlled HID devices at boot and decodes
reports synchronously inside `/bin/dwc2-usb`. It keeps one decoder state per
interface and emits device-identified events. `/bin/dwc2-hid` is removed.
`/bin/lan9514` remains a capability-free protocol front-end. Runtime HID
reconnect is intentionally unsupported: physical detach requires reboot.

This profile is production-eligible only for controlled embedded peripherals
after parser fuzzing, bounded-resource checks, recovery tests, and physical
qualification. It is not the hostile-hotplug security profile.

### 2.1.1 Deferred full HID plug-and-play

A periodic rescan of only boot-claimed HID ports is not plug-and-play. It lacks
topology discovery, debouncing, per-port lifecycle, bounded backoff, complete
lease revocation, and qualification under concurrent Ethernet traffic. The
embedded profile therefore omits limited reconnect support rather than claiming
partial plug-and-play. The required full state machine and qualification matrix
are specified in `docs/roadmap/usb-isolation.md`.

### 2.2 Future secure USB profile

Full isolation is a separate implementation milestone. The DWC2 cell becomes a minimal host-controller driver: MMIO/DMA, generic USB enumeration, endpoint ownership, and transfer scheduling only. It does not parse HID report descriptors or decode class reports. One capability-free USB HID service owns all HID descriptor parsing and per-device decode state unless a later threat model requires per-interface workers.

The split may land only with:

- reliable bounded ordered queues or SPSC rings in both directions;
- explicit worker-ready acknowledgement before endpoint polling;
- device generation IDs and sequence numbers;
- backpressure that pauses an interface instead of dropping accepted reports;
- non-blocking operation of the shared DWC2/NIC host;
- dedicated Input-producer authority rather than `NIC_DRIVER` identity;
- deterministic detach/crash cleanup and source revocation.

### 2.3 Shared-controller failure

Software isolation cannot make Ethernet and HID independently available when one physical DWC2 controller or LAN9514 hub fails. The secure profile contains the failure outside the kernel and other cells, performs a bounded controller reset, revokes all affected device generations, and restarts class services. A product requiring independent NIC/HID availability must use separate physical controllers.

### 2.4 Hostile USB and DMA

Class workers do not protect the HCD from malicious enumeration or transfer behavior. The HCD must validate descriptor lengths, endpoint types, addresses, split schedules, and transfer quotas before programming hardware. DMA uses kernel-owned bounded bounce/grant buffers; arbitrary cell memory is never exposed. Platforms with an IOMMU/SMMU additionally place the HCD in a restricted DMA domain. Pi 3 lacks that hardware guarantee, so its secure posture depends on bounded bounce DMA and controlled peripherals.

## 3. Rejected alternatives

- **Keep the current hybrid worker design and patch `TrySend`.** Rejected because descriptor parsing and fallback decoding remain inside the privileged host, decoder state is duplicated, and a reliable queue alone would not deliver the claimed isolation.
- **Use blocking host-to-worker sends.** Rejected because a crashed or stalled parser would block the shared DWC2 loop and therefore Ethernet, defeating fault containment.
- **One worker per interface immediately.** Rejected because it multiplies stacks and lifecycle state before a threat model requires device-to-device availability isolation. One HID service is the default future boundary.
- **Keep single-cell HID forever.** Rejected as the universal security architecture because safe Rust reduces memory-corruption risk but does not contain panic, CPU/heap denial, or hostile class-parser faults.

## 4. Consequences

- The current RPi3 path becomes smaller, ordered, and suitable for weak embedded hardware.
- Current production claims must say “controlled peripherals”; arbitrary hostile USB remains unqualified.
- Full isolation depends on transport, authority, DMA, and recovery primitives rather than merely creating another process.
- A shared-controller reset remains a joint NIC/HID outage on Pi 3; this is an explicit hardware limitation, not a software promise.

## 5. Cross-references

- Implementation plan: `.agents/260925-1554-staged-usb-hid-isolation/plan.md`
- Security roadmap: `docs/roadmap/usb-isolation.md`
- Input protocol: `docs/input-api.md`
- Hardware isolation layers: `docs/specs/19-hardware-isolation-layers.md`
