# Phase 01 — RPi3 input fallback

## Context Links

- `.agents/debug/debug-260817-0805-rpi3-input-virtio-probe.md`
- `cells/services/input/src/virtio_device.rs`
- `scripts/build-aarch64-cells.ps1`
- `kernel/build.rs` (`EMBEDDED_OVERRIDE`)

## Overview

- Priority: High
- Status: Complete
- Description: keep `/bin/input` alive on RPi3 by disabling only its direct QEMU VirtIO probe.

## Key Insights

- The real board reproducibly faults on the first volatile read at `0x0A000000`.
- The board kernel already exposes zero VirtIO slots and does not map that QEMU window.
- `service-input` is a separate Cargo build and does not inherit `board-rpi3`.
- The service already supports kernel-pushed UART/input events when it owns no VirtIO device.

## Requirements

- Default AArch64/RISC-V builds retain direct VirtIO probing.
- RPi3 build produces an empty slot iterator and logs the existing kernel-push fallback marker.
- RPi3 artifacts live outside `kernel/src/embedded-aarch64`.
- Existing policy and cell layout remain intact.

## Architecture

`service-input --no-default-features` → RPi3-only cell target directory → RPi3-only `kernel_fs.img` → `EMBEDDED_OVERRIDE` → `vicell-kernel --features board-rpi3` → raw image → TFTP uImage.

## Related Code Files

- Modify `cells/services/input/Cargo.toml`
- Modify `cells/services/input/src/virtio_device.rs`
- Modify `scripts/build-aarch64-cells.ps1`
- Modify `tools/rpi3-netboot/test-netboot-scripts.ps1`

## Implementation Steps

1. Add default-on `virtio-mmio` Cargo feature.
2. Gate architecture slot iterators on that feature; use `empty()` otherwise.
3. Add `-BoardRpi3` builder mode and separate target/embedded paths.
4. Add guards proving RPi3 uses `--no-default-features` and `EMBEDDED_OVERRIDE`.
5. Build both service variants and the RPi3 kernel.
6. Wrap/deploy, then validate on the real board.

## Todo List

- [x] Feature gate implemented
- [x] Artifact separation implemented
- [x] Regression guards pass
- [x] Default and RPi3 builds pass
- [x] Reviewer passes
- [x] Real board no longer faults at `0x0A000000`

## Success Criteria

- No `/bin/input` `EC=0x24 FAR=0x0A000000` on RPi3.
- UART/kernel-push fallback remains live.
- QEMU default build retains VirtIO slot scanning.

## Risk Assessment

- Shared artifact contamination: prevented with separate target and embedded directories.
- QEMU input regression: verify default feature build separately.
- Stale embedded cells: inspect FAT layout and rebuild kernel after artifact generation.

## Security Considerations

- Never map or read unregistered MMIO on RPi3.
- No new capability, syscall, or allowlist entry.

## Next Steps

Hardware reproduction passed over static TFTP: `/bin/input` registered, selected
the kernel-push fallback, and reached init supervision without the original
`EC=0x24 FAR=0x0A000000` abort or a service restart.
