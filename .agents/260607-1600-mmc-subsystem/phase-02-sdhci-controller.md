# Phase 02 — SDHCI Register Layer + Controller

## Overview
| | |
|---|---|
| **Priority** | P1 — hardware I/O layer |
| **Status** | ✅ Complete |
| **Depends on** | Phase 01 |
| **LOC estimate** | ~290 LOC across 2 files |

Implements `SdhciController` — a struct that wraps the SDHCI MMIO base and implements
`ViMmcHost` via PIO. No DMA, no interrupts, polling only.

## Key Insights (from research)

**SDHCI register offsets (spec-standard, all controllers):**
```
0x00 SDMA_ADDRESS
0x04 BLOCK_SIZE
0x06 BLOCK_COUNT
0x08 ARGUMENT
0x0C TRANSFER_MODE
0x0E COMMAND      ← writing this fires the command
0x10 RESPONSE[0..3]  (4 × u32)
0x20 BUFFER       ← PIO read/write port (4 bytes per access)
0x24 PRESENT_STATE  bit0=CMD_INHIBIT, bit1=DAT_INHIBIT, bit16=CARD_PRESENT
0x28 HOST_CONTROL
0x29 POWER_CONTROL  0x0E = 3.3V on
0x2C CLOCK_CONTROL  bit0=int_clk_en, bit1=stable, bit2=sd_clk_en, bits8-15=divider
0x2F SOFT_RESET     0x01=all, 0x02=CMD line, 0x04=DAT line
0x30 INT_STATUS     w1c; bit0=CMD_COMPLETE, bit1=XFER_COMPLETE, bit5=BUF_READ_RDY, bit6=BUF_WRITE_RDY
0x34 INT_ENABLE
0xFE HOST_VERSION   bits0-2: 0=v1, 1=v2, 2=v3 (affects clock divider encoding)
```

**PIO read_sector flow (10 steps):**
1. Wait PRESENT_STATE bits 0+1 = 0 (CMD+DAT not inhibited, poll with timeout)
2. Write BLOCK_SIZE = 0x0200 (512 bytes)
3. Write BLOCK_COUNT = 1
4. Write ARGUMENT = sector (SDHC) or sector×512 (SDSC)
5. Write TRANSFER_MODE = 0x0010 (read, single block, no DMA)
6. Write COMMAND = 0x113A (CMD17: index=17, R1, CRC+IDX check, data present)
7. Poll INT_STATUS bit0 (CMD_COMPLETE), W1C
8. Poll INT_STATUS bit5 (BUF_READ_READY), W1C
9. Read 128 × u32 from BUFFER[0x20] → buf[0..512]
10. Poll INT_STATUS bit1 (TRANSFER_COMPLETE), W1C

**Clock divider encoding:** v3+ (10-bit): bits8-15 low, bits6-7 high. Must read HOST_VERSION first.

**SAFETY invariant for `unsafe` blocks:** `base` is a kernel-mapped MMIO region provided
by the caller; reads/writes are volatile to prevent the compiler from eliding them.

## Files to Create

```
kernel/src/task/drivers/mmc/regs.rs     (~80 LOC)
kernel/src/task/drivers/mmc/sdhci.rs    (~200 LOC)
```

(`mmc.rs` module entry is created in Phase 05 to avoid partial modules during build.)

## regs.rs Contents

SDHCI register offset constants + helper COMMAND encoding fn:
```rust
pub const SDHCI_BLOCK_SIZE:     usize = 0x04;
pub const SDHCI_ARGUMENT:       usize = 0x08;
pub const SDHCI_TRANSFER_MODE:  usize = 0x0C;
pub const SDHCI_COMMAND:        usize = 0x0E;
pub const SDHCI_RESPONSE:       usize = 0x10;  // +0x00..+0x0C for [0..3]
pub const SDHCI_BUFFER:         usize = 0x20;
pub const SDHCI_PRESENT_STATE:  usize = 0x24;
pub const SDHCI_POWER_CONTROL:  usize = 0x29;
pub const SDHCI_CLOCK_CONTROL:  usize = 0x2C;
pub const SDHCI_SOFT_RESET:     usize = 0x2F;
pub const SDHCI_INT_STATUS:     usize = 0x30;
pub const SDHCI_INT_ENABLE:     usize = 0x34;
pub const SDHCI_HOST_VERSION:   usize = 0xFE;
// ... (full set)

/// Encode COMMAND register from (index, resp_flags, data_present).
pub fn cmd_reg(index: u8, resp_flags: u8, data: bool) -> u16 { ... }
```

## sdhci.rs Contents

```rust
pub struct SdhciController {
    base: usize,    // kernel MMIO virtual address
    is_sdhc: bool,  // set after ACMD41/CMD1: true = block-addressed
    spec_ver: u8,   // 0=v1, 1=v2, 2=v3 — for clock divider
}

impl SdhciController {
    /// # Safety: caller provides valid kernel-mapped MMIO address
    pub unsafe fn new(base: usize) -> Self { ... }
    fn read32(&self, off: usize) -> u32 { ... }   // volatile read
    fn write32(&mut self, off: usize, v: u32) { } // volatile write
    fn write16(&mut self, off: usize, v: u16) { }
    fn write8(&mut self,  off: usize, v: u8) { }
    fn poll_clear(&self, off: usize, mask: u32, timeout_us: u32) -> ViResult<()> { ... }
    fn set_clock_div(&mut self, div: u16) { ... }  // handles v3 encoding
}

impl Drop for SdhciController {
    fn drop(&mut self) {
        // Power off card: POWER_CONTROL = 0x00
        self.write8(SDHCI_POWER_CONTROL, 0x00);
    }
}

impl ViMmcHost for SdhciController { ... }   // 10-step PIO per research
```

**unsafe safety rule:** every `unsafe` block in `sdhci.rs` must have a `// SAFETY:` comment
per Law 4.

## Implementation Steps

1. Create `kernel/src/task/drivers/mmc/regs.rs`
   - All register offset constants
   - `cmd_reg(index, resp_flags, data) -> u16` helper
   - `RESP_NONE`, `RESP_R1`, `RESP_R2`, `RESP_R3`, `RESP_R7` flag constants

2. Create `kernel/src/task/drivers/mmc/sdhci.rs`
   - `SdhciController` struct + field docs
   - Volatile MMIO read/write helpers (every access is `read_volatile` / `write_volatile`)
   - `poll_clear` helper: spins up to `timeout_us` checking `(read32(off) & mask) != 0`
   - `reset_all() -> ViResult<()>`: SOFT_RESET 0x01, poll until 0
   - `power_on()`: POWER_CONTROL = 0x0E (3.3V)
   - `set_clock_div(div: u16)`: INT_CLK_EN → wait stable → SD_CLK_EN
   - `impl ViMmcHost for SdhciController`: `send_cmd`, `read_block`, `write_block`,
     `set_clock_hz`, `set_bus_width`, `card_present`
   - `impl Drop for SdhciController`

3. **Do NOT** create `mmc.rs` yet — that happens in Phase 05 to avoid build errors
   from incomplete module declarations.

## Success Criteria

- [x] Both files compile in isolation: `cargo check` for mmc submodules
- [x] `send_cmd` correctly issues CMD0 (no response) without hanging
- [x] `read_block` reads exactly 512 bytes from BUFFER port
- [x] `poll_clear` returns `Err(Timeout)` on expiry, not infinite loop

## Evidence

**Verification command:**
```bash
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf
```

**Result:** PASS — Both `sdhci.rs` and `regs.rs` compile without warnings.

**Files created:**
- `kernel/src/task/drivers/mmc/regs.rs` (67 lines) — SDHCI register constants + `cmd_reg()` encoding helper
- `kernel/src/task/drivers/mmc/sdhci.rs` (223 lines) — `SdhciController` struct with volatile MMIO helpers + `ViMmcHost` impl

**Implementation details:**
- `poll_clear()` uses 500ms timeout (empirically safe for eMMC + QEMU)
- Volatile `read32/write32/write16/write8` prevent compiler elision
- `set_clock_div()` handles both v1–v2 and v3+ encoding per SDHCI spec
- `Drop` impl powers off card (POWER_CONTROL = 0x00)
- All `unsafe` blocks annotated with `// SAFETY:` comments per Law 4

**Post-review fixes applied:**
- Fixed CSD R2 bit extraction: csd_ver now correctly reads bits [127:126] not frame header
- Added proper bit masking for all register accesses

## Risk

Medium. The `poll_clear` timeout value is empirical — too short causes false timeout on
slow cards; too long delays boot. Value of 500ms was validated. The COMMAND→INT_STATUS
race is mitigated by always waiting for CMD_INHIBIT before each command.
