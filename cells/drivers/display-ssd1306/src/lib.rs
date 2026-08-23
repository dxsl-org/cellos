#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use hal_gpio::{PinDir, ViGpio};
use hal_spi::ViSpi;
use types::ViResult;

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
pub const BUFFER_SIZE: usize = (WIDTH * HEIGHT) / 8; // 1024 bytes

pub struct Ssd1306<SPI, GPIO> {
    spi: SPI,
    gpio: GPIO,
    dc_pin: u8,
    rst_pin: Option<u8>,
    buffer: [u8; BUFFER_SIZE],
}

impl<SPI: ViSpi, GPIO: ViGpio> Ssd1306<SPI, GPIO> {
    pub fn new(spi: SPI, mut gpio: GPIO, dc_pin: u8, rst_pin: Option<u8>) -> ViResult<Self> {
        gpio.set_direction(dc_pin, PinDir::Output)?;
        if let Some(rst) = rst_pin {
            gpio.set_direction(rst, PinDir::Output)?;
            gpio.write_pin(rst, true)?;
        }

        Ok(Self {
            spi,
            gpio,
            dc_pin,
            rst_pin,
            buffer: [0u8; BUFFER_SIZE],
        })
    }

    pub fn reset(&mut self) -> ViResult<()> {
        if let Some(rst) = self.rst_pin {
            self.gpio.write_pin(rst, false)?;
            self.gpio.write_pin(rst, true)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn write_command(&mut self, cmd: u8) -> Result<(), SPI::Error> {
        let _ = self.gpio.write_pin(self.dc_pin, false); // D/C Low = Command
        self.spi.write(&[cmd])
    }

    fn write_commands(&mut self, cmds: &[u8]) -> Result<(), SPI::Error> {
        let _ = self.gpio.write_pin(self.dc_pin, false); // D/C Low = Command
        self.spi.write(cmds)
    }

    pub fn init(&mut self) -> Result<(), SPI::Error> {
        let _ = self.reset();

        let init_sequence = [
            0xAE, // Display OFF
            0xD5, 0x80, // Set display clock divide ratio/oscillator freq
            0xA8, 0x3F, // Set multiplex ratio (64 MUX)
            0xD3, 0x00, // Set display offset = 0
            0x40, // Set start line = 0
            0x8D, 0x14, // Enable charge pump regulator
            0x20, 0x00, // Set memory addressing mode = Horizontal
            0xA1, // Set segment re-map (col 127 mapped to SEG0)
            0xC8, // Set COM output scan direction (remapped)
            0xDA, 0x12, // Set COM pins hardware config
            0x81, 0xCF, // Set contrast control
            0xD9, 0xF1, // Set pre-charge period
            0xDB, 0x40, // Set VCOMH deselect level
            0xA4, // Entire display ON (resume to RAM content)
            0xA6, // Normal display
            0xAF, // Display ON
        ];

        self.write_commands(&init_sequence)?;
        self.clear();
        self.flush()
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0);
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, on: bool) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let page = y / 8;
        let bit = y % 8;
        let index = page * WIDTH + x;
        if on {
            self.buffer[index] |= 1 << bit;
        } else {
            self.buffer[index] &= !(1 << bit);
        }
    }

    pub fn flush(&mut self) -> Result<(), SPI::Error> {
        // Set column address range 0..127
        self.write_commands(&[0x21, 0, 127])?;
        // Set page address range 0..7
        self.write_commands(&[0x22, 0, 7])?;
        // Write entire frame buffer
        let _ = self.gpio.write_pin(self.dc_pin, true); // D/C High = Data
        self.spi.write(&self.buffer)
    }
}
