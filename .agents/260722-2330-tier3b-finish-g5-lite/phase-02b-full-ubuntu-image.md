# Phase 02b — Full Ubuntu + systemd + apt-persist image pipeline (T2)

- **Track:** A (finish Tier 3b) · **Label:** **coding** — ARM64 TCG validatable · **Tier:** thinking · **Effort:** XL (~1.5-2.5K LOC + image-build infrastructure) · **Depends:** P02

## Context Links
- User scope decision #2: **wide guest = FULL Ubuntu + systemd + apt-persist, NOT minimal glibc.** Red-team M9: categorically bigger than the 2-3K "minimal glibc" — its own realistic estimate.
- Builds on the [P02](phase-02-wide-guest-substrate.md) substrate (writable blk, root-on-blk boot, large RAM carve, RTC/RNG/net).

## Overview
- **Priority:** P2 · **Status:** pending
- Deliver an init-system-capable full Ubuntu guest: image build pipeline, systemd boot to multi-user target, and `apt install` persistence across reboot.

## Key Insights
- This is an **image + integration** phase more than a VMM phase — the VMM substrate is P02. The bulk is: an image build pipeline (debootstrap/mmdebstrap → ext4 image), enough emulated devices/timers for systemd to reach a target, and a persistent writable rootfs.
- systemd is demanding: needs a working monotonic + wall clock (RTC), `/dev` population, a functioning block device with a real filesystem (ext4), and enough RAM headroom. Each missing piece = a boot hang, not a clean error.
- `apt` needs network-in-guest (P02 net wiring) + persistent writable root (P02 blk) + working DNS/time. Persistence is the acceptance test.
- CI cost: a full Ubuntu boot under TCG is minutes, not seconds — dual-lane (Alpine fast smoke + Ubuntu slow nightly) with a named maintenance owner.

## Requirements
- **Functional:** full Ubuntu boots via systemd to a shell/login; `apt update && apt install <pkg>` succeeds; the installed package survives a guest reboot.
- **Non-functional:** image build is reproducible + scripted; Ubuntu lane is a separate (nightly) CI job; Alpine fast lane unaffected.

## Architecture
Image pipeline: debootstrap/mmdebstrap → ext4 rootfs image → per-VM writable backing (P02 mechanism). Boot: root-on-blk (`root=/dev/vda rw`) → systemd → multi-user.target. Persistence: writes land in the per-VM image file, re-read on next boot.

## Related Code Files
- **Add:** image build script (`scripts/make-ubuntu-guest.sh` or extend `make-hypervisor-fs.sh`).
- **Possibly extend:** `loader_image.rs` (larger image streaming — mind the quadratic FAT re-seek at `loader_image.rs:130-134`), guest device set (ensure systemd prerequisites), `main.rs` RAM headroom.
- **CI:** add an Ubuntu nightly lane.

## Implementation Steps
1. Image build pipeline (debootstrap/mmdebstrap → ext4), scripted + reproducible.
2. Verify systemd prerequisites (RTC, /dev, block+ext4, RAM) — fill gaps found during boot bring-up.
3. Boot to multi-user.target; reach shell/login.
4. `apt install` + reboot + verify persistence.
5. Ubuntu nightly CI lane + maintenance owner.

## Todo
- [ ] reproducible Ubuntu image build pipeline (ext4)
- [ ] systemd prerequisite audit + gap-fill
- [ ] boot to multi-user.target
- [ ] apt-install-then-reboot persistence test
- [ ] Ubuntu nightly CI lane + owner

## Success Criteria
- Build + boot + run (ARM64 TCG): full Ubuntu boots via systemd; `apt install` succeeds and **persists across reboot**; Alpine fast lane still green.

## Risk Assessment
- **High:** systemd stalls on an unemulated prerequisite (clock, device) with no clean error → hard-to-diagnose boot hang. Mitigation: incremental bring-up (getty before full target); serial-log the systemd unit progression.
- **Med:** full-Ubuntu-under-TCG boot time balloons CI. Mitigation: nightly-only lane; Alpine remains the fast per-PR smoke.
- **Med:** large image streaming hits the quadratic FAT re-seek (`loader_image.rs:130-134`) → slow load. Mitigation: chunk-size tuning already large; measure, consider a stateful handle if it dominates.

## Security Considerations
- Same per-VM backing isolation as P02 (never shared cell-store). A full Ubuntu guest has a larger attack surface inside the guest — irrelevant to host isolation (guest is confined by S2 + IOMMU), but relevant to the "not an untrusted-hosting moat" positioning.

## Next Steps
- The full Ubuntu guest is the Wide preset's reference guest (P04). It is a poor CoW-golden candidate (huge dirty set) — CoW value is with minimal guests.
