#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::declare_manifest;
use driver_display_ssd1306::{Ssd1306, HEIGHT, WIDTH};
use driver_gpio_bcm::BcmGpio;
use driver_spi_bcm::BcmSpi0;
use ostd::io::println;

// Raspberry Pi 3 default GPIO pins for SPI display:
// D/C: GPIO24 (pin 18)
// RST: GPIO25 (pin 22)
const DC_PIN: u8 = 24;
const RST_PIN: u8 = 25;

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
    println("[ssd1306] SSD1306 128x64 SPI OLED driver cell starting...");

    let spi = match BcmSpi0::open() {
        Ok(spi) => {
            println("[ssd1306] BCM SPI0 controller acquired");
            spi
        }
        Err(_) => {
            println("[ssd1306] failed to acquire BCM SPI0 controller");
            return;
        }
    };

    let gpio = match BcmGpio::open() {
        Ok(gpio) => {
            println("[ssd1306] BCM GPIO controller acquired");
            gpio
        }
        Err(_) => {
            println("[ssd1306] failed to acquire BCM GPIO controller");
            return;
        }
    };

    let mut display = match Ssd1306::new(spi, gpio, DC_PIN, Some(RST_PIN)) {
        Ok(disp) => disp,
        Err(_) => {
            println("[ssd1306] failed to initialize display struct");
            return;
        }
    };

    match display.init() {
        Ok(()) => {
            println("[ssd1306] SSD1306 initialized successfully");
        }
        Err(_) => {
            println("[ssd1306] display init sequence failed");
            return;
        }
    }

    // Draw test pattern: diagonal lines and border
    for x in 0..WIDTH {
        display.set_pixel(x, 0, true);
        display.set_pixel(x, HEIGHT - 1, true);
    }
    for y in 0..HEIGHT {
        display.set_pixel(0, y, true);
        display.set_pixel(WIDTH - 1, y, true);
    }
    for i in 0..HEIGHT {
        display.set_pixel(i * 2, i, true);
    }

    let _ = display.flush();
    println("[ssd1306] test pattern rendered to OLED display");
}
