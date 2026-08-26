# Phase 01 — SpiMode 0-3 in hal_spi + BitBangSpi honors CPOL/CPHA

## Context Links
- Plan: [plan.md](plan.md)
- Trait: `hal/traits/spi/src/lib.rs`
- Driver: `cells/drivers/spi-gpio/src/lib.rs`
- Pattern precedent: `ViCan::configure(kbps)` `hal/traits/can/src/lib.rs` (device config via param, not trait-method widening)

## Overview
- **Priority:** P2 · **Status:** pending · **Effort:** S (~70 LOC)
- Add SPI clock-polarity/phase configurability (Modes 0-3) to the shipped Mode-0-only bit-bang SPI. Backward-compatible: `new()` stays Mode 0.

## Key Insights
- Current `xfer_byte` (`spi-gpio/src/lib.rs:111-127`) hardcodes Mode 0: SCK idles low, MOSI set before rising edge, MISO sampled on rising edge, SCK returns low. This is CPOL=0/CPHA=0.
- SPI modes = (CPOL, CPHA): M0=(0,0), M1=(0,1), M2=(1,0), M3=(1,1). CPOL sets idle clock level; CPHA selects sample-on-leading (0) vs sample-on-trailing (1) edge.
- Trait uses `type Error` (associated), not `ViResult` — keep that.
- `MANIFEST_FLAG_GPIO` gating unchanged; no new cap.

## Requirements
- **Functional:** `BitBangSpi` clocks all four modes correctly (idle level per CPOL; sample edge per CPHA); MSB-first preserved. `new()` == Mode 0 (unchanged behavior).
- **Non-functional:** no trait-signature change (object safety + existing callers intact); no `libs/api` touch; cell stays `#![forbid(unsafe_code)]`.

## Architecture / Data flow
```
hal_spi::SpiMode {Mode0,Mode1,Mode2,Mode3}
   ├─ fn cpol(&self) -> bool   (idle clock high?)
   └─ fn cpha(&self) -> bool   (sample on trailing edge?)
        │
BitBangSpi { gpio, mode }               // NEW field: mode
   ├─ new(gpio)              -> mode = Mode0   (BACK-COMPAT)
   ├─ new_with_mode(gpio, m) -> mode = m       (NEW)
   ├─ setup_pins()  : SCK idle = mode.cpol()   (was hardcoded false)
   └─ xfer_byte()   : edge order branches on cpol/cpha
```
Data in: `tx: &[u8]`. Transform: per-bit GPIO toggling with mode-dependent edge/sample ordering. Data out: `rx` bytes (real slave) or 0x00 (QEMU float).

## Related Code Files
- **Modify** `hal/traits/spi/src/lib.rs` — add `SpiMode` enum + `cpol()`/`cpha()`; update trait doc (no longer "Mode 0" only).
- **Modify** `cells/drivers/spi-gpio/src/lib.rs` — add `mode` field, `new_with_mode`, mode-aware `setup_pins`/`xfer_byte`.
- **Modify** `cells/demos/spi-demo/src/main.rs` — construct with an explicit mode (demo Mode 3) to exercise the new path; keep `SPI TX OK` probe string.

## Implementation Steps
1. In `hal_spi`: add `#[derive(Copy,Clone,Debug,PartialEq,Eq)] pub enum SpiMode { Mode0, Mode1, Mode2, Mode3 }` with `pub const fn cpol(&self)->bool` and `pub const fn cpha(&self)->bool`.
2. `BitBangSpi`: add `mode: SpiMode`; `new(gpio)` sets `Mode0`; add `pub fn new_with_mode(gpio, mode)`.
3. `setup_pins`: set idle SCK = `self.mode.cpol()` (line 76 currently `false`).
4. `xfer_byte`: branch clock edges on `cpol`/`cpha`. For CPHA=0 sample on the leading edge (as today, but leading edge = rising if CPOL=0 else falling); for CPHA=1 shift on leading, sample on trailing. Keep MSB-first.
5. `spi-demo`: use `BitBangSpi::new_with_mode(gpio, SpiMode::Mode3)`; print the active mode in the banner.

## Todo List
- [ ] `SpiMode` enum + cpol/cpha helpers in hal_spi
- [ ] `mode` field + `new_with_mode` constructor (keep `new` = Mode0)
- [ ] mode-aware `setup_pins` idle level
- [ ] mode-aware `xfer_byte` edge/sample ordering
- [ ] spi-demo uses Mode 3, banner shows mode
- [ ] `cargo build` aarch64 target clean; existing `new()` callers unaffected

## Success Criteria
- **Done =** aarch64 build clean; `spi-demo` boots and prints `[spi-demo] SPI TX OK` with Mode 3 selected (proves the mode-3 edge ordering doesn't break the MMIO TX path).
- **Test oracle (primary, boot-verifiable):** QEMU ARM virt — run `spi-demo` from shell, observe `SPI TX OK` under Mode 3.
- **Test oracle (bonus):** host `cargo test -p hal-spi` for cpol/cpha truth-table IF the crate builds for the host target (see R1); not required to pass the phase.

## Risk Assessment
- **R1 (Med) — edge-ordering bug in CPHA=1 modes.** No real SPI slave on QEMU to catch it (MISO floats). *Mitigation:* Phase 02's LoopbackSpi (software slave) gives the first behavioral check of RX under each mode; unit-test the bit order in `LoopbackSpi` round-trip.
- **R2 (Low) — accidentally changing `new()` semantics** breaks existing demo/callers. *Mitigation:* `new()` explicitly pins `Mode0`; assert in review.

## Security Considerations
None new — no cap change, no MMIO range change, no unsafe. Same PL061 range already allowlisted.

## Rollback
Single-crate-pair change. Revert the two crate edits + demo edit; `new()` back-compat means no downstream churn. No disk/init changes in this phase.

## Next Steps / Open Questions
- **Q:** Does `hal-spi` build for a host target (`cargo test` without `--target`)? If `#![no_std]` blocks it, switch to `#![cfg_attr(not(test), no_std)]`. Resolve at step 1; if still blocked, drop the bonus host test (QEMU oracle stands).
- Blocks Phase 02 (LoopbackSpi consumes `SpiMode` to validate per-mode round-trip).
</content>
