---
phase: 4
title: "Chromium guest image, storage and RAM"
status: completed
priority: P1
effort: "4-6d"
dependencies: [3]
tier: medium
checkpoint: false
---

# Phase 4: Chromium guest image, storage and RAM

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Build a reproducible Alpine guest disk with cage + Chromium, give the hypervisor a disk and
RAM profile large enough to run it, and boot Chromium fullscreen inside the launcher window.

## Requirements

- Functional: one pinned builder produces `chrome-guest.sqfs` (read-only squashfs, xz;
  Alpine 3.21 rootfs: cage, Chromium, fonts, dbus, eudev) with a tmpfs overlay for writes,
  and an init that loads the virtio modules, starts cage (`WLR_RENDERER=pixman`) on the
  virtio-gpu DRM device, and runs Chromium with
  `--ozone-platform=wayland --disable-gpu --no-first-run --user-data-dir=/tmp/chrome`.
- Measured 2026-11-11 (`docker alpine:3.21`, `apk add chromium cage eudev font-dejavu`):
  installed 747 MB (Chromium itself 238 MB, `libLLVM` 154 MB pulled by mesa, `libgallium`
  38 MB, ICU data 30 MB, ffmpeg ~21 MB, Alpine base ~8 MB); tar+gzip 277 MiB, tar+xz 199 MiB.
  Target: squashfs <= 200 MiB; then try dropping mesa/LLVM (≈ -190 MB installed) only if
  Chromium + cage still show pages without them (verify, never assume).
- Functional: hypervisor profile `chrome-guest`: RAM >= 1536 MiB (measured; raise if
  Chromium OOMs), root on the virtio-blk disk, gui devices on.
- Functional: the disk reaches the hypervisor through CellOS storage, not the kernel ELF.
- Non-functional: builder pins package versions and checks SHA-256 of every download; output
  checksum recorded. Replaces `build-linux-base-image.sh` + `build-chrome-app.sh`
  (two-disk `/dev/vdb` design dropped: the hypervisor has one disk).

## Architecture

Storage choice (decide in step 1 with the squashfs size, log it):
- (A, preferred) a dedicated guest partition / second data disk exposed by VFS as
  `/mnt/guest/chrome-guest.sqfs`, sized for the image plus headroom;
- (B) grow `/mnt/sd` P1 beyond 256 MiB in `tools/write-mbr.py` (a ~200 MiB squashfs does
  not fit beside the existing P1 contents with margin).
QEMU for desktop/lanes gains the guest disk drive and `-m 3G`.

## Assumptions

- **Claim:** the Alpine `virt` kernel/initramfs provides `squashfs` (xz) and `overlay` modules.
  **Confidence:** high. **Verify:** list them in `initramfs-virt` / `modules.dep`.
- **Claim:** Alpine 3.21 Chromium runs under cage with pixman renderer on virtio-gpu DRM
  without GPU acceleration. **Confidence:** medium. **Verify:** boot the image under plain
  QEMU/KVM first (no CellOS) with virtio-gpu, then under the CellOS hypervisor.
- **Claim:** `MapGuestMemory` and the kernel can back 1.5 GiB of guest RAM. **Confidence:** low.
  **Verify:** boot with the profile, read guest `free -m`.
- **Claim:** the persistent-disk backend's read path is fast enough to start Chromium in < 60 s
  under nested KVM. **Confidence:** low. **Verify:** time to first Chromium frame in the lane.

## Related Files

- Create: `scripts/build-chrome-guest-image.sh` (pinned), guest `init` script it installs
- Delete: `scripts/build-linux-base-image.sh`, `scripts/build-chrome-app.sh`, `apps/chrome/chrome.toml` (if unused)
- Modify: `cells/services/hypervisor/src/boot_x86_profile.rs` (profile), `persistent_disk.rs`
  (path/size), `Cargo.toml` features, storage/VFS config or `tools/write-mbr.py` (per decision)

## Implementation Steps

1. Measure: image size, Chromium RSS at a blank page; pick storage A/B; log the decision.
2. Write the builder (Docker `alpine:3.21` pinned digest, apk with pinned versions).
3. Validate the image on plain QEMU/KVM with virtio-gpu (baseline: Chromium visible).
4. Add the hypervisor profile and disk path; boot under CellOS; screenshot evidence.
5. Changelog entry with sizes and boot time.

## Success Criteria

- [x] Builder run twice gives the same package set (manifest diff empty). — Two runs, identical
      manifest (Deviation Log; changelog 2026-11-12, phase 4).
- [x] Under CellOS (KVM/VMX) the Chrome window shows Chromium's real UI (tab strip,
      omnibox) — screenshot in evidence; guest `free -m` logged. — `evidence/phase04-chromium-in-window.png`;
      1475 MiB visible to the guest (changelog 2026-11-12, phase 4).
- [x] Hypervisor host tests still pass; console lanes unchanged. — `service-hypervisor` host tests
      PASS at `c4c7d5d34f` (2026-11-19); hypervisor smoke `boot` PASS at `3c7e719c2`
      (multi-sector plan phase 04).

## Security Considerations

Chromium runs `--no-sandbox` inside the guest only because the VM boundary contains it
(ADR-0017); state this in the guide. The image contains no credentials; the user-data dir is
tmpfs (no persistence in this plan).

## Risk Notes

One vCPU may make Chromium sluggish; measure first-paint and typing latency and record them.
An SMP guest is a separate plan if measured unusable.

## Deviation Log

- Decision (storage): option A in practice — the image is a file on the CellOS SD volume
  (`/mnt/sd/chrome-guest.sqfs`, FAT32 P1 at LBA 2048, the same path the Ubuntu lane uses for
  `guest_disk.img`), opened read-only (`DiskSpec`, `VIRTIO_BLK_F_RO`, OUT refused).
- Deviation: squashfs is 254 MiB (target 200): Chromium 238 MB + mesa/LLVM + the guest kernel's
  modules (20 MiB modloop, evdev is not in the initramfs and v3.21 no longer carries the
  6.12.81 linux-virt package). Image bytes differ between runs (install scripts); the package
  manifest is identical (success criterion).
- Decision: profile chosen at runtime (`GuestProfile`): Chrome when `hypervisor app` and no test
  fixture; 1536 MiB, `/chrome-initrd.gz` (Alpine initramfs + stage1), read-only disk.
- Surprise (CONFIRMED): guest time nearly stopped (`sleep 2` took minutes): the guest cannot
  calibrate its TSC and runs on jiffies, which lose ticks while the vCPU waits on device I/O.
  Fix: hypervisor measures the TSC and passes `tsc_early_khz=… tsc=reliable` (all x86 profiles).
- Surprise (CONFIRMED): with a known TSC the guest chose the TSC-deadline clockevent, whose MSR
  writes the VMM drops; boot stopped after the virtio-gpu probe. Fix: CPUID leaf 1 bit 24 hidden.
- Surprise (CONFIRMED, open): guest disk reads run at ~2 MB/s (`dd` 48 MiB in 24-25 s) with
  4 KiB or 64 KiB FAT clusters alike: the block wire moves one 512-byte sector per IPC round
  trip (`types::block_wire`, VFS `BlockStream::read`). udev alone took ~90 s.
- Deviation: window resize (phase 3 RESIZE -> cage mode change) and first-paint/typing latency are
  not measured yet: each attempt costs ~18 min of startup. Deferred until the multi-sector block
  plan lands. A lane for this phase is deferred to phase 6 for the same reason.
