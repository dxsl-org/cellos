# Phase 02 — Wide-guest VMM substrate + minimal-glibc boot (T1)

- **Track:** A (finish Tier 3b) · **Label:** **coding** — fully QEMU-TCG validatable on ARM64 · **Tier:** thinking · **Effort:** L (~2-3K LOC + build scripts)

## Context Links
- Folds `.agents/260712-0952` P04 (writable storage) + the boot-substrate half of P05, red-teamed there (F1/F2/M1/A2). User decision: glibc/Ubuntu guest + writable virtio-blk + overlay.
- Splits from the original single "wide guest" phase per red-team M9 + user scope decision #2: **this phase = the VMM-side substrate + a minimal-glibc boot (T1)**; the full Ubuntu+systemd+apt image is [phase-02b](phase-02b-full-ubuntu-image.md) (T2).
- Current: Alpine only (`scripts/make-hypervisor-fs.sh`), initramfs→`/bin/sh`, RAM 128 MiB (`main.rs`), disk = volatile Vec (`virtio_blk.rs:15`).

## Overview
- **Priority:** P2 · **Status:** pending
- Build the host-side capability to run a glibc guest: larger contiguous guest-RAM carve, per-VM writable virtio-blk image backing, root-on-blk boot path, and guest RTC/RNG/network. Prove it by booting a **minimal glibc rootfs to a shell** (T1). Full Ubuntu/systemd/apt is P02b.

## Key Insights
- **Contiguous RAM carve is a hard question (red-team M9).** A glibc/Ubuntu guest needs 512 MiB-1 GB. `allocate_guest_ram` (`frame.rs:339`) must be verified to obtain a run that large after boot fragmentation — OR this phase specs a **non-contiguous guest-RAM mapping** (map N scattered physical runs into a contiguous IPA range; the multi-region idea overlaps the P05 guard rework — coordinate). Do this analysis FIRST; it gates the RAM bump.
- **Backing isolation is the security boundary** (M1/A2): virtio-blk is sector-addressed (`virtio_blk.rs:76`). Backing MUST be a per-VM image file/partition, NEVER shared `PART_CELLSTORE` (writing another cell's ELF/FAT = disk-escape). Clamp sector→offset to the real backing size.
- P05→P04 is a hard edge: systemd/glibc needs writable root-on-blk, not initramfs→sh.

## Requirements
- **Functional:** minimal glibc rootfs boots to shell; virtio-blk writable + persists; Alpine lane still green.
- **Non-functional:** no shared-store backing; sector clamp enforced; RAM carve strategy (contiguous vs scattered-IPA) decided and documented; writable-cap in cell manifest (NOT libs/api).

## Architecture
`make-hypervisor-fs.sh` gains a minimal-glibc rootfs path; `loader_image.rs` gains root-on-blk GPA layout (`root=/dev/vda`); `virtio_blk.rs` backing switches volatile-Vec → per-VM image file via VFS with sector→offset bound. RAM carve either a verified large contiguous run or a scattered-physical→contiguous-IPA mapping.

## Related Code Files
- **Modify:** `scripts/make-hypervisor-fs.sh` (minimal glibc rootfs), `loader_image.rs` (root-on-blk layout), `virtio_blk.rs` (image-file backing + sector clamp), `main.rs` (RAM bump), `dtb.rs` (bootargs `root=`), guest RTC/RNG/net wiring.
- **Verify/possibly modify:** `kernel/src/memory/frame.rs` (`allocate_guest_ram` large-contiguous feasibility).
- **Modify:** hypervisor cell manifest (write-cap).

## Implementation Steps
1. **RAM analysis (gate):** can `allocate_guest_ram` get 512 MiB-1 GB contiguous post-boot? If not, spec scattered-physical→contiguous-IPA mapping.
2. Minimal glibc rootfs in `make-hypervisor-fs.sh` (keep Alpine).
3. `loader_image.rs` root-on-blk layout + cmdline.
4. `virtio_blk.rs` per-VM image-file backing + sector clamp (NOT cell-store).
5. RAM bump + guest RTC/RNG/network.

## Todo
- [ ] RAM-carve feasibility analysis (contiguous vs scattered-IPA) — GATE
- [ ] minimal glibc rootfs (keep Alpine)
- [ ] root-on-blk loader layout + cmdline
- [ ] per-VM image-file backing + sector clamp (NOT cell-store)
- [ ] RAM bump + guest RTC/RNG/network

## Success Criteria
- Build + boot + run (ARM64 TCG): minimal glibc guest reaches shell; virtio-blk write persists across guest reboot; Alpine lane unaffected; RAM-carve strategy documented.

## Risk Assessment
- **High:** backing points at shared cell-store → disk escape. Mitigation: per-VM image only + sector clamp; reject any path opening `PART_CELLSTORE` as backing.
- **High:** large contiguous carve unobtainable post-fragmentation → boot fails. Mitigation: the step-1 analysis; scattered-IPA fallback.

## Security Considerations
- Backing-store isolation invariant: guest→host-disk escape independent of RAM bounds-check; sector clamp mandatory.
- Law 2: cell copies guest buffers to `Box<[u8]>` before `.await` IPC to VFS.

## Next Steps
- P02b builds the full Ubuntu+systemd+apt image on this substrate. P05 CoW pairs with a *minimal* guest (small dirty set) — the full Ubuntu guest is a poor CoW candidate (P04 records the pairing).
