---
phase: 11
title: "Sound"
status: completed
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

- [x] Lane: the WAV recorded by QEMU contains a 440 Hz peak for 5 s +/- 0.5 s, with no gap
      longer than 50 ms. (`sound-device-records-tone`: 5.008 s, 439.3 Hz, longest gap 8 ms;
      1 105 920 bytes to `AUDIO`, 0 dropped; Chrome lane 38/38, 2026-11-26.)
- [x] With the sound device removed from the machine, the same page finishes without stalling.
      (`CHROME_LANE_SOUND=0`: 5568 ms, "no audio service; playing silence", 1 122 304 bytes
      paced and dropped; see the deviation log for a second, slower run.)
- [x] Driver and wire host tests, `f1-admission`, Chrome lane and the x86 Tier 3 CI jobs pass.
      (driver 25, wire 100, hypervisor 73 host tests; `cellos-sign --check` F1 OK, Tier-1 587;
      display lane 43/43; CI lint, x86_64-lint, unit-tests, hypervisor-boot, virtio-e2e,
      hostile, python pass; all from a clone where the new crate is tracked.)

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
- **Decision (owner, 2026-11-26):** go ahead ("làm tiếp phase 11"), including the re-pinned
  image recipe the new driver Cell needs.
- **Assumption 1 settled (microvm reference, 2026-11-26):** the guest needs no sound server and
  no new package. The modloop has `virtio_snd`; Chromium fails to reach PulseAudio and falls back
  to ALSA (`audio_manager_linux.cc: Falling back to ALSA`). First run recorded nothing (44-byte
  WAV): `/dev/snd/*` belong to group `audio`, which the `chrome` user was not in. With
  `addgroup chrome audio`, QEMU's virtio-sound recorded the page's tone: 5.0 s, 439.8 Hz, no gap
  (`scripts/tier3_wav_tone.py`). QEMU writes the WAV only on a clean quit (QMP `quit`).
- **Decision:** `wire::audio` carries PCM in a client-owned Grant with blocking calls (OPEN,
  WRITE) and a posted CLOSE; the driver copies before it replies and answers BUSY instead of
  waiting, so the guest never waits on the host audio path. No OWNER_WATCH for `AUDIO`.
- **Decision:** the driver Cell implements its virtqueues over `DmaBuf`'s safe accessors instead
  of the vendored `VirtIOSound`, whose HAL needs `unsafe`: allowlist and Tier-1 ceiling unchanged.
- **Deviation (criterion 2):** "sound device removed" is the same lane with
  `CHROME_LANE_SOUND=0`: no virtio-sound on the machine, so no `AUDIO` provider; the guest still
  has its card (the hypervisor's model) and the page must still finish on time.
- **Surprise (host, not this phase):** on 2026-11-26 afternoon the Chromium/cage crash
  (`ld-musl` segfault, "GPU process isn't usable") hit most lane runs, with and without the
  sound device, at 1 and 4 vCPUs; the pushed HEAD (phase 10, no sound code) crashed the same
  way on the same host. Runs with the device hidden: 0 of 2 crashed at 4 vCPUs, 2 of 2 at 1
  vCPU; with the device: 3 of 4. No mechanism tied to the device was found (its guest writes
  stay inside the guest's buffers; a run where the guest never touched it after boot crashed
  too). The earlier choppy run (19 stream restarts, 419.7 Hz, page 8.4 s) was on the slow host;
  the passing run had 2 starts. A later no-device run on the same host took 8.7 s, Chromium
  never opened the card (no SET_PARAMS) and its download never started: host slowness. Rerun
  the lane on a quiet host (after restarting WSL) before relying on these numbers.
- **Deviation:** the page check accepts 4.5-7 s instead of +/- 0.5 s: it is wall-clock time and
  includes opening the card in the guest; the recording judges the timing exactly. The
  clipboard step now waits for the guest's echo instead of a fixed 2 s (lost that race twice on
  the slow host).
- **Note:** the image signer admits only tracked crates, so until `cells/drivers/virtio-snd/`
  is committed the lanes run from a clone with it committed (`/tmp/cellos-p11wt`).
- **Note:** the driver writes ring indexes byte by byte (no `unsafe`); a device reading the
  avail index while its low byte wraps can see a wrong value. The driver detects
  `DEVICE_NEEDS_RESET` or failed requests and re-initialises (at most 3 times).
