---
phase: 4
title: "Hypervisor disk and Chrome startup"
status: completed
priority: P1
effort: "1-2d"
dependencies: [1]
tier: medium
checkpoint: true
---

# Phase 4: Hypervisor disk and Chrome startup

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Remove the remaining per-request costs between the Chrome guest and the VFS, measure
time-to-UI, and finish the Tier 3 phase-4 checks that were deferred because each try cost ~18 min.

## Requirements

- Functional: VFS `read_handle_grant` stops re-opening and re-seeking the file from cluster 0
  for every 64 KiB chunk: keep the fatfs position per open handle, or cache the cluster chain
  per handle [decide in step 1 by measurement; log it].
- Functional: the hypervisor's virtio-blk advertises a larger `seg_max`/`size_max` so guest
  requests are not split into 2-segment pieces (`virtio_blk.rs` `config_read`).
- Functional: the Chrome guest no longer reports soft lockups during startup.
- Done-when: Chromium's UI appears within a measured bound recorded in the changelog
  (target ≤ 60 s from the taskbar click under KVM/VMX); Tier 3 phase-4 deferred items
  (window resize → cage mode change, first-paint and typing latency) are measured.

## Assumptions

- **Claim:** after phase 01 the dominant cost is the per-chunk `open_file` + seek in
  `backend_fat.rs:364`. **Confidence:** low. **Verify:** time the VFS call with the guest `dd`.
- **Claim:** Linux virtio-blk honours `seg_max`/`size_max` from config. **Confidence:** high.

## Related Files

- Modify: `cells/services/vfs/src/{dispatch_grants.rs,backend_fat.rs,file_handles*}`
- Modify: `cells/services/hypervisor/src/virtio_blk.rs`
- Modify: Tier 3 plan files (`.agents/261111-1700-tier3-chrome/phase-04-chromium-guest.md`)
- Modify: `docs/project-changelog.md`, `docs/guides/tier3b-linux-vm.md` (startup time)

## Implementation Steps

1. Measure where a 64 KiB `ReadHandleGrant` spends time (VFS log timestamps, one run).
2. Fix the largest cost found; re-measure; repeat once.
3. Raise virtio-blk `seg_max`/`size_max`; host test of `config_read` offsets.
4. Chrome run: time-to-UI, soft-lockup count, resize, typing latency; screenshots in evidence.
5. Update the guide's startup-time note and the changelog.

## Success Criteria

- [x] Chrome time-to-UI measured and ≤ 60 s, or the measured value and its remaining cause logged.
      — 29, 30, 32 s on a quiet host (Deviation Log, correction).
- [x] No `soft lockup` lines in the guest log of that run.
- [x] Tier 3 display lane, both hypervisor smoke lanes and the phase-04 GUI proof PASS. — At
      `3c7e719c2`: display 41/41, smoke `boot` and `host-shell`, GUI proof 268 checks.

## Security Considerations

Keeping per-handle fatfs state must not let one Cell's handle read another's file: state is
keyed by the existing (caller, handle) pair.

## Risk Notes

If time-to-UI stays far above 60 s after these steps, the next suspect is Chromium on one vCPU;
log it and propose an SMP plan rather than widening this one.

## Deviation Log

- **Decision (step 1, measured): per-file extent map, not per-handle fatfs state.** Hypervisor
  stats showed ~1.4 ms per 8 KiB guest request after phase 01; the cost was `read_at`'s
  open + `seek` (fatfs walks the chain from the first cluster on every seek). The hypervisor
  reads through `ReadHandleGrant` by path, so a per-handle position would not help; the FAT
  backend now maps each file's clusters once (`fat_extent_map.rs`) and reads the device
  directly. Keyed by first cluster (review: path-string keys missed invalidation through
  aliases); unmappable files cached as such.
- **Done: `seg_max` 2 → 16.** `size_max` stays unadvertised (no per-segment limit). Two-boot
  VirtIO persistence lane PASS (unpinned QEMU 11.1.3).
- **Surprise: the "hang" was the lane.** Three runs looked stuck: Chromium's UI was on screen
  with a focused (white) omnibox the lane's pixel check did not accept. Also a key typed right
  after the UI appeared auto-repeated ("httppppps://"); the lane now waits 15 s.
- **Result: UI after 60, 70, 71, 78 s** (four runs, KVM on WSL2; was ~132 s). The target ≤ 60 s
  is met only at the low end. Remaining cause, measured with guest `top` and hypervisor exit
  counters: guest CPU (xz squashfs decompression, Chromium start-up on one vCPU); disk reads keep
  the vCPU busy ~15 s in total, VM exits stay at ~200–600/s. Next levers, not taken here: a
  less CPU-heavy squashfs compression needs more than the fixed 256 MiB `/mnt/sd`; larger block
  transfers need the kernel to let the VFS read a Grant it owns; SMP guests.
- **Correction (re-measured, quiet host): UI after 29, 30, 32 s** with HEAD `6007bd85b`, three
  interleaved runs; the 60–78 s above came from runs made while parallel builds loaded the host
  (an A/B batch on a busy host gave 83–85 s for the same build). The target ≤ 60 s is met. The
  "remaining cause is guest CPU" conclusion was wrong: guest `/proc/stat` shows user+sys of a few
  seconds; host contention dominates the slow runs.
- **Rejected after measurement:** injecting a VirtIO completion at the notify exit instead of the
  next idle exit (completions sat pending ~30% of the time) gave 29/43/29 s vs 30/32/29 s for
  HEAD, interleaved; no gain, not committed. Yielding after every HLT exit: 94/64 s, not taken.
- **Lanes at `3c7e719c2` (KVM):** hypervisor smoke `boot` PASS (Alpine `/ #`); `host-shell` PASS
  on the headless profile image (the `hypervisor-min` image autostarts the guest, so it never shows
  `Cellos >`); phase-04 GUI proof PASS (268 checks); Chrome 13/13; display 41/41; `nvme-x86` 11/11 with
  `CELLOS_X86_ACCEL=kvm` (its FAT32 write/read-back now runs through `OP_READ_RUN` and the extent
  map; under TCG that test times out waiting for the shell, at the previous HEAD too). The final Chrome
  run's serial log has no soft-lockup lines.
