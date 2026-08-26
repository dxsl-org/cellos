---
title: "SPI peripheral — Mode 0-3 + software loopback (enhance shipped bit-bang SPI)"
description: "SPI is already shipped (Mode-0, TX-only-verified). This plan closes the two real gaps: configurable Mode 0-3 and a software MOSI→MISO loopback so the full-duplex RX path is finally test-covered."
status: pending
priority: P2
effort: 3 phases (~230 LOC net new)
branch: main
tags: [g1, peripherals, spi, driver-cell, hal, qemu-arm-virt, loopback]
created: 2026-07-12
---

# SPI Peripheral — Mode 0-3 + Software Loopback

> ⚠️ **PREMISE CORRECTION (verified against codebase 2026-07-12).** The brief framed SPI as
> "the last missing piece of the G1 peripheral set" that "was descoped." **This is stale.**
> SPI bit-bang shipped **2026-06-14** (plan `.agents/260613-2132-c-i2c-spi/` = `status: complete`)
> and is fully wired. G1 peripherals GPIO/UART/I2C/**SPI**/PWM/ADC/CAN are ALL present. This plan
> does **not** rebuild SPI — it implements the genuine remaining work that maps onto the brief's
> own scope bullets (modes 0-3, software loopback, unit-testable protocol layer).

## What ALREADY EXISTS (do NOT rebuild — verified)

| Component | Reality (file:line) | State |
|---|---|---|
| `ViSpi` HAL trait | `hal/traits/spi/src/lib.rs:24` — `cs_select`/`cs_deselect`/`transfer`/`write`, `type Error` | ✅ **Mode 0 only** (doc line 12) |
| `BitBangSpi<G: ViGpio>` driver | `cells/drivers/spi-gpio/src/lib.rs:40` — full bit-bang, `#![forbid(unsafe_code)]`, pins MOSI=2/MISO=3/SCK=4/CS=5, `into_gpio()` cycle helper (:54) | ✅ **Mode 0 hardcoded** (`xfer_byte` :111) |
| `spi-demo` cell | `cells/demos/spi-demo/src/main.rs` — `gpio=true` manifest cap (:17), TX write + full-duplex transfer | ✅ ships; RX = 0x00 on QEMU |
| Integration test | `tests/integration/tests/periph-i2c-spi.rs:72` — asserts `[spi-demo] SPI TX OK` | ⚠️ **passes only by skip** (see gap 3) |
| Workspace + disk embed | `Cargo.toml:39,59,109`; `scripts/format-disk-arm.ps1:38,93` | ✅ wired |
| Cap gating | `libs/api/src/abi/manifest.rs:33` `MANIFEST_FLAG_GPIO` reused (bit-bang SPI *is* GPIO) | ✅ **no Law 1 needed** |

## Genuine gaps (this plan — maps to brief scope)

1. **Mode 0 only.** Trait doc + `xfer_byte` (`spi-gpio/src/lib.rs:111`) hardcode CPOL=0/CPHA=0. Brief asks **modes 0-3**. → Phase 01.
2. **No software loopback → RX path has ZERO test coverage.** QEMU PL061 has no MOSI→MISO wire; `transfer()` always reads `0x00` (`spi-gpio/src/lib.rs:110`, demo :55). The full-duplex/RX half of `ViSpi` is unverified. Brief asks for a "loopback impl (MOSI→MISO echo in software)" — the exact CAN/ADC sim pattern (`LoopbackCan`, `cells/drivers/can-loopback/src/lib.rs`). → Phase 02.
3. **Broken test wiring.** `init` never spawns `spi-demo` — it is listed as an on-demand shell demo only (`cells/tools/init/src/main.rs:216` is a comment; `grep` confirms no `sys_spawn_from_path("/bin/spi-demo")`). But `periph-i2c-spi.rs:77` assumes init auto-spawns it. The test therefore only "passes" when skipped (disk/QEMU absent). → Phase 03.

## Trait shape decision

**Extend the EXISTING `ViSpi` trait — do NOT create a new one, do NOT add a trait method.**
Add a `SpiMode` enum to `hal_spi` and thread it through the **driver constructor** (`BitBangSpi::new_with_mode`), keeping `new()` = Mode 0 for backward compat. The trait signature is unchanged (still object-safe, existing callers compile untouched). Mode is a *device configuration*, not a per-call parameter — mirrors `ViCan::configure(kbps)` taking bus config rather than widening `send_frame`. A trait-method (`set_mode`) was rejected: it forces every impl to change and adds a failure mode (call-order: mode-after-transfer) for no benefit (KISS/YAGNI). `hal_spi` lives in `hal/traits/` → **not `libs/api` → no Law 1 change** for the enum.

## Phases

| # | Phase | Status | Effort | Blocks | Test oracle |
|---|-------|--------|--------|--------|-------------|
| 01 | [SpiMode 0-3 in hal_spi + BitBangSpi honors CPOL/CPHA](phase-01-spimode-0-3.md) | pending | S (~70 LOC) | — | QEMU: `spi-demo` Mode-3 write still prints `SPI TX OK`; (bonus) host unit test of mode bit-math if host-buildable |
| 02 | [LoopbackSpi software echo + RX round-trip coverage](phase-02-loopback-spi.md) | pending | M (~110 LOC) | 01 | QEMU: `spi-demo` prints `[spi-demo] SPI loopback RX OK` (rx==tx); first real RX-path assertion |
| 03 | [Fix spi-demo test wiring + docs + real-board note](phase-03-wiring-docs.md) | pending | S (~50 LOC) | 02 | QEMU: `periph-i2c-spi.rs` TX **and** loopback probes both asserted and actually reachable (not skip-passing) |

**Critical path:** 01 → 02 → 03 (sequential; 02 exercises 01's mode config, 03 asserts 02's probe).

## Non-goals / deferred (explicit)

- **SPI slave mode** — out of scope (brief: master only).
- **Hardware SPI controller (PL022)** — QEMU virt has no SPI block; real-SBC follow-up.
- **Real sensor validation (MCP3008 ADC / BME280)** — needs physical SPI slave; **deferred to real-board task**. QEMU-verifiable surface = loopback echo + TX MMIO toggling only.
- **Dedicated `MANIFEST_FLAG_SPI`** — manifest flags byte is full (`u8`); a new flag is a Law 1 `u8→u16` bump. Not needed — keep `gpio` gating (bit-bang SPI is GPIO). Revisit only with hardware PL022.
- **Hypha P4 tool-peripheral SPI verb** — note the hook only: a future Hypha tool-cell could expose an `spi.transfer` verb over the existing `ViSpi` rlib. Do NOT build (no consumer yet — YAGNI). Recorded so P4 knows the seam exists.

## Law compliance

- **Law 1:** not triggered — `SpiMode` in `hal/traits/spi`, not `libs/api`; cap gating reuses `MANIFEST_FLAG_GPIO`. No ABI change.
- **Law 4:** `spi-loopback` + `spi-gpio` cells `#![forbid(unsafe_code)]`; `spi-demo` uses `#[no_mangle] main` so cannot `forbid` but stays unsafe-free by construction (matches existing demos).
- **Law 5:** `lib.rs`/`main.rs` only; new dir `cells/drivers/spi-loopback/src/lib.rs`.
- **Law 6:** `ViSpi` (Vi prefix); `SpiMode`, `LoopbackSpi` types.
- **Law 8:** `LoopbackSpi` is pure in-memory (no resource) — no Drop needed; `BitBangSpi::into_gpio()` returns the GPIO for move-cycling (pre-existing MMIO-release debt inherited, not regressed).

## Top risk

**R1 (High) — host unit testing may be blocked by the bare-metal default target.** No peripheral crate has any `#[cfg(test)]`; default target is `riscv64gc-unknown-none-elf` (`.cargo/config.toml:2`), and memory records "ostd not host-testable." → **Mitigation:** make the **QEMU loopback round-trip the primary oracle** (proven path); host unit tests are a *bonus* gated on `cfg_attr(not(test), no_std)` compiling for the host target — if they don't build, the phase still passes on the QEMU probe. Do not let host-test aspirations block delivery.

See per-phase files for full risk/rollback/data-flow. Open questions at end of each phase.
</content>
</invoke>
