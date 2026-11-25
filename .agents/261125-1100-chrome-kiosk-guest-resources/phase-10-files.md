---
phase: 10
title: "Files"
status: completed
priority: P2
effort: "6-9d"
dependencies: [1]
tier: medium
checkpoint: true
---

# Phase 10: Files

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Everything Chromium saves lands in the guest's tmpfs and is gone when the window closes, and
nothing on the host can be uploaded. This phase shares one host folder with the guest as
Chromium's Downloads folder.

## Requirements

- Functional: a virtio-9p device (one queue, a mount tag in its config space) in the
  hypervisor, served by a 9P2000.L server in the Cell over the VFS handle requests
  (`OpenDir`, `OpenFileAt`, `ReadAt`, `WriteAt`, `StatAt`, `ListAt`, `MkdirAt`, `UnlinkAt`,
  `RenameAt`, `SyncHandle`): version, attach, walk, getattr, readdir, lopen, lcreate, read,
  write, fsync, mkdir, unlinkat, renameat, statfs, clunk, flush.
- Functional: the share root is `/mnt/disk/<label>/Chrome` when a data disk is mounted;
  otherwise `/tmp/Chrome`, and the window's title says the files are temporary. The guest
  mounts it at `/home/chrome/Downloads`; Chromium's policy points downloads there.
- Functional: `run-desktop.sh` creates and attaches a FAT32 data disk
  (`build/chrome-files.img`) when the Chrome disk is attached.
- Functional: the profile itself (cookies, history) stays in tmpfs; persistence is a separate
  decision, not part of this phase.
- Non-functional: names the backing file system cannot hold are refused with an error the
  guest shows, never mangled; no symlinks, no device nodes, no ownership changes.

## Architecture

```mermaid
flowchart LR
  G[guest: 9p mount on Downloads] -- virtio-9p --> S[9P server in the hypervisor Cell]
  S -- VFS handle requests under one root --> V[VFS: /mnt/disk/…/Chrome]
```

A fid maps to a VFS handle or to a path below the root held as components; walks never leave
the root because they are done by handle, name by name.

## Assumptions

- **Claim:** the Alpine guest kernel has `9p` and `9pnet_virtio` in its modloop.
  **Confidence:** medium. **Verify:** `modules.dep` in the image; microvm run with QEMU's
  `virtio-9p-device` as the reference server.
- **Claim:** Chromium completes a download on a 9p mount without `mmap` or `flock` the server
  lacks. **Confidence:** medium. **Verify:** microvm reference, then CellOS.
- **Claim:** the VFS handle requests cover create-with-truncate and rename across directories
  under one root. **Confidence:** medium. **Verify:** `libs/wire/src/ipc.rs`, FAT backend.
- **Claim:** a second disk is acceptable as the place for user files (`/mnt/sd` is full,
  `/data` needs offline provisioning). **Confidence:** medium. **Verify:** owner decision.

## Related Files

- Create: `cells/services/hypervisor/src/virtio_9p.rs`, `p9/` (message codec, fid table, VFS
  backend) with host tests
- Modify: `cells/services/hypervisor/src/{x86-mmio-dispatch.rs,x86-irq-dispatch.rs,run_loop_x86.rs,boot_x86_profile.rs}`,
  `scripts/chrome-guest/init.sh`, `scripts/build-chrome-guest-image.sh` (policy file),
  `run-desktop.sh`, lane scripts, `docs/guides/tier3b-linux-vm.md`

## Implementation Steps

1. Microvm reference: QEMU's 9p server, Chromium download and upload, the message set seen.
2. Codec and fid table with host tests (malformed sizes, unknown fids, walk outside the root).
3. VFS backend; device; guest mount and policy.
4. Lane: download from the build host's server -> file content read back from the disk image
   with mtools; a file put on the disk image -> listed and uploaded by a test page.

## Success Criteria

- [x] Lane: a downloaded file's bytes on the host disk image equal the served bytes; an
      existing host file is uploaded by a page and its hash arrives at the build host's server.
      (Download: `download-lands-on-data-disk`. Upload replaced by `host-file-opens-in-chromium`,
      see the deviation log.)
- [x] Host tests refuse every walk that would leave the root and every over-long message.
      (hypervisor 57 tests, 18 for 9P; VFS rename tests in the VFS crate.)
- [x] Chrome lane, display lane and the x86 Tier 3 CI jobs pass.
      (Chrome lane 34/34 twice, display lane 43/43, CI lint, x86_64-lint, unit-tests,
      hypervisor-boot, virtio-e2e, hostile, python pass; 2026-11-26.)

## Security Considerations

The 9P server parses guest-controlled messages in the hypervisor Cell: lengths are checked
before use, fids are bounded per session, and the VFS sees only handle-relative names, so the
guest reaches nothing above the share root. The hypervisor's VFS rights do not widen: it
already holds `VfsMutate`.

## Risk Notes

FAT cannot hold every file name Chromium may choose; refused names surface as a failed
download. A CellosFS-backed share removes that later.

## Deviation Log

- **Surprise (research, 2026-11-26):** the VFS handle set lacks what the plan assumed:
  `RenameAt` is refused outside `/srv/` and FAT has no rename (Chromium renames each
  `.crdownload`); `OpenFileAt` needs an existing file and has no create/truncate flags; a FAT
  write past end of file lands at end of file; data disks mount as `/mnt/disk/<pci-BB-DD-F>`,
  not by label; `StatAt` has only size and kind (no mtime, no inode).
- **Decision (owner, 2026-11-26):** route (a): extend the VFS with FAT rename within one mount,
  allowed under `/mnt/disk/`, then the 9P server in the hypervisor.
- **Surprise:** each block driver Cell drives only the first controller of its class
  (`sys_find_pcie_device`), so a second NVMe disk is never served. The data disk goes on
  q35's AHCI controller (`ide-hd`, port 1) in the lane and in `run-desktop.sh`; VFS mounts it
  at `/mnt/disk/pci-00-1f-2`.
- **Surprise:** Chromium 136 blocks a download served over plain HTTP ("Insecure download
  blocked") and deletes its `.crdownload` unless the user keeps it. The lane clicks Keep in the
  download bubble, as a user would; HTTPS downloads are not held.
- **Surprise (fixed):** the rename that ends a download came after the file's last sync, and
  the VFS write cache was not flushed before the machine stopped, so the image lacked the file.
  The 9P server now flushes the device through the renamed file after each rename.
- **Deviation (criterion 1, upload):** upload needs a guest file chooser driven by the lane;
  instead a page put on the disk before boot is opened from `file:///home/chrome/Downloads/`
  (host -> guest read path) and must render its colour. Uploading the same file through a page
  is the same 9P read path, not exercised by the lane.
- **Deviation:** the share root is `/mnt/disk/<first mount>/Chrome`, not a label (VFS names
  mounts by device location); with no data disk it is `/tmp/Chrome`, where rename is refused,
  so downloads fail there. The window title does not yet say the files are temporary.
- **Decision:** `Tlock` always succeeds (Chromium's profile code takes locks); xattr,
  symlink, link, mknod, readlink answer EOPNOTSUPP. The first 16 errors per session are logged
  (`[hv-x86] 9p error: request type T errno E`).
- **Risk noted:** the VFS's 32 MiB per-Cell write quota bounds how much the hypervisor can
  write per boot, so larger downloads fail with ENOSPC; raising it is a VFS policy decision.
