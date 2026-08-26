# RPi3 input fallback fix

Status: Complete

## Phase 1 — Platform-safe input artifact

- Status: Complete
- Add a default-on `virtio-mmio` feature to `service-input`.
- Compile its slot iterator to empty when the feature is disabled.
- Extend the AArch64 cell builder with an RPi3 mode that writes to a separate embedded-artifact directory.
- Build the RPi3 kernel with `EMBEDDED_OVERRIDE`, preserving the ordinary QEMU AArch64 image.
- Details: [phase-01-rpi3-input-fallback.md](phase-01-rpi3-input-fallback.md)

## Phase 2 — Verification

- Status: Complete
- Build both default QEMU and no-VirtIO RPi3 input variants.
- Run format, static build guards, kernel build, tester, and reviewer.
- Publish a verified uImage and reproduce the original real-board boot.

## Constraints

- No public syscall or manifest ABI change.
- Do not map QEMU `0x0A000000` on BCM2837.
- Preserve existing UART/kernel-push fallback and QEMU direct VirtIO ownership.
- Do not modify or overwrite the generic AArch64 embedded image in RPi3 mode.
