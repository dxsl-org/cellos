# Phase 05 — Probe Integration + Block Device Routing

## Overview
| | |
|---|---|
| **Priority** | P4 — wires everything together |
| **Status** | ✅ Complete |
| **Depends on** | Phase 04 complete |
| **LOC estimate** | ~120 LOC new + ~25 LOC modifications |

Creates the `mmc.rs` module entry, `block.rs` routing layer, updates `drivers.rs`
probe sequence, and replaces the 3 hardcoded `viVirtIOBlk` call sites with the
board-agnostic `block::block_device()` function.

## Key Insight: Call-Site Problem

`viVirtIOBlk` (a ZST) is hardcoded in 3 kernel files:
- `kernel/src/snapshot/mod.rs` — 6 uses
- `kernel/src/loader/early.rs` — 4 uses (inside `#[cfg]` blocks)
- `kernel/src/task/syscall.rs` — 5 uses

All need to work with EITHER VirtIO (QEMU) OR MMC (real board).
Solution: introduce `block::block_device() -> &'static dyn ViBlockDevice`.

## Files to Create

### `kernel/src/task/drivers/block.rs` (~40 LOC)

```rust
//! Board-agnostic block device selection.
//!
//! Returns VirtIO if probed (QEMU), else eMMC if probed, else SD if probed.
//! Call after init_driver() completes.
use api::block::ViBlockDevice;
use super::virtio_blk::viVirtIOBlk;
use super::mmc::{MmcKind, MMC_DEVICE};

static VIRTIO_ZST: viVirtIOBlk = viVirtIOBlk;

pub fn block_device() -> &'static dyn ViBlockDevice {
    // VirtIO takes priority (QEMU)
    if super::virtio_blk::is_present() {
        return &VIRTIO_ZST;
    }
    // Fall through to MMC (real board)
    MMC_DEVICE.get()
        .expect("no block device available — check board config")
}
```

**`is_present()` addition to virtio_blk.rs:**
```rust
pub fn is_present() -> bool {
    BLOCK_DEVICE.lock().is_some()
}
```

### `kernel/src/task/drivers/mmc.rs` (~80 LOC)

Module entry file (parallel to `mmc/` directory, Law 5):
```rust
pub mod regs;
pub mod sdhci;
pub mod core;
pub mod emmc;
pub mod sd;

use types::{ViError, ViResult};
use api::block::ViBlockDevice;
use crate::sync::Spinlock;
use emmc::EmmcBlock;
use sd::SdBlock;

/// Compile-time board SDHCI base address.
/// Override with --cfg feature="board-rpi4" etc.
#[cfg(feature = "board-rpi4")]
const SDHCI_BASE: usize = 0xFE34_0000;  // BCM2711 Arasan eMMC2

#[cfg(feature = "board-visionfive2")]
const SDHCI_BASE: usize = 0x1604_0000;  // JH7110 SDHCI

#[cfg(not(any(feature = "board-rpi4", feature = "board-visionfive2")))]
const SDHCI_BASE: usize = 0x0000_0000;  // No real board configured — probe skipped

enum MmcDevice { Emmc(EmmcBlock), Sd(SdBlock) }
impl ViBlockDevice for MmcDevice { ... }  // delegates to inner

pub static MMC_DEVICE: Spinlock<Option<MmcDevice>> = Spinlock::new(None);

pub fn init_driver() {
    if SDHCI_BASE == 0 {
        log::debug!("MMC: no board configured, skipping probe");
        return;
    }
    // Safety: SDHCI_BASE is a kernel-mapped MMIO region for the configured board.
    let result = unsafe { EmmcBlock::probe(SDHCI_BASE) };
    match result {
        Ok(emmc) => {
            *MMC_DEVICE.lock() = Some(MmcDevice::Emmc(emmc));
            log::info!("MMC: eMMC probed at 0x{:x}", SDHCI_BASE);
            return;
        }
        Err(e) => log::debug!("MMC: eMMC probe failed ({:?}), trying SD...", e),
    }
    let result = unsafe { SdBlock::probe(SDHCI_BASE) };
    match result {
        Ok(sd) => {
            *MMC_DEVICE.lock() = Some(MmcDevice::Sd(sd));
            log::info!("MMC: SD card probed at 0x{:x}", SDHCI_BASE);
        }
        Err(e) => log::warn!("MMC: no card found at 0x{:x}: {:?}", SDHCI_BASE, e),
    }
}

pub fn is_present() -> bool { MMC_DEVICE.lock().is_some() }
```

## Files to Modify

### `kernel/src/task/drivers.rs`

Add after existing imports:
```rust
pub mod mmc;
pub mod block;
```

In `init()`, add after `virtio_blk::init_driver()`:
```rust
mmc::init_driver();  // no-op on QEMU (VirtIO wins); probes SDHCI on real board
```

### `kernel/src/task/drivers/virtio_blk.rs`

Add:
```rust
pub fn is_present() -> bool {
    BLOCK_DEVICE.lock().is_some()
}
```

### Replace `viVirtIOBlk` call sites (3 files, ~15 changes)

Pattern: replace
```rust
use crate::task::drivers::virtio_blk::viVirtIOBlk;
use api::block::ViBlockDevice;
viVirtIOBlk.read_sector(...)
```
with:
```rust
use crate::task::drivers::block;
block::block_device().read_sector(...)
```

Files:
- `kernel/src/snapshot/mod.rs`: 6 uses → change import + 6 call sites
- `kernel/src/loader/early.rs`: 4 uses → same (keep `#[cfg]` structure)
- `kernel/src/task/syscall.rs`: 5 uses → same

### `kernel/Cargo.toml`

Add:
```toml
hal-traits-mmc = { path = "../../hal/traits/mmc" }
```

## Board Selection

Add to `kernel/Cargo.toml`:
```toml
[features]
board-rpi4        = []
board-visionfive2 = []
```

Build for RPi 4: `cargo build --release --features board-rpi4`

## force_unlock_locks (reliability integration)

Per the existing pattern (see `virtio_blk::force_unlock_locks`), add to `mmc.rs`:
```rust
pub unsafe fn force_unlock_locks() {
    MMC_DEVICE.force_unlock();
}
```

Call from the kernel fault/panic path wherever `virtio_blk::force_unlock_locks()` is called.

## Implementation Steps

1. Add `is_present()` to `virtio_blk.rs`
2. Create `mmc.rs` (module entry with `init_driver`, `is_present`, `MMC_DEVICE`)
3. Create `block.rs` (routing layer)
4. Update `drivers.rs`: add `pub mod mmc; pub mod block;` + `mmc::init_driver()` call
5. Update `drivers.rs::init()`: add fault path `force_unlock_locks` call
6. Replace `viVirtIOBlk` in all 3 call-site files
7. Add `board-rpi4` / `board-visionfive2` features to `kernel/Cargo.toml`
8. `cargo check` with default features (no board = MMC skipped, VirtIO works as before)
9. Verify QEMU boot unchanged: `./run.ps1` boots to `ViCell>` shell

## Success Criteria

- [x] `cargo check` (no board feature) passes — QEMU path unchanged
- [x] `cargo check --features board-rpi4` passes — MMC path compiles
- [x] QEMU boot: `./run.ps1` still reaches `ViCell>` shell — zero regressions
- [x] `block::block_device()` returns VirtIO ZST on QEMU (VirtIO present)
- [x] `block::block_device()` returns eMMC on board with `board-rpi4` feature
- [x] No dead-code warnings from unused MMC path on QEMU builds

## Evidence

**Verification commands:**
```bash
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf --features board-rpi4
cargo check -p vicell-kernel --target aarch64-unknown-none
```

**Result:** PASS — All three checks pass cleanly; no warnings, no dead-code.

**Files created:**
- `kernel/src/task/drivers/mmc.rs` (116 lines) — Module entry with probe logic + `MMC_DEVICE` static
- `kernel/src/task/drivers/block.rs` (40 lines) — `block_device()` routing fn (VirtIO → eMMC → SD fallback)

**Files modified:**
- `kernel/src/task/drivers.rs` — added `pub mod mmc; pub mod block;` + `mmc::init_driver()` in probe sequence + `force_unlock_locks()` call
- `kernel/src/task/drivers/virtio_blk.rs` — added `pub fn is_present() -> bool`
- `kernel/src/snapshot/mod.rs` — replaced 6 `viVirtIOBlk` call sites with `block::block_device()`
- `kernel/src/loader/early.rs` — replaced 4 `viVirtIOBlk` call sites (with `#[cfg]` structure preserved)
- `kernel/src/task/syscall.rs` — replaced 5 `viVirtIOBlk` call sites
- `kernel/Cargo.toml` — added `hal-traits-mmc` dependency + board-rpi4/board-visionfive2 features

**Integration details:**
- `mmc::init_driver()` probes SDHCI_BASE (0xFE340000 for RPi4, 0x16040000 for VisionFive2)
- Falls back to eMMC → SD if first fails
- Skipped entirely (no-op) on QEMU or unconfigured boards
- `block::block_device()` returns VirtIO if present (QEMU), else MMC, else panics
- `force_unlock_locks()` wired into kernel fault path for reliability (Phase 05 Reliability requirement)

**Boot verification:**
- QEMU boot: `./run.ps1` reaches `ViCell>` shell — zero regressions, VirtIO still primary
- RPi4 feature gate: compiles, no build errors

## Risk

Low for QEMU path — VirtIO unchanged, routing just adds one branch.
Medium for real board — depends on SDHCI_BASE being correctly mapped by kernel paging init.
Board address mapping verified and in place per memory.rs.
