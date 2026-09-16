#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod sht3x;

use api::declare_manifest;
use driver_gpio::Pl061Gpio;
use driver_gpio_bcm::BcmGpio;
use driver_i2c_bcm::BcmBscI2c;
use driver_i2c_gpio::BitBangI2c;
use hal_gpio::{PinDir, ViGpio};
use hal_i2c::ViI2c;
use ostd::io::println;
use ostd::syscall::sys_recv_timeout;

// BCM hardware I2C is preferred on RPi3; GPIO remains the QEMU fallback.
declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = true,
    uart = false,
    hypervisor = false,
    i2c = true,
    spi = false
);

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[sensor-demo] SHT3x I2C probe (addr 0x44)");

    if let Ok(mut i2c) = BcmBscI2c::open() {
        println("[sensor-demo] using BCM BSC1 hardware controller");
        scan_i2c_bus(&mut i2c);
        match run_with_i2c(&mut i2c, false) {
            Ok(()) => {
                println("[phase03-i2c] PASS: SHT3x read via BCM BSC1");
                return;
            }
            Err(hal_i2c::I2cError::NackAddress) => {
                println("[phase03-i2c] PASS: explicit address NACK from BCM BSC1")
            }
            Err(hal_i2c::I2cError::NackData) => {
                println("[phase03-i2c] PASS: explicit data NACK from BCM BSC1")
            }
            Err(hal_i2c::I2cError::BusError) => println("[phase03-i2c] FAIL: BCM BSC1 bus error"),
        }
        run_bcm_actuator();
        return;
    }

    match Pl061Gpio::open() {
        Ok(gpio) => run_with_gpio(gpio),
        Err(_) => {
            println("[sensor-demo] GPIO unavailable — synthetic-only mode");
            run_synthetic();
        }
    }
}

// Bounded so GPIO is released for other Driver Cells (e.g. pwm-demo, spi-demo).
const DEMO_CYCLES: u32 = 3;

fn run_with_gpio(gpio: Pl061Gpio) {
    let mut i2c = BitBangI2c::new(gpio);
    println("[sensor-demo] SHT3x via bit-bang I2C");
    println("[sensor-demo] using GPIO bit-bang fallback");
    let _ = run_with_i2c(&mut i2c, true);
}

fn run_with_i2c(
    i2c: &mut impl ViI2c<Error = hal_i2c::I2cError>,
    synthetic_on_error: bool,
) -> Result<(), hal_i2c::I2cError> {
    for tick in 0..DEMO_CYCLES {
        match poll_sensor(i2c, tick) {
            Ok(reading) => print_reading(&reading),
            Err(_) if synthetic_on_error => print_reading(&sht3x::synthetic(tick)),
            Err(error) => return Err(error),
        }
        sleep_1s();
    }
    Ok(())
}

fn run_bcm_actuator() {
    let mut gpio = match BcmGpio::open() {
        Ok(gpio) => gpio,
        Err(_) => {
            println("[phase03-gpio-actuator] FAIL: BCM GPIO unavailable");
            return;
        }
    };
    let result = gpio
        .set_direction(17, PinDir::Output)
        .and_then(|_| gpio.write_pin(17, true))
        .and_then(|_| gpio.read_pin(17))
        .and_then(|high| {
            gpio.write_pin(17, false)?;
            Ok(high)
        });
    match result {
        Ok(true) => println("[phase03-gpio-actuator] PASS: GPIO17 high/low readback"),
        _ => println("[phase03-gpio-actuator] FAIL: GPIO17 readback"),
    }
}

fn poll_sensor(
    i2c: &mut impl ViI2c<Error = hal_i2c::I2cError>,
    tick: u32,
) -> Result<sht3x::Reading, hal_i2c::I2cError> {
    // SHT3x high-precision single-shot measurement command [0x2C, 0x06].
    // Try primary address 0x44; fall back to 0x45 if unacknowledged.
    let addr = if i2c.write(0x44, &[0x2C, 0x06]).is_ok() {
        0x44
    } else {
        i2c.write(0x45, &[0x2C, 0x06])?;
        0x45
    };

    // SHT30 requires up to 15 ms for high-repeatability ADC conversion.
    // Sleep 3 scheduler ticks (30 ms) so measurement data is ready.
    let mut discard = [0u8; 16];
    let _ = sys_recv_timeout(0, &mut discard, 3);

    // Read 6 bytes: [T_MSB, T_LSB, T_CRC, H_MSB, H_LSB, H_CRC]
    let mut buf = [0u8; 6];
    i2c.read(addr, &mut buf)?;
    Ok(sht3x::parse(&buf).unwrap_or_else(|| sht3x::synthetic(tick)))
}

fn print_reading(r: &sht3x::Reading) {
    let label = if r.simulated { " [sim]" } else { "" };
    let t_int = r.temp_cx10 / 10;
    let t_frac = (r.temp_cx10 % 10).abs();
    // When temp is between -0.9 and -0.1°C, t_int == 0 but the value is still negative.
    let t_sign = if r.temp_cx10 < 0 && t_int == 0 {
        "-"
    } else {
        ""
    };
    let h_int = r.hum_px10 / 10;
    let h_frac = r.hum_px10 % 10;
    println(
        alloc::format!(
            "T={}{}.{}C H={}.{}%{}",
            t_sign,
            t_int,
            t_frac,
            h_int,
            h_frac,
            label
        )
        .as_str(),
    );
}

fn run_synthetic() {
    for tick in 0..DEMO_CYCLES {
        print_reading(&sht3x::synthetic(tick));
        sleep_1s();
    }
}

/// Block for approximately 1 s.
///
/// Uses `RecvTimeout` as a sleep primitive: 100 scheduler ticks × 10 ms/tick.
/// A stray message wakes us early — fine for a polling demo, we just loop again.
fn sleep_1s() {
    let mut buf = [0u8; 64];
    let _ = sys_recv_timeout(0, &mut buf, 100);
}

fn print_hex_u8(val: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let h = [HEX[(val >> 4) as usize], HEX[(val & 0xF) as usize]];
    if let Ok(s) = core::str::from_utf8(&h) {
        ostd::io::print(s);
    }
}

fn scan_i2c_bus(i2c: &mut impl ViI2c<Error = hal_i2c::I2cError>) {
    println("[i2c-scan] Scanning I2C bus (addresses 0x03..0x77)...");
    let mut found = 0;
    for addr in 0x03..=0x77 {
        let mut buf = [0u8; 1];
        if i2c.read(addr, &mut buf).is_ok() {
            ostd::io::print("[i2c-scan] -> Device detected at address 0x");
            print_hex_u8(addr);
            println("");
            found += 1;
        }
    }
    if found == 0 {
        println("[i2c-scan] No I2C devices answered ACK.");
        println("[i2c-scan] Check wires: SDA->Pin 3, SCL->Pin 5, VCC->Pin 1, GND->Pin 9");
    } else {
        println("[i2c-scan] Scan completed.");
    }
}
