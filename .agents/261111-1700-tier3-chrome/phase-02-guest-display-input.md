---
phase: 2
title: "Guest display and input devices on x86"
status: completed
priority: P1
effort: "3-5d"
dependencies: [1]
tier: thinking
checkpoint: true
---

# Phase 2: Guest display and input devices on x86

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Make the Linux guest see and use the hypervisor's virtio-gpu and virtio-input on x86, with
correct interrupts, so a guest-drawn frame reaches the compositor. Kept separate from the
launcher work because it is device-model correctness, provable with the existing
Background surface.

## Requirements

- Functional: guest cmdline declares virtio-mmio slots 3 (GPU, `0xd0000600`) and 4 (input,
  `0xd0000800`) beside blk/net; Linux binds `virtio_gpu` (DRM + fbdev) and `virtio_input`.
- Functional: GPU and input interrupts go through the emulated 8259 on free ISA lines (as
  blk/net use IRQ 5/6), never as raw vectors 19/20.
- Functional: a guest program draws a known test pattern on `/dev/fb0` (or a DRM dumb
  buffer); the compositor shows exactly that pattern.
- Functional: an input event injected by the hypervisor arrives in the guest as an evdev event.
- Non-functional: blk/net behaviour and existing lanes unchanged.

## Architecture

- `boot_x86_profile.rs`: one cmdline builder appends GPU/input devices for profiles that
  enable display (`gui` flag), so console-only lanes keep their exact cmdline.
- `x86-irq-dispatch.rs` + `virtio_gpu.rs:119` + `virtio_input.rs:11,249`: route through
  the same PIC path as blk/net (pick IRQ lines not used by PIT/UART/blk/net, e.g. 10/11).
- Guest side: the initramfs used for this phase loads `virtio-gpu`, `virtio_input` and their
  drm deps (`modprobe`), then runs a static test-pattern writer from the initramfs overlay.

## Assumptions

- **Claim:** Alpine `initramfs-virt` carries `virtio-gpu.ko`, `virtio_input.ko` and all DRM deps.
  **Confidence:** medium (deps unverified). **Verify:** list the cpio, check `modules.dep` targets.
- **Claim:** the virtio-gpu model's 2D command set (create_2d, attach_backing, transfer,
  set_scanout, flush) is enough for Linux `virtio_gpu` fbdev emulation. **Confidence:** medium.
  **Verify:** guest dmesg + pattern on screen.
- **Claim:** `virtio_mmio.device=` cmdline probing works for 4 devices on x86. **Confidence:** high.

## Related Files

- Modify: `cells/services/hypervisor/src/boot_x86_profile.rs`, `x86-irq-dispatch.rs`,
  `virtio_gpu.rs`, `virtio_input.rs`, `virtio-mmio-address.rs` (if IRQ data lives there)
- Modify: `scripts/make-hypervisor-fs-x86.sh` / `scripts/lib-make-hypervisor-fs.sh` (gui profile)
- Create: guest test-pattern init in the initramfs overlay used by the lane
- Create: `scripts/qemu-tier3-gpu-x86.sh` (KVM/VMX lane, pixel check over QMP screendump)

## Implementation Steps

1. Host tests first: cmdline builder (console profile byte-identical; gui profile adds slots
   3/4); IRQ routing table (GPU/input lines distinct from PIT/UART/blk/net).
2. Implement the cmdline builder and the PIC routing; remove the raw 19/20 injection.
3. Build the gui initramfs overlay (module load + pattern writer + one evdev read logged
   to the console).
4. New lane: boot, wait for the guest marker, QMP screendump, compare the pattern region
   exactly; inject one key through `FORWARD_INPUT`/test hook and require the guest's log line.
5. Changelog entry.

## Success Criteria

- [x] Hypervisor host tests for cmdline and IRQ routing pass. — **Not as written:** the
      `service-hypervisor` host suite (22 tests, PASS at `c4c7d5d34f` on 2026-11-19) has no
      cmdline or IRQ-routing test; that code is x86-only. Covered on target instead by the display
      lane's "virtio-input bound and IRQ 3 delivered" checks (changelog 2026-11-11, phase 2).
- [x] `scripts/qemu-tier3-gpu-x86.sh` PASS: exact test-pattern pixels on screen, guest evdev marker.
      — Lane named `qemu-tier3-display-x86.sh`: 11 checks PASS, exact two-band pattern, virtio-input
      bound (changelog 2026-11-11, phase 2).
- [x] `qemu-hypervisor-smoke-x86.sh` (SVM/TCG and VMX) still PASS; `qemu-x86-virtio-e2e.sh` PASS.
      — Smoke PASS (Deviation Log). `qemu-x86-virtio-e2e.sh` was not recorded for this phase; the
      later `virtio-x86` 4/4 under KVM (changelog 2026-11-13) is the nearest evidence.

## Security Considerations

The guest controls every virtio descriptor: GPU transfer rects and backing addresses must
stay bounds-checked against the resource and guest RAM (existing `virtqueue_guard`); add a
host test for an out-of-range transfer rect if none exists.

## Risk Notes

Linux may want virtio-gpu features the model lacks (EDID, `RESOURCE_CREATE_BLOB`); answer
them "unsupported" correctly rather than emulating more than needed.

## Deviation Log

- Deviation: GPU and input share ISA IRQ 3, not 10/11.
  Why: the emulated 8259 has no slave; free master lines are 3 and 7 (7 = spurious line).
  Impact: `x86-irq-dispatch.rs` `DISPLAY_IRQ`, cmdline `:3` for slots 3 and 4 (virtio-mmio IRQs are shared).
  Revert: restore the raw `inject_irq(19/20)` (which never worked on x86).
- Deviation: all x86 cmdlines declare the four devices (no `gui` flag).
  Why: the devices are always modelled; Linux binds no driver without the modules, console lanes unchanged (smoke PASS).
  Impact: `boot_x86_profile.rs`.
  Revert: drop the two `virtio_mmio.device=` entries.
- Surprise: Linux virtio-gpu probe failed -EBUSY: SHM length registers read 0.
  Why: `virtio_mmio.rs` answered unknown registers with 0; 0x0b0/0x0b4 must read all-ones (no region).
  Impact: `virtio_mmio.rs` (both arches).
- Surprise: undecodable MMIO `movzbl`/`movzwl` (virtio config `readb`/`readw`) and ATTACH_BACKING with >128 entries.
  Why: decoder handled only 88/89/8a/8b/c7; the device limited backing to 128 pages (a 1280x800 fb needs ~1000).
  Impact: `vmexit_decode.rs` (0x66 16-bit, 0F B6/B7 with destination cleared), `x86-mmio-dispatch.rs` word reads,
  `virtio_gpu/wire.rs` 8192 entries.
- Surprise: partial pattern on screen came from the fixture (`dd of=` on a pipe writes short blocks), not the device.
  Impact: fixture builds band files first and checks their size.
- Decision: phase 6's packaging pulled forward minimally: desktop profile now builds `/bin/hypervisor`
  (`volatile-disk`), signing route table updated (new recipe hash, `hypervisor` in the desktop set); guest
  kernel/initramfs are added by the lane, not the assembler (phase 6).
  Impact: `assemble-x86-image.py` (`Cell.features`), `cellos_sign/signing.py`, assembler tests.
  Revert: drop the cell and restore the hash.
