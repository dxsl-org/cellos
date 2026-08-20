#![no_std]
#![no_main]
#![forbid(unsafe_code)]
// The `main` symbol comes from `ostd::cell_main!`, whose expansion carries the
// `#[no_mangle]` the ELF loader needs without tripping `forbid(unsafe_code)`
// (see libs/ostd/src/entry.rs).
// All peripheral-access code is unsafe-free (uses MmioRegion abstraction).

extern crate alloc;

use api::declare_manifest;
use driver_gpio::Pl061Gpio;
use driver_spi_bcm::BcmSpi0;
use driver_spi_gpio::BitBangSpi;
use hal_spi::{SpiError, ViSpi};
use ostd::io::println;
use types::ViError;

// BCM hardware SPI is preferred on RPi3; GPIO remains the QEMU fallback.
declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = true,
    uart = false,
    hypervisor = false,
    i2c = false,
    spi = true
);

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[spi-demo] SPI controller probe");

    if let Ok(mut spi) = BcmSpi0::open() {
        println("[spi-demo] using BCM SPI0 hardware controller");
        return run_spi_demo(&mut spi, true);
    }

    match Pl061Gpio::open() {
        Ok(gpio) => run_gpio_spi_demo(gpio),
        Err(ViError::PermissionDenied) => {
            println("[spi-demo] SPI unavailable (gpio cap not granted — non-aarch64 target)");
        }
        Err(_) => {
            println("[spi-demo] SPI unavailable (GPIO open failed)");
        }
    }
}

fn run_gpio_spi_demo(gpio: Pl061Gpio) {
    let mut spi = BitBangSpi::new(gpio);
    println("[spi-demo] using GPIO bit-bang fallback");
    run_spi_demo(&mut spi, false);
}

fn run_spi_demo(spi: &mut impl ViSpi<Error = SpiError>, require_loopback: bool) {
    // ── TX-only write: primary assertion ─────────────────────────────────────
    // write() clocks out bytes via MOSI/SCK/CS without sampling MISO.
    // On QEMU this validates the full GPIO MMIO path.
    match spi.write(&[0xA5, 0x3C, 0x00]) {
        Ok(()) => println("[spi-demo] SPI TX OK (0xA5 0x3C 0x00)"),
        Err(_) => {
            println("[spi-demo] SPI BusError — TX failed");
            return;
        }
    }

    // ── Full-duplex transfer: MISO floats to 0x00 in QEMU ────────────────────
    // transfer() clocks out tx bytes and simultaneously clocks in rx bytes.
    // QEMU PL061 has no MOSI→MISO loopback; rx will be 0x00 — expected.
    let mut rx = [0xFFu8; 2];
    match spi.transfer(&[0xAA, 0x55], &mut rx) {
        Ok(()) => {
            if require_loopback {
                if rx == [0xAA, 0x55] {
                    println("[phase03-spi] PASS: BCM SPI0 MOSI-MISO loopback AA55");
                } else {
                    let msg = alloc::format!(
                        "[phase03-spi] FAIL: expected AA55, received {:02X}{:02X}",
                        rx[0],
                        rx[1]
                    );
                    println(&msg);
                }
            } else {
                let msg = alloc::format!(
                    "[spi-demo] SPI transfer OK: sent 0xAA 0x55, recv 0x{:02X} 0x{:02X} (QEMU MISO=0)",
                    rx[0], rx[1]
                );
                println(&msg);
            }
        }
        Err(_) => {
            println("[spi-demo] SPI BusError — transfer failed");
            return;
        }
    }

    println("[spi-demo] SPI demo complete");
}
