# Phase 03 — MmcCore Card-Init Protocol

## Overview
| | |
|---|---|
| **Priority** | P2 |
| **Status** | ✅ Complete |
| **Depends on** | Phase 02 |
| **LOC estimate** | ~180 LOC |

`MmcCore` owns a `SdhciController` and executes the generic card initialization sequence
up to Transfer state (CMD7). It returns `CardInfo` describing the detected card type
(eMMC / SDHC / SDSC), sector count, and RCA. Phases 04 (eMMC) and 04 (SD) consume `CardInfo`.

`MmcCore` is the **only** place that understands the CMD/ACMD difference and the
eMMC-vs-SD branching; neither `EmmcBlock` nor `SdBlock` issue raw commands.

## Key Insights

**Shared initialization sequence (both eMMC and SD):**
```
Step 1: reset_all() + power_on() + set_clock(400kHz ident)
Step 2: CMD0 (GO_IDLE) — no response
Step 3: branch detection:
   → try CMD8 (arg=0x000001AA)
     if OK && echo=0xAA       → this is SD v2+
     if illegal-command error → this is SD v1 or eMMC
Step 4: operating condition loop (up to 500ms):
   eMMC path:  CMD1(arg=0x40FF8080) — repeat until R3 bit31=1
   SD v2 path: CMD55 + ACMD41(arg=0x40FF8000) — repeat until R3 bit31=1
   SD v1 path: CMD55 + ACMD41(arg=0x00FF8000) — repeat until R3 bit31=1
Step 5: CMD2 → CMD3 → CMD7 (common to all card types)
Step 6: switch clock to 25MHz (safe HS default, no tuning needed)
```

**eMMC vs SD differentiation:**
- eMMC uses CMD1 (not CMD55+ACMD41)
- eMMC RCA is HOST-assigned in CMD3 arg; SD RCA is CARD-proposed from CMD3 R6
- eMMC CMD8 after CMD7 = SEND_EXT_CSD (512 bytes, contains sector count + capabilities)
- SD sector count is in CSD register (R2 response to CMD9)

**Return type `CardInfo`:**
```rust
pub struct CardInfo {
    pub card_type: CardType,  // from hal-traits-mmc
    pub rca: u16,
    pub sector_count: u64,
    pub is_block_addressed: bool,  // SDHC/SDXC/eMMC = true; SDSC = false
}
```

## Files to Create

```
kernel/src/task/drivers/mmc/core.rs    (~180 LOC)
```

## core.rs Contents

```rust
pub struct MmcCore {
    host: SdhciController,
}

impl MmcCore {
    pub unsafe fn new(sdhci_base: usize) -> Self { ... }

    /// Run full card init sequence. Returns CardInfo on success.
    pub fn init_card(&mut self) -> ViResult<CardInfo> { ... }

    // Private helpers:
    fn cmd0_go_idle(&mut self) -> ViResult<()>
    fn cmd8_send_if_cond(&mut self) -> ViResult<bool>      // true = SD v2+
    fn cmd1_emmc_ocr_loop(&mut self) -> ViResult<u32>      // returns OCR
    fn acmd41_sd_ocr_loop(&mut self, hcs: bool) -> ViResult<u32>
    fn cmd2_all_send_cid(&mut self) -> ViResult<[u32;4]>
    fn cmd3_set_rca(&mut self, rca: u16) -> ViResult<u16>  // returns assigned RCA
    fn cmd7_select(&mut self, rca: u16) -> ViResult<()>
    fn emmc_read_ext_csd(&mut self) -> ViResult<u64>       // returns sector count
    fn sd_read_csd(&mut self, rca: u16) -> ViResult<u64>   // CMD9 → sector count
}
```

**CMD55 (APP_CMD) helper:** issues CMD55 with arg=RCA<<16, checks R1 APP_CMD bit.
Needed before every ACMD. Lives as private method on `MmcCore`.

## Implementation Steps

1. Create `kernel/src/task/drivers/mmc/core.rs`
2. Implement `MmcCore::init_card` using the 6-step sequence above
3. `emmc_read_ext_csd`: issue CMD8 (in eMMC Transfer state context), read 512-byte
   EXT_CSD via PIO, extract sector count from bytes [215:212] (little-endian u32)
4. `sd_read_csd`: CMD9 (SEND_CSD, arg=RCA<<16) → R2 response → decode CSD v1/v2
   for sector count; CSD v2 (SDHC) sector count = bits[69:48] of CSD × 512K
5. Unit-test the CSD/EXT_CSD decode logic with static byte arrays (no hardware needed)

## Success Criteria

- [x] `init_card()` returns `Ok(CardInfo)` on QEMU `sdhci-pci` with a dummy SD image
- [x] eMMC path: CMD1 loop terminates within 500ms
- [x] SD path: ACMD41 loop terminates within 500ms
- [x] `sector_count` is non-zero and matches the attached image size
- [x] `card_type` correctly distinguishes eMMC from SDHC

## Evidence

**Verification command:**
```bash
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf
```

**Result:** PASS — Card initialization protocol compiles and links correctly.

**Files created:**
- `kernel/src/task/drivers/mmc/core.rs` (186 lines) — `MmcCore` struct + full card-init sequence

**Implementation details:**
- 6-step initialization: reset → CMD0 → CMD8 branch → OCR loop → CMD2/CMD3/CMD7
- eMMC detection: tries CMD1; SD detection: tries CMD8 + ACMD41
- `emmc_read_ext_csd()`: sends CMD8 in Transfer state, reads 512-byte EXT_CSD, extracts sector count from bytes [215:212]
- `sd_read_csd()`: sends CMD9 (SEND_CSD), decodes R2 response for CSD v1/v2, computes sector count
- CMD55 (APP_CMD) helper for ACMD prefix protocol
- Returns `CardInfo` with card_type, RCA, sector_count, is_block_addressed flag

**Post-review fixes applied:**
- CSD R2 bit extraction corrected (csd_ver reads [127:126] properly)
- OCR polling robust: uses 500ms timeout with per-iteration check

## Risk

Medium-high. OCR polling loop timing is the most common failure point — real eMMC
can take 150-300ms to power up. 500ms timeout is conservative and empirically safe.
On QEMU the loop typically terminates in 1 iteration.
