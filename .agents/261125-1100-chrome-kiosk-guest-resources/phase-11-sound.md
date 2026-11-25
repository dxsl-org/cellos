---
phase: 11
title: "Sound"
status: blocked
priority: P3
effort: "8-12d"
dependencies: [1]
tier: medium
checkpoint: true
---

# Phase 11: Sound

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

CellOS plays no sound at all: no driver, no service, and syscall 218 `AudioPlay` is retired.
Guest audio therefore needs a host output path first. This phase builds the shortest one that
works where CellOS runs today (as a KVM/QEMU guest) and connects Chromium to it.

## Requirements

- Functional: a driver Cell for the virtio sound device (PCI, one output stream, S16LE 48 kHz
  stereo) that registers the new service `AUDIO` and takes PCM in Grants: open, write, drain,
  close (`wire::audio`). One stream at a time; a second opener is refused. No mixer until a
  second consumer exists.
- Functional: a virtio-snd device model in the hypervisor (control, event, tx, rx queues; the
  MMIO transport's `MAX_QUEUES` goes from 2 to 4): one output stream with the same format,
  forwarding tx buffers to `AUDIO` and completing them at playback pace.
- Functional: the guest loads the virtio sound module, has an ALSA default device, and
  Chromium plays through it.
- Functional: `run-desktop.sh` adds the sound device with the host's audio backend; the lane
  records to a WAV file.
- Non-functional: with no `AUDIO` service the guest device still completes buffers (silence),
  so pages do not stall. Hardware audio (HDA) is not in this phase.

## Architecture

```mermaid
flowchart LR
  C[Chromium -> ALSA] -- virtio-snd tx --> H[hypervisor device model]
  H -- wire::audio write (Grant) --> D[driver Cell: service AUDIO] -- virtio-snd --> Q[QEMU audiodev]
```

## Assumptions

- **Claim:** the guest kernel's modloop has the virtio sound driver and Alpine's Chromium can
  output through ALSA without a sound server. **Confidence:** low. **Verify:** microvm
  reference with QEMU's `virtio-sound-device` and `-audiodev wav`; if Chromium needs
  PulseAudio or PipeWire, the guest packages must fit `/mnt/sd` (about 10 MiB free) or the
  disk layout changes first (Law 1) — stop and re-plan.
- **Claim:** QEMU 11's `virtio-sound-pci` works with CellOS's virtio-pci transport library.
  **Confidence:** medium. **Verify:** driver bring-up.
- **Claim:** completing tx buffers on a timer derived from the byte count keeps Chromium's
  audio clock steady. **Confidence:** medium. **Verify:** drift over a 5-minute tone.

## Related Files

- Create: `cells/drivers/virtio-snd/`, `libs/wire/src/audio.rs`,
  `cells/services/hypervisor/src/virtio_snd.rs`, a tone page under `tests/guests/chrome-bench/`
- Modify: `libs/abi/src/syscall.rs` (`service::AUDIO`, CP1), `kernel/src/catalog.rs` and
  launch profiles (driver row), `config/services.toml` / `autoload.toml`,
  `scripts/assemble-x86-image.py` (new Cell; re-pin the recipe), `scripts/cellos_sign/signing.py`,
  `cells/services/hypervisor/src/virtio_mmio.rs` (`MAX_QUEUES`), dispatch tables,
  `scripts/chrome-guest/init.sh`, `scripts/build-chrome-guest-image.sh`, `run-desktop.sh`,
  `docs/abi-boundary-records.md`, `scripts/unsafe-allowlist.toml` (driver MMIO)

## Implementation Steps

1. Microvm reference for the guest side; decide the package question.
2. CP1: service id, `wire::audio`, catalog rows.
3. Driver Cell against QEMU; a native tone test Cell proves the service alone.
4. Hypervisor device model; guest configuration.
5. Lane: a page plays a 440 Hz tone for 5 s; the recorded WAV has that peak and the duration.

## Success Criteria

- [ ] Lane: the WAV recorded by QEMU contains a 440 Hz peak for 5 s +/- 0.5 s, with no gap
      longer than 50 ms.
- [ ] With the sound device removed from the machine, the same page finishes without stalling.
- [ ] Driver and wire host tests, `f1-admission`, Chrome lane and the x86 Tier 3 CI jobs pass.

## Security Considerations

PCM from the guest is data, not code, but its volume is the guest's: buffer sizes and rates
are fixed by the device model, and the driver bounds what it queues. The driver Cell is a new
PCIe driver with its own catalog row and DMA under the IOMMU like the other virtio drivers.

## Risk Notes

This is the largest host-side addition in the plan and the only one with no existing code to
extend. On a physical machine it produces nothing until an HDA or USB-audio driver exists.

## Deviation Log

- **Decision (owner, 2026-11-26): deferred.** A virtio-snd driver Cell needs a re-pinned
  `scripts/assemble-x86-image.py` recipe (its reviewed SHA-256 pin blocks rollout otherwise) and
  driver MMIO rows in `scripts/unsafe-allowlist.toml`; both need owner review. Blocked on that
  review; nothing built.
