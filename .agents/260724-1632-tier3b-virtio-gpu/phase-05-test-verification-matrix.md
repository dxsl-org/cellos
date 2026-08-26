# Phase 05 — Test & Verification Matrix

## Context Links
- Plan: [plan.md](plan.md) · Validates phases 00-04. Runs on the **hv-arm-gui image (phase 00)**;
  depends on 00, 03, AND 04 (the cursor test T9 is in this completion gate).
- Existing Tier3b boot path: `cells/services/hypervisor/src/main.rs:126-255` (Alpine boot), `dtb.rs:21` (BOOTARGS).
- Functional-verification rule: CLAUDE.md → "build + boot + run before claiming complete".
- CI QEMU-TCG guest-boot limit (project memory): nested-guest under TCG is slow/limited.
- FB-capture oracle (T5) candidate: the phase-00 FB-capture front-end, if landed.

## Overview
- **Priority:** P1 (gates "done" for Track A)
- **Status:** partial — automated lane wired; strict guest tokens require ARM64 KVM/real hardware
- **Description:** Define and run the layered verification matrix on ARM64 QEMU with an Alpine guest
  carrying the `virtio-gpu` DRM module. Classify each test as CI-gateable vs real-HW/interactive so
  the roadmap does not over-claim what CI proves.

## Key Insights
- Guest needs `CONFIG_DRM_VIRTIO_GPU` (Alpine ships it as a module; ensure it is in the initramfs
  or built-in). BOOTARGS currently sets `rdinit=/bin/sh` (dtb.rs:22) — extend with a test script.
- The compositor output is observable via QEMU SDL/VNC or a captured framebuffer; there is no
  screendump oracle in CI historically (project memory: serial-probe only), so pixel-appearance
  tests are interactive/real-HW, while ring-liveness + device-bind tests are serial-observable →
  CI-gateable.
- Guest-side 3D uses Mesa **llvmpipe** (software) into the dumb buffer — no host support needed
  (to-verify-at-impl); include one llvmpipe smoke test to prove "graphical apps" work end to end.

## Requirements — Test Matrix
| # | Test | Level | Oracle | CI-gateable? |
|---|------|-------|--------|--------------|
| T1 | virtio-gpu driver binds, `/dev/dri/card0` exists | integration | guest `dmesg`/serial | **Yes** (serial) |
| T2 | controlq: CREATE_2D (format 1 AND 2)→ATTACH→TRANSFER→SET_SCANOUT all OK, no ring stall | integration | serial (test script prints OK) | **Yes** |
| T3 | VMM cell survives (no OOM-kill) through resource lifecycle | integration | host serial (no "[hv]" abort) | **Yes** |
| T4 | fbcon: text console pixels appear in compositor | e2e | visual (SDL/VNC/FB capture) | No (interactive) |
| T5 | Known RGB test pattern (guest DRM dumb write) matches in compositor FB | e2e | FB byte compare | Partial (needs FB capture harness) |
| T6 | Xorg (modesetting/fbdev SW) starts + renders | e2e | visual | No (interactive) |
| T7 | Wayland (SW, e.g. weston-headless→fb) renders | e2e | visual | No |
| T8 | Mesa llvmpipe GL app (e.g. `glxgears`/`eglinfo`) draws into dumb buffer, visible | e2e | visual | No |
| T9 | Cursor visible + tracks (phase 04) | e2e | visual | No |
| T10 | Grant + surface released on guest shutdown (no compositor fault) | integration | compositor serial | **Yes** |
| T11 | Hostile-input unit: bad resource_id / oversized rect / huge nr_entries / `CREATE_2D(id=0xFFFFFFFF)` / format 3-4 / cursor `pos-hotspot` underflow → error resp, no VMM crash/OOM | unit/integration | serial + no abort | **Yes** |
| T12 | XRGB8888 guest (DRM default → **format 2**) creates + scans out successfully | integration | serial (test script prints OK) | **Yes** |

## Architecture
CI lane runs T1-T3, T10-T12 via serial `wait_for` probes (respect the whole-buffer `wait_for`
footgun — use `cmd && echo TOKEN$?` → wait for `TOKEN0`, per project memory). Interactive lane
(T4-T9) runs on the hv-arm-gui image (phase 00) and is documented as a manual/real-HW checklist with
SDL/VNC screenshots attached to the PR. The real gate for T1 is `CONFIG_DRM_VIRTIO_GPU` present in
the Alpine initramfs (this is the wire-protocol consumer; there is no C-struct ABI to match, m6).

## Related Code Files
- **Modify:** guest test rootfs / init script (Alpine initramfs build), `dtb.rs:22` BOOTARGS if a
  dedicated test rdinit is used.
- **Create:** a host-side FB-capture probe for T5 if feasible (optional; else T5 is interactive).
- **Reference:** existing hv test harness / run scripts for ARM64.

## Implementation Steps
1. Build/confirm an Alpine initramfs with `virtio_gpu` + `modetest`/`libdrm-tests` + a small DRM
   dumb-buffer fill program that prints a serial token on success.
2. Add CI probes for T1-T3, T10-T12 to the ARM64 hv suite; gate on the serial tokens.
3. Document the interactive checklist (T4-T9) with expected screenshots.
4. Run the full matrix; only mark Track A phases complete when T1-T3/T10-T12 pass in CI and the
   interactive set is verified once by hand (functional-verification rule).

## Todo List
- [x] Alpine test initramfs (virtio_gpu module + static framebuffer probe + serial tokens)
- [x] Dedicated probes T1-T3, T10, T11, T12 wired to serial tokens
- [ ] Interactive checklist T4-T9 documented with screenshots (on hv-arm-gui image)
- [ ] llvmpipe GL smoke (T8) verified once
- [ ] Full matrix pass recorded in plan

## Verification Evidence — 2026-07-25

- Host contract tests: 8 passed (`virtio-gpu-contract`, `virtqueue-guard`,
  `virtio-gpu-resource-rules`).
- AArch64 checks: `vicell-kernel`, `service-hypervisor`, `service-compositor`, `api`, and `ostd`
  compile cleanly with PIC/PAC and `qemu-virt-1g`.
- Image lane: pinned Alpine 3.21.3 kernel/initramfs/modloop verified by SHA-256; the 72 MiB VIFS1
  contains all core cells, raw ARM64 Image, GPU-test initramfs, and the static guest probe.
- QEMU-TCG reaches physical GPU Driver Cell registration, compositor startup, VM creation, image
  streaming, vCPU entry, and scanout teardown. It then hits the documented nested
  stage-1-over-stage-2 translation fault before Linux can emit `TIER3B_T1_*`.
- The ignored-by-default strict lane has bounded serial accept, child-exit polling, and QEMU stderr
  capture, so it cannot hang or pass vacuously in its dedicated CI job.
- Therefore T1/T2/T12 and interactive T4-T9 remain unverified locally; run
  `TIER3B_GPU_E2E=1 cargo test --manifest-path tests/integration/Cargo.toml
  --target x86_64-pc-windows-msvc --test tier3b-virtio-gpu -- --ignored --nocapture` on ARM64
  KVM/real hardware before changing this phase to complete.

## Success Criteria
- CI-gateable tests (T1-T3, T10, T11, T12) green in the ARM64 hv lane.
- Interactive tests (T4-T9) verified at least once with attached evidence; the guest renders a
  desktop/app and it appears in the Cellos compositor.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Nested-guest-under-TCG too slow for Xorg/Wayland in CI | H×M | Keep heavy e2e (T4-T9) OUT of CI; CI proves device correctness (T1-T3), humans prove pixels. |
| No FB-capture oracle → T5 can't be automated | M×M | Accept T5 as interactive; rely on T2 (controlq correctness) + T4 visual for CI confidence. |
| Alpine kernel lacks virtio_gpu in initramfs | M×H | Verify module presence first; rebuild initramfs including it (mirrors existing image-build flow). |

## Security Considerations
- T11 is the hostile-guest regression gate — must stay green: malformed guest commands (bad
  resource_id, oversized rect, huge nr_entries, `CREATE_2D(id=0xFFFFFFFF)`, format 3-4, cursor
  `pos-hotspot` underflow) produce virtio-gpu error responses (0x12xx) and never a VMM abort, OOM,
  or out-of-Grant write.
- T10 additionally exercises the C6 owner-death path: killing the VMM cell (not a graceful shutdown)
  must fire the compositor NotifyOnExit cleanup so no freed Grant frame is subsequently painted.

## Next Steps
On green, Track A ships. Phase 06 (x86) reuses this matrix once x86 MMIO dispatch exists.
