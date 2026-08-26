# Phase 02 — LoopbackSpi software echo + RX round-trip coverage

## Context Links
- Plan: [plan.md](plan.md) · Depends on: Phase 01 (`SpiMode`)
- Pattern to mirror exactly: `cells/drivers/can-loopback/src/lib.rs` (`LoopbackCan`, in-memory, no MMIO, no cap) and `cells/drivers/adc-sim/` (`SimAdc`).
- Trait: `hal/traits/spi/src/lib.rs` (`ViSpi`)

## Overview
- **Priority:** P2 · **Status:** pending · **Effort:** M (~110 LOC)
- Add `LoopbackSpi` — a software `ViSpi` impl that echoes MOSI→MISO — so the full-duplex RX path (untested today because QEMU MISO floats to 0) gets its first real assertion. This is the brief's "loopback impl (MOSI→MISO echo in software)."

## Key Insights
- Today `transfer()` RX is dead-tested: `spi-gpio/src/lib.rs:110` and demo comment `spi-demo/src/main.rs:48-55` both document `rx == 0x00` on QEMU. Half of `ViSpi` (the read half) has never been behaviorally verified.
- A loopback needs **no GPIO, no MMIO, no cap** — exactly like `LoopbackCan` (`can-loopback/src/lib.rs:4` "no MMIO, no capability required"). It is host-and-QEMU testable.
- Echo semantics: for `transfer(tx, rx)`, a pure loopback returns `rx[i] = tx[i]` (MOSI wired to MISO). This validates bit assembly/disassembly and (with Phase 01) per-mode ordering symmetry.

## Requirements
- **Functional:** `LoopbackSpi::new(mode)` implements `ViSpi`; `transfer(tx,rx)` fills `rx` with the echoed tx bytes (respecting the `max(tx,rx)` / pad-0x00 / discard-excess contract in the trait doc `hal/traits/spi/src/lib.rs:21-22`); `write()` succeeds and is a no-op sink; `cs_select`/`cs_deselect` track an internal asserted flag and error if misused (optional).
- **Non-functional:** `#![forbid(unsafe_code)]`; no workspace member outside `cells/drivers/spi-loopback`; no init/disk change here (demo wiring is Phase 03).

## Architecture / Data flow
```
LoopbackSpi { mode: SpiMode, cs_asserted: bool }
   transfer(tx, rx):
       len = max(tx.len, rx.len)
       for i in 0..len:
           byte = tx.get(i).copied().unwrap_or(0x00)   // pad rule
           if i < rx.len { rx[i] = byte }               // echo (MOSI→MISO)
   write(bytes): accept, drop (TX sink)
```
Data in: `tx`. Transform: identity echo into `rx` under trait length rules. Data out: `rx == tx` (bounded). No hardware, deterministic → host-unit-testable.

## Related Code Files
- **Create** `cells/drivers/spi-loopback/Cargo.toml` — `name = "driver-spi-loopback"`, deps: `hal-spi`. Mirror `can-loopback/Cargo.toml`.
- **Create** `cells/drivers/spi-loopback/src/lib.rs` — `LoopbackSpi`, `#![cfg_attr(not(test), no_std)]`, `#![forbid(unsafe_code)]`, `impl ViSpi`, `#[cfg(test)] mod tests`.
- **Modify** `cells/demos/spi-demo/src/main.rs` — after the real BitBangSpi TX, run a `LoopbackSpi` transfer and assert `rx == tx`; print `[spi-demo] SPI loopback RX OK`.
- **Modify** root `Cargo.toml` — add `"cells/drivers/spi-loopback"` member.

## Implementation Steps
1. Scaffold `spi-loopback` crate mirroring `can-loopback` (Cargo.toml + lib.rs).
2. Implement `LoopbackSpi::new(mode: SpiMode)` + `Default`; store `mode`, `cs_asserted=false`.
3. Implement `ViSpi`: `transfer` echoes per the length/pad contract; `write` is a sink; `cs_select`/`cs_deselect` toggle `cs_asserted`.
4. Add `#[cfg(test)] mod tests`: round-trip `[0xAA,0x55]` → rx equal; pad rule (tx shorter than rx → trailing 0x00); excess-tx discard; per-mode (Mode0..Mode3) round-trip identical.
5. `spi-demo`: import `driver_spi_loopback::LoopbackSpi`; run a transfer, compare, print probe.
6. Register the workspace member.

## Todo List
- [ ] `spi-loopback` crate scaffolded (Cargo.toml + lib.rs)
- [ ] `LoopbackSpi` impls `ViSpi` (echo transfer, write sink, cs flag)
- [ ] length/pad/discard rules match trait doc contract
- [ ] `#[cfg(test)]` round-trip + pad + per-mode tests
- [ ] spi-demo asserts rx==tx, prints `SPI loopback RX OK`
- [ ] root `Cargo.toml` member added; aarch64 build clean

## Success Criteria
- **Done =** `spi-demo` on QEMU ARM virt prints `[spi-demo] SPI loopback RX OK` (rx byte-for-byte equals tx) — the **first** behavioral test of the SPI RX/full-duplex path.
- **Test oracle (primary, boot-verifiable):** QEMU — observe the `SPI loopback RX OK` probe.
- **Test oracle (bonus):** `cargo test -p driver-spi-loopback` round-trip passes on host if buildable (R1 in plan).

## Risk Assessment
- **R1 (High, inherited) — host test build blocked by bare-metal target.** *Mitigation:* `cfg_attr(not(test), no_std)`; if still blocked, the QEMU demo assertion is the oracle. Do not block the phase on host `cargo test`.
- **R2 (Low) — loopback masks real edge-ordering bugs** because it re-uses byte-level echo, not GPIO edges. *Mitigation:* explicit — loopback validates the *trait contract / byte assembly*, not physical timing. Real-slave timing is the deferred real-board task (stated in plan non-goals). Document this limit in the crate doc-comment so nobody mistakes green loopback for hardware-verified SPI.

## Security Considerations
None — no MMIO, no cap, no unsafe, no new syscall. LoopbackSpi cannot touch hardware by construction.

## Rollback
New crate + one demo edit + one Cargo.toml line. Remove the member, delete the dir, revert the demo block. Phase 01 unaffected.

## Next Steps / Open Questions
- **Q:** Keep `LoopbackSpi` as a standalone driver crate (matches `LoopbackCan`) vs. a `#[cfg(test)]` helper inside `spi-gpio`? **Recommend standalone** — it's a reusable `ViSpi` impl the QEMU demo consumes at runtime (not just tests), so it must ship, exactly like `can-loopback`.
- Blocks Phase 03 (which asserts the `SPI loopback RX OK` probe in the integration test and fixes spawn wiring).
</content>
