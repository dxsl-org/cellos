# Phase 01 — ViMmcHost HAL Trait

## Overview
| | |
|---|---|
| **Priority** | P0 — foundation for all later phases |
| **Status** | ✅ Complete |
| **LOC estimate** | ~80 LOC |

New HAL trait crate for the SDHCI host controller abstraction. Lives in `hal/traits/mmc/`
following the exact same pattern as `hal/traits/gpio/` and `hal/traits/uart/`.

This is **not** a Law 1 scope change — `libs/api/src/block.rs` (`ViBlockDevice`) is
unchanged. `ViMmcHost` is a lower-level hardware interface consumed only by `MmcCore`
inside the kernel driver.

## Key Insights

- Existing pattern: `hal/traits/gpio/src/lib.rs` — `#![no_std]`, uses `types::ViResult`,
  defines a single trait with enums. Replicate exactly.
- `ViMmcHost` needs only: send_cmd, read_data (PIO), write_data (PIO), set_clock, set_bus_width.
  No DMA, no IRQ abstractions at this level.
- Law 6: `ViMmcHost`, `MmcCmd`, `MmcResponse` use `Vi` prefix for the trait.
  Support types (`BusWidth`, `CardType`) may omit prefix (they are enums, not traits/ZSTs).

## Files to Create

```
hal/traits/mmc/
├── Cargo.toml
└── src/
    └── lib.rs    (~80 LOC)
```

## Cargo.toml

```toml
[package]
name = "hal-traits-mmc"
version = "0.1.0"
edition = "2021"

[dependencies]
types = { path = "../../../libs/types" }

[profile.dev]
panic = "abort"
[profile.release]
panic = "abort"
```

## lib.rs Contents

Types to define:
```rust
#[no_std]

// MmcCmd — encodes command index + argument + response type
pub struct MmcCmd { pub index: u8, pub arg: u32, pub resp_type: RespType }
pub enum RespType { None, R1, R1b, R2, R3, R6, R7 }
pub type MmcResponse = [u32; 4];  // 128-bit response, R2 uses all 4 words

pub enum BusWidth { One, Four, Eight }
pub enum CardType { Emmc, SdSc, SdHc }

/// SDHCI host controller interface (PIO mode).
///
/// Implementors: `SdhciController` (kernel). All methods are synchronous (polling).
/// Async DMA support deferred — see Phase 06 notes.
pub trait ViMmcHost {
    fn send_cmd(&mut self, cmd: MmcCmd) -> ViResult<MmcResponse>;
    fn read_block(&mut self, buf: &mut [u8]) -> ViResult<()>;   // reads exactly buf.len() bytes
    fn write_block(&mut self, buf: &[u8]) -> ViResult<()>;
    fn set_clock_hz(&mut self, hz: u32) -> ViResult<()>;
    fn set_bus_width(&mut self, width: BusWidth) -> ViResult<()>;
    fn card_present(&self) -> bool;
}
```

## Implementation Steps

1. Create `hal/traits/mmc/Cargo.toml` (content above)
2. Create `hal/traits/mmc/src/lib.rs` with types + `ViMmcHost` trait
3. Add `hal-traits-mmc` to the workspace `hal/Cargo.toml` members list
4. Add re-export in `hal/core/src/lib.rs`: `pub use hal_traits_mmc::*;`

## Success Criteria

- [x] `cargo check -p hal-traits-mmc` passes with zero warnings
- [x] Trait is accessible as `hal_core::ViMmcHost`

## Evidence

**Verification command:**
```bash
cargo check -p hal-traits-mmc --target riscv64gc-unknown-none-elf
cargo check -p hal-core --target riscv64gc-unknown-none-elf
```

**Result:** PASS — Trait crate compiles cleanly, re-exported via `hal::core`.

**Files created:**
- `hal/traits/mmc/Cargo.toml` (34 lines)
- `hal/traits/mmc/src/lib.rs` (77 lines) — `ViMmcHost` trait with `MmcCmd`, `RespType`, `BusWidth`, `CardType` enums

**Changes:**
- Added `hal-traits-mmc` to `hal/Cargo.toml` workspace members
- Added `pub use hal_traits_mmc::{ViMmcHost, MmcCmd, RespType, BusWidth, CardType};` to `hal/core/src/lib.rs`

## Risk

Low. Pure trait definition, no hardware interaction.
