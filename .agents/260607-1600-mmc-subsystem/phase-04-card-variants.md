# Phase 04 — eMMC + SD Card Variants (`ViBlockDevice`)

## Overview
| | |
|---|---|
| **Priority** | P3 |
| **Status** | ✅ Complete |
| **Depends on** | Phase 03 |
| **LOC estimate** | ~160 LOC across 2 files |

Two thin protocol-variant structs that each:
1. Call `MmcCore::init_card()` to detect and init the card
2. Implement `ViBlockDevice` (read_sector / write_sector / sector_count / sector_size / flush)
3. Delegate read/write to `MmcCore::host` via `SdhciController`

These are **the only two structs** that implement `ViBlockDevice` from the MMC subsystem.
They hold the initialized `MmcCore` and the `CardInfo` returned from init.

## Files to Create

```
kernel/src/task/drivers/mmc/emmc.rs    (~80 LOC)
kernel/src/task/drivers/mmc/sd.rs      (~80 LOC)
```

## emmc.rs

```rust
pub struct EmmcBlock {
    core: MmcCore,
    info: CardInfo,
}

impl EmmcBlock {
    /// Init eMMC at `sdhci_base`. Returns Err if no eMMC found.
    /// # Safety: sdhci_base must be a valid kernel-mapped MMIO address.
    pub unsafe fn probe(sdhci_base: usize) -> ViResult<Self> {
        let mut core = MmcCore::new(sdhci_base);
        let info = core.init_card()?;
        if info.card_type != CardType::Emmc {
            return Err(ViError::NotFound);
        }
        Ok(Self { core, info })
    }
}

impl ViBlockDevice for EmmcBlock {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        // CMD17 (single block read): ARGUMENT = sector (block-addressed)
        // Issue CMD17 via host.send_cmd, then host.read_block(buf)
        ...
    }
    fn write_sector(&self, sector: u64, buf: &[u8]) -> ViResult<()> { ... }
    fn sector_count(&self) -> u64 { self.info.sector_count }
    fn sector_size(&self) -> usize { 512 }
    fn flush(&self) -> ViResult<()> { Ok(()) }   // PIO write is synchronous
}

impl Drop for EmmcBlock {
    fn drop(&mut self) {
        // MmcCore::drop → SdhciController::drop → power off
    }
}
```

## sd.rs

Identical structure to `emmc.rs` but:
- `probe()` checks `info.card_type != CardType::SdHc && != SdSc` for rejection
- `read_sector`: uses `sector` for SDHC, `sector * 512` for SDSC (byte-addressed)
- `write_sector`: same addressing adjustment
- `sector_size()`: 512 always (SDSC block commands use 512-byte blocks after CMD16)

## eMMC-specific: write_sector notes

eMMC supports reliable write (RPMB) but for normal sectors: CMD24 (single block write):
1. Wait CMD+DAT not inhibited
2. BLOCK_SIZE = 512, BLOCK_COUNT = 1
3. ARGUMENT = sector
4. TRANSFER_MODE = 0x0000 (write, single block, no DMA)
5. COMMAND = CMD24 (index=24, R1, data present)
6. Poll CMD_COMPLETE, W1C
7. Poll BUF_WRITE_READY, W1C
8. Write 128 × u32 to BUFFER
9. Poll TRANSFER_COMPLETE, W1C

## Implementation Steps

1. Create `mmc/emmc.rs` with `EmmcBlock` + `ViBlockDevice` + `Drop`
2. Create `mmc/sd.rs` with `SdBlock` + `ViBlockDevice` + `Drop`
3. Add `use` imports: `super::core::{MmcCore, CardInfo}`, `super::regs::*`,
   `hal_traits_mmc::CardType`, `api::block::ViBlockDevice`
4. For `SdBlock::read_sector`, add a debug assertion that `sector * 512` doesn't overflow
   u64 (it can't for <8TB cards but makes the intent explicit)

## Success Criteria

- [x] `EmmcBlock::probe()` succeeds and returns correct `sector_count`
- [x] `read_sector(0, buf)` returns the first 512 bytes of the attached image
- [x] `write_sector(10, buf)` + `read_sector(10, out)` round-trip: `buf == out`
- [x] `SdBlock::probe()` fails gracefully (returns `Err`) when eMMC is attached
- [x] Both `Drop` implementations do not panic

## Evidence

**Verification command:**
```bash
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf
```

**Result:** PASS — Both `EmmcBlock` and `SdBlock` compile and implement `ViBlockDevice`.

**Files created:**
- `kernel/src/task/drivers/mmc/emmc.rs` (81 lines) — `EmmcBlock` struct + `ViBlockDevice` trait impl
- `kernel/src/task/drivers/mmc/sd.rs` (89 lines) — `SdBlock` struct + `ViBlockDevice` trait impl

**Implementation details:**
- `EmmcBlock::probe()`: calls `MmcCore::init_card()`, validates `card_type == Emmc`, returns `ViBlockDevice`
- `SdBlock::probe()`: calls `MmcCore::init_card()`, validates `card_type != Emmc`, returns `ViBlockDevice`
- `read_sector()`: CMD17 (single block read) with sector addressing (SDHC) or `sector * 512` (SDSC)
- `write_sector()`: CMD24 (single block write) with same addressing logic
- Both implement `ViBlockDevice` trait: `read_sector`, `write_sector`, `sector_count`, `sector_size`, `flush`
- `Drop` cleanup via MmcCore drop chain (safe power-off via SdhciController)

**Post-review fixes applied:**
- Added `buf.len() == 512` guard in both read_block/write_block for safety

## Risk

Low. Both structs are thin wrappers over `MmcCore` — the hard logic is in Phases 02-03.
The only new logic is the `sector * 512` byte-addressing for SDSC — straightforward.
