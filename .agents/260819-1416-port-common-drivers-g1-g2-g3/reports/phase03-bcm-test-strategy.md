# Test Strategy - Phase 03 RPi3 BCM BSC I2C + BCM SPI - 2026-08-19

Verdict: 6 critical paths; biggest coverage gap is hardware-controller semantics. Existing coverage proves QEMU ARM virt PL061 bit-bang I2C/SPI markers, not BCM BSC/SPI register behavior or RPi3 physical bus evidence.

## Scope

Phase 03 of `.agents/260819-1416-port-common-drivers-g1-g2-g3/phase-03-g1-robot-peripheral-drivers.md`: add RPi3-line real controller support for BCM BSC I2C and BCM SPI Driver Cells while preserving the existing `ViI2c`/`ViSpi` trait contracts and avoiding `libs/api` / `libs/types` changes.

Existing coverage respected:
- `hal/traits/i2c/src/lib.rs` already defines synchronous `write`, `read`, `write_read` and STOP-on-error contract.
- `hal/traits/spi/src/lib.rs` already defines Mode 0, MSB-first, internal CS management, and CS deassert-on-error contract.
- `tests/integration/tests/periph-i2c-spi.rs` already boots QEMU ARM virt and checks bit-bang SPI TX and I2C sensor banner. Keep it as fallback/regression coverage only.
- `hal/soc/bcm27xx/src/tests.rs` already verifies BCM2837 GPIO/AUX/SDHCI offsets and bounded grants; it has no BSC/SPI layout assertions yet.

## Critical Paths

1. Board descriptor selects real BCM BSC/SPI only for RPi-compatible board data and never promotes bit-bang fallback as real-controller evidence.
2. Driver Cell open path requests only the exact SoC MMIO window and fails closed when the controller range or device class is absent.
3. BCM BSC I2C completes bounded write/read/write_read, emits repeated START for write_read, reports address NACK/data NACK distinctly, and leaves the bus idle/STOP on every error.
4. BCM SPI completes bounded Mode 0 transfer/write, drains FIFO, deasserts CS on success and every error, and leaves TA/DONE/CS state clean for the next transaction.
5. Kernel/resource lifecycle releases MMIO ownership when the Driver Cell exits, panics, or is force-killed, so a restarted controller can re-open the same range.
6. Physical RPi3 evidence proves real bus traffic with a wired device or records explicit PASS/FAIL/BLOCKED; QEMU/compile/fake-MMIO cannot satisfy this gate.

## Minimum Acceptance Matrix

| Target | Minimum slice | Required automated evidence | Required physical evidence |
|--------|---------------|-----------------------------|----------------------------|
| RPi3 BCM BSC1 | `DriverId::I2cBcmBsc`, SoC layout for `BSC1 @ 0x3F804000`, exact MMIO grant, `ViI2c::write_read` repeated START | Register-model tests for ACK, address NACK, data NACK, timeout cleanup, repeated START; board/catalog tests select BCM I2C only on RPi3-line board data; RPi3 compile passes | SHT3x or equivalent wired to GPIO2/GPIO3; transcript shows real BCM BSC path, ACK or explicit NACK, no panic, and PASS/FAIL/BLOCKED label |
| RPi3 BCM SPI0 | `DriverId::SpiBcm0`, SoC layout for `SPI0 @ 0x3F204000`, exact MMIO grant, `ViSpi` Mode 0 baseline | Register-model tests for FIFO fill/drain, DONE timeout, CS cleanup, TX/RX length mismatch, mode bits; board/catalog tests select BCM SPI only on RPi3-line board data; RPi3 compile passes | Loopback MISO<-MOSI or known SPI device; transcript shows real BCM SPI path, expected RX/ID or explicit no-device cleanup, and PASS/FAIL/BLOCKED label |
| No-controller/unauthorized | Same binaries on non-RPi3 target or caller without matching grant class | `RequestMmio`/registry negative test returns `PermissionDenied` and never constructs a usable region | Not required; QEMU/host negative coverage is enough |
| Fallback regression | Existing `i2c-gpio`/`spi-gpio` bit-bang demos remain unchanged | Existing `periph-i2c-spi` QEMU integration still passes or skips for missing prerequisites | Not evidence for BCM promotion |

## Test Plan

### Unit Tests

| Test | Input | Expected | Failure Mode Covered |
|------|-------|----------|----------------------|
| `hal-soc-bcm27xx::tests::bcm2837_exposes_bsc_and_spi_windows` | `BCM2837.mmio` with new BSC/SPI fields | BSC and SPI bases equal documented peripheral offsets, grant sizes are exact, and each window is within `[0x3F00_0000, 0x4000_0000)` | Wrong physical address maps a random peripheral or RAM aperture |
| `boards::catalog_tests::rpi3_selects_real_bcm_i2c_spi_drivers` | `RASPBERRY_PI_3_MODEL_B.enabled_drivers` after adding `DriverId` entries | Contains real BCM BSC/SPI IDs; bit-bang `i2c-gpio`/`spi-gpio` remain absent from board promotion list | Fallback driver misreported as hardware support |
| `bcm_bsc_register_model::write_read_repeated_start_sequence` | Fake BSC MMIO status script: TX ready, transfer done, RX byte ready | Register trace shows START write phase, repeated START read phase, no STOP between phases, STOP/idle at end | Sensors that require register-pointer latch break due STOP+START |
| `bcm_bsc_register_model::address_nack_stops_and_returns_nack_address` | Fake status sets ERR on address phase | Returns `I2cError::NackAddress`; trace includes CLEAR/STOP or controller idle cleanup | Driver spins forever or leaves bus active after absent slave |
| `bcm_bsc_register_model::data_nack_stops_and_returns_nack_data` | Address ACK then ERR on data byte | Returns `I2cError::NackData`; no further TX writes after NACK | Data phase error mislabeled as address error; writes continue after NACK |
| `bcm_bsc_register_model::timeout_resets_ta_and_fifo` | Fake status never sets DONE/RX ready/TX ready within poll budget | Returns bus/timeout error; clears FIFO and leaves transfer inactive | Dead loop in Driver Cell or stale FIFO poisons next transaction |
| `bcm_bsc_register_model::rx_length_zero_and_tx_length_zero_are_bounded` | Empty read/write buffers | Either no-op success with no MMIO transaction or explicit invalid input, per implementation contract; never underflows length counters | `len - 1` underflow, accidental huge transfer |
| `bcm_spi_register_model::mode0_transfer_programs_cs_clk_len` | TX `[0x9f, 0x00]`, RX len 3, mode 0, chosen CS line | Register trace sets CPOL=0/CPHA=0, TA active during clocks, writes FIFO, drains RX, deasserts CS | Wrong SPI mode or partial-duplex assumptions |
| `bcm_spi_register_model::cs_deasserted_on_fifo_write_error` | Fake MMIO errors after first FIFO write | Returns `SpiError`; trace shows best-effort CS high/TA clear | Connected device stays selected after a fault |
| `bcm_spi_register_model::fifo_drain_before_done` | RX FIFO provides more words than requested | Driver drains or explicitly discards surplus within bounds before DONE cleanup | Next transfer reads stale bytes |
| `bcm_spi_register_model::timeout_clears_ta_and_cs` | DONE never appears | Returns transfer error; TA cleared and CS deasserted | Hung SPI controller locks bus for future transfers |

Unit seam rule: use a crate-local fake register block or `RegisterIo` trait compiled only under `#[cfg(test)]`; do not mock Cell IPC or claim this as hardware evidence. The fake must record register reads/writes and expose scripted status bits so assertions verify ordering, cleanup, and bounded polling.

### Integration Tests

| Test | Components | Setup | What's verified |
|------|------------|-------|-----------------|
| `resource_registry_rejects_absent_bcm_bsc_spi_without_device_class` | `kernel/src/resource_registry.rs` and `RequestMmio` path | Host/kernel selftest invokes new controller base with `allowed_devices = 0` or without selected board feature | No-controller / unauthorized path returns `PermissionDenied`, not a forged `MmioRegion` |
| `resource_registry_allows_exact_rpi3_bsc_spi_windows_only` | `resource_registry` + `hal-soc-bcm27xx` layout | Compile with `--features board-rpi3`; request exact BSC/SPI window, subrange, overlap, OOB range | Exact/subrange policy is deliberate; overlap returns `AlreadyExists`; OOB returns `PermissionDenied`/`InvalidInput` |
| `driver_cell_exit_releases_bcm_controller_mmio` | Driver Cell lifecycle + registry cleanup | A test cell opens BCM controller MMIO, exits or is killed, replacement cell opens same range | Release-on-exit works for controller ranges just like existing GPIO/UART grants |
| `rpi3_build_includes_bcm_i2c_spi_driver_crates` | Workspace + board feature selection | `RUSTFLAGS="-C relocation-model=pic" cargo build --release --target aarch64-unknown-none-softfloat -p cellos-kernel --features board-rpi3` plus new driver crates | Compile-time contract and feature wiring; no runtime hardware claim |
| `qemu_arm_virt_periph_i2c_spi_still_passes` | Existing fallback demos and integration harness | Existing `tests/integration/tests/periph-i2c-spi.rs` if QEMU prerequisites exist | Phase 03 did not regress bit-bang fallback lane |

Integration depth is intentionally narrow. Do not add E2E tests that try to emulate BCM controllers in QEMU ARM virt; that would create false evidence. Use integration tests for selection, grants, lifecycle, compile, and fallback non-regression only.

### E2E / Contract Tests

| Test | Flow | Tools | Threshold |
|------|------|-------|-----------|
| `rpi3_bsc_i2c_real_device_gate` | Netboot/boot RPi3 to `Cellos >`; run real I2C probe against known wired device, preferably SHT3x at `0x44`; execute `write_read(0x44, [0x2C, 0x06], 6)` | RPi3 UART capture, known-good wiring record, low-risk sensor | PASS only if log shows real BCM BSC path, address ACK, non-synthetic reading, and no bus timeout; FAIL if kernel/driver faults; BLOCKED if no wired device or unsafe rig |
| `rpi3_bcm_spi_loopback_or_known_device_gate` | Boot RPi3; run SPI transfer using physical loopback MISO<-MOSI or a known JEDEC-ID SPI device | RPi3 UART capture, loopback jumper or known chip, documented CS line | PASS only if real BCM SPI path returns expected loopback bytes or JEDEC ID and leaves CS inactive; FAIL on timeout/fault/wrong bytes; BLOCKED if wiring unavailable |
| `rpi3_no_device_fail_closed_gate` | Boot RPi3 with no I2C/SPI device connected; run controller open/probe | RPi3 UART capture | I2C reports explicit NACK/timeout without panic; SPI reports no-device marker only if a real identity probe exists; otherwise SPI open/transfer cleanup only |
| `rpi3_controller_restart_gate` | Start controller demo, terminate/force-exit it, start it again | Shell/UART command capture | Second open succeeds and transaction result is identical or explicitly NACKs; proves MMIO release after Driver Cell death |

Physical evidence format: save raw UART transcript with timestamp, board model, firmware path, kernel commit, exact Cellos command, connected pins/device, and result label `PASS`, `FAIL`, or `BLOCKED`. Existing RPi3 UART/FIFO evidence in `docs/baremetal/load-cellos.md` proves console stability only and must not be reused as I2C/SPI evidence.

## Test Data Strategy

Use static scripted register fixtures for unit tests:
- BSC fixture names should encode status script: `ack_write_read`, `address_nack`, `data_nack`, `tx_timeout`, `rx_timeout`, `stale_fifo`.
- SPI fixture names should encode controller state: `mode0_loopback`, `fifo_full_then_done`, `done_timeout`, `write_fault_after_cs`.
- Keep fake register traces in the new driver crate's `#[cfg(test)]` module so production code remains `no_std` and no extra public API is added.

Physical data:
- Use deterministic I2C command bytes for SHT3x high precision single shot: address `0x44`, command `[0x2C, 0x06]`, read 6 bytes.
- Use SPI loopback with bytes `[0x00, 0xA5, 0x5A, 0xFF]` or a known JEDEC-ID read if a flash/device is connected.
- No PII. Hardware transcripts may include local paths and serial adapter names; keep those in `.agents/.../reports/` unless promoted to docs intentionally.

## Flakiness Risks

- Busy polling with real controller status bits: set explicit poll budgets and assert timeout path in unit tests.
- RPi3 wiring errors: require a wiring preamble in the transcript and classify missing/unsafe wiring as `BLOCKED`, not failed driver logic.
- QEMU skip behavior: existing integration tests skip when QEMU/kernel/disk are missing; do not count skip as pass for Phase 03.
- Shared bus residue: always test cleanup after NACK/timeout before a second transaction.
- Physical SPI floating MISO: prefer loopback or known device ID; do not assert arbitrary RX values from an unconnected line.

## Existing Coverage Gaps

- No BCM BSC/SPI controller crates or workspace entries are present yet; only `i2c-gpio` and `spi-gpio` fallback crates exist.
- `DriverId` currently has GPIO/UART/IRQ/timer/SDHCI/PCIe/NVMe/e1000 entries but no real I2C/SPI controller IDs.
- `resource_registry.rs` has an RPi3 comment for future BCM I2C/SPI allowlist entries but no active device class or ranges for them.
- `hal-soc-bcm27xx` exposes BCM2837 GPIO/AUX/SDHCI data only; BSC/SPI offsets must be added and bounded by tests.
- Existing QEMU peripheral tests validate bit-bang fallback marker strings, not hardware controller semantics.

## Out of Scope

- DesignWare I2C/SPI tests for JH7110 or other boards until DT/manual compatible evidence exists.
- Multi-Cell I2C/SPI broker IPC and `libs/api` request/response contracts; Phase 03 should stay in rlib/single-owner or existing syscall boundaries unless the user explicitly approves an ABI expansion.
- Performance/latency benchmarking beyond bounded-poll correctness; no throughput target is stated.
- PWM/ADC/CAN promotion; they remain fallback/sim/loopback in later phases.
- Claiming RPi4/VF2/Pioneer evidence from RPi3 or QEMU results.
