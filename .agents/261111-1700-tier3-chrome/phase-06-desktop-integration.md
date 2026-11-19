---
phase: 6
title: "Desktop image, lane and docs"
status: completed
priority: P2
effort: "2-3d"
dependencies: [5]
tier: medium
checkpoint: true
---

# Phase 6: Desktop image, lane and docs

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Ship it in the normal desktop image and gate it with one KVM lane, then make the docs tell
the truth about what runs.

## Requirements

- Functional: `assemble-x86-image.py --profile desktop` packages `/bin/hypervisor` (spawned
  by the launcher, not autostarted) and the guest kernel/initramfs; the guest disk is
  produced and attached by the packaging/QEMU scripts.
- Functional: taskbar/registry name "Chrome" (drop "(Tier 3)" from the user-facing label;
  keep the tier in docs); MIME registry `APP_CHROME` points to the launcher.
- Functional: one lane `scripts/qemu-tier3-chrome-x86.sh` (KVM only, skips with a clear
  message when VMX is unavailable): taskbar launch -> Chromium UI -> type a URL -> page
  shown -> resize -> Close -> both Cells exit.
- Non-functional: desktop boot time and memory without Chrome open are unchanged.

## Architecture

No new components; packaging and documentation only.

## Assumptions

- **Claim:** CI runners lack nested VMX, so the lane is `manual`/local tier in `lanes.toml`.
  **Confidence:** high. **Verify:** lanes.toml tiers and runner docs.

## Related Files

- Modify: `scripts/assemble-x86-image.py`, `scripts/package-desktop-x86.py`,
  `scripts/test_assemble_x86_image.py`, `cells/apps/desktop/src/apps.rs`,
  `libs/api/src/services/mime/registry.rs`, `lanes.toml` (or the lane registry in use)
- Extend: `scripts/qemu-tier3-chrome-x86.sh` + `scripts/tier3_chrome_check.py` (created in phase 5:
  taskbar launch -> lease -> Chromium UI -> HTTPS page; add resize, Close, both Cells exit)
- Modify docs: `docs/guides/tier3b-linux-vm.md`, ADR-0017 status, `docs/project-changelog.md`,
  `docs/system-architecture.md` / `docs/project-roadmap.md` if they describe Chrome

## Implementation Steps

1. Packaging + assembler tests. 2. Registry/taskbar label. 3. Lane. 4. Docs.
5. Full checks: phase-04 GUI proof, Stack & Tile lane, new Chrome lane, host tests of every
   touched package, RV64 check of touched no_std crates.

## Success Criteria

- [x] `scripts/qemu-tier3-chrome-x86.sh` 13/13 on the i7-14700 (nested KVM): lease, Chromium UI
      (154-296 s across runs), HTTPS page typed into Chromium, Maximize followed in ~10 s, Close ends
      both Cells (`evidence/phase06-*`).
- [x] phase-04 GUI proof PASS (one earlier run timed out on "Terminal raised above Files" while two
      3 GiB Chrome VMs ran beside it; baseline at HEAD and the final rerun on an idle host PASS);
      `qemu-stack-tile-test.sh` 12/12; display lane 41/41.
- [x] `test_assemble_x86_image.py` 40 OK, `test_cellos_sign.py` 68 OK. `kernel_fs.img` 29.4 MB ->
      79.7 MB, ISO 40.0 MB -> 90.3 MB (embedded guest kernel + initramfs).
- [x] Docs (guide, ADR-0017 status, ABI §17, changelog) state the startup time, 1536 MiB, one vCPU
      and ~3 GiB for the machine. Desktop boot time without Chrome was not measured.

## Security Considerations

The hypervisor Cell's manifest (`hypervisor = true`) enters the desktop image: confirm only
the Chrome launcher's spawn path can start it (spawn policy / signing), and record it.

## Risk Notes

Image size growth from the guest kernel/initramfs (~48 MB) in `kernel_fs.img`; if too large,
load them from the guest storage too (log as deviation).

## Deviation Log

- Decision (user, 2026-11-12, first answer): load the guest kernel/initramfs from `/mnt/sd` to keep
  desktop RAM unchanged. Tried (VFS `GuestFile` reader in the hypervisor) and abandoned:
  `/mnt/sd` is the fixed 256 MiB ABI partition (`api::disk::PART_FAT32_SECTORS` = 524 288) and
  the 254 MB root already fills it; the lane read past the partition (`[vfs] read error` at the
  256 MiB boundary, `abs_lba`). The root cannot shrink: Alpine's Chromium hard-links
  `libgbm -> libgallium -> libLLVM` (190 MB installed). The unused VFS reader was dropped.
- Decision (user, second answer): embed. The desktop profile packages `/vmlinux` (pinned Alpine
  kernel, 39 MB) and `/chrome-initrd.gz` (Alpine initramfs pruned to virtio_blk, squashfs and
  overlay, plus stage 1; 4.5 MB instead of 9 MB) through `Assembler.guest_inputs`.
- Deviation (NFR): desktop RAM and image size grow with the embedded guest files even when
  Chrome is closed (measured below). Removing that needs a larger `/mnt/sd` or a second guest
  volume (disk-layout ABI change, own plan).
- Decision: the assembler edit moves its reviewed SHA-256 pin (`cellos_sign/signing.py`
  `X86_CANDIDATE_RECIPE_SHA256`), as every earlier assembler change did (e.g. `3ff097739`).
- Decision: new `scripts/make-chrome-guest-disk.sh` writes the guest root disk (512 MiB, FAT32 P1
  at LBA 2048 with `chrome-guest.sqfs`); the Chrome lane boots the desktop image as assembled.
- Follow-up (done 2026-11-12): `run-desktop.sh` attaches `build/chrome-guest-disk.img` when it
  exists. VFS mounts the lowest PCI key as `/mnt/sd` (`[vfs] primary disk ... (lowest key)`), and
  `if=virtio` took 00:02 ahead of an NVMe at 00:06; the guest disk's NVMe is now created first
  (00:02) and the boot disk is an explicit `virtio-blk-pci` with `bootindex=0`; memory 3 GiB.
  Verified by booting that exact layout: `/mnt/sd/chrome-guest.sqfs` opened, guest leased, cage
  started.
- Follow-up (done 2026-11-12, user CP1): `/bin/hypervisor` lost its generic `user_target` row; only
  init, the shell's `hv` (Path route) and the Chrome launcher reach it, with its boot ceiling
  (`profiles::hypervisor_edge`, host test `only_init_the_shell_and_chrome_reach_the_hypervisor`,
  ABI record §19, `catalog.generated.json` regenerated). Display lane 41/41 with it (both `hv`
  and Chrome edges exercised).
- Measured (2026-11-12, KVM, 3 boots each): desktop Ready 1.6-1.8 s before the embed, 2.4 s after
  (one vCPU); 1.8-2.0 s vs 2.2-2.6 s (two vCPUs). Chromium's UI after 132 s with two machine vCPUs
  (154-296 s with one).
- Surprise (CONFIRMED, fixed): resize was never exercised (phase 3). Two faults: (1) cage keeps its
  mode on a DRM hotplug, so the guest init now runs a udev watcher that moves cage's output to the
  new preferred mode with `wlr-randr` (pinned 0.4.1-r0; `CHROME_GUEST_DISPLAY WxH` on serial);
  (2) Linux virtio-gpu builds its mode with `drm_cvt_mode`, which rounds the width down to 8
  pixels, and `set_scanout` refuses a resource that is not exactly the offered size: a maximized
  window offered 1278, got 1272 and stayed black. The hypervisor now offers
  `guest_display_width` (width & !7) at LAUNCH and RESIZE (ABI record §17 updated).
- Note: the serial shell does not answer while any desktop window has focus (serial bytes follow
  keyboard focus; same with Ocel, measured), so the lane proves both Cells' exit from the kernel
  log (the launcher tid the hypervisor binds to, plus two `Syscall::Exit`) instead of `ps`.
  Separately, launcher-started hypervisors no longer read the host serial input at all
  (`526a6563b`): it fed host console keystrokes to the Chrome guest.
