# MMC Subsystem — Implementation Plan

> QEMU targets continue using VirtIO block (unchanged).
> Real hardware (RPi 4 / VisionFive2) boots from SDHCI/eMMC or SD card.

## Status

| Phase | File | Status | Description |
|-------|------|--------|-------------|
| 01 | [phase-01-hal-trait.md](phase-01-hal-trait.md) | ✅ Complete | `ViMmcHost` HAL trait crate |
| 02 | [phase-02-sdhci-controller.md](phase-02-sdhci-controller.md) | ✅ Complete | SDHCI register layer + PIO controller |
| 03 | [phase-03-mmc-core.md](phase-03-mmc-core.md) | ✅ Complete | Shared card-init protocol (CMD state machine) |
| 04 | [phase-04-card-variants.md](phase-04-card-variants.md) | ✅ Complete | eMMC + SD protocol variants → `ViBlockDevice` |
| 05 | [phase-05-integration.md](phase-05-integration.md) | ✅ Complete | Probe order, block-device routing, board config |

## Key Dependencies

```
Phase 01 ──► Phase 02 ──► Phase 03 ──► Phase 04 ──► Phase 05
  (trait)     (HW layer)   (protocol)   (variants)   (wire-up)
```

Phases 01→04 are linear (each builds on prior).
Phase 05 depends on 04 complete + reads 3 existing files to update call sites.

## Scope

**In scope:**
- `hal/traits/mmc/` crate: `ViMmcHost` trait, types
- `kernel/src/task/drivers/mmc/` kernel-resident driver (not a Cell)
- SDHCI PIO mode (no DMA, no UHS tuning) — sufficient for boot I/O
- eMMC (CMD1 path) + SD card (ACMD41 path) protocol variants
- Compile-time board selection (`board-rpi4`, `board-visionfive2` features)
- `block_device()` routing fn replacing hardcoded `viVirtIOBlk` call sites

**Out of scope:**
- ADMA2/SDMA DMA mode (Phase 06 future)
- UHS-I/HS200/HS400 speed modes (requires tuning — deferred)
- eMMC RPMB (deferred until secure-storage use case)
- Resource Registry changes (kernel MMC driver bypasses it — only Driver Cells use registry)
- QEMU SDHCI emulation (VirtIO block remains primary for all QEMU targets)

## Files Touched

| File | Action |
|------|--------|
| `hal/traits/mmc/` | Create new crate |
| `hal/core/src/lib.rs` | Re-export `hal_traits_mmc` |
| `hal/Cargo.toml` (workspace) | Add mmc trait crate |
| `kernel/src/task/drivers/mmc.rs` | Create module entry |
| `kernel/src/task/drivers/mmc/` | Create directory with 4 submodules |
| `kernel/src/task/drivers/block.rs` | Create routing layer |
| `kernel/src/task/drivers.rs` | `pub mod mmc; pub mod block;` + probe call |
| `kernel/src/snapshot/mod.rs` | Replace `viVirtIOBlk` → `block::block_device()` |
| `kernel/src/loader/early.rs` | Same |
| `kernel/src/task/syscall.rs` | Same |
| `kernel/Cargo.toml` | Add `hal-traits-mmc` dep |
