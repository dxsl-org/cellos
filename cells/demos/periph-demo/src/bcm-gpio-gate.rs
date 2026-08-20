use driver_gpio_bcm::BcmGpio;
use hal_gpio::{Edge, PinDir, ViGpio};
use ostd::io::println;
use ostd::syscall::sys_recv_timeout;
use types::ViError;

const OUTPUT_PIN: u8 = 17;
const INPUT_PIN: u8 = 27;
const INPUT_MASK: u32 = 1 << INPUT_PIN;

/// Run the RPi3 GPIO17-to-GPIO27 physical loopback gate.
///
/// Returns `false` only when BCM GPIO is not allowlisted, allowing QEMU to run
/// the existing PL061 gate. Once BCM ownership is available, failures stay visible.
pub fn run() -> bool {
    let mut gpio = match BcmGpio::open() {
        Ok(gpio) => gpio,
        Err(ViError::PermissionDenied) => {
            println("[phase03-gpio] BCM GPIO permission denied; trying PL061 fallback");
            return false;
        }
        Err(_) => {
            println("[phase03-gpio] FAIL: BCM GPIO open");
            return true;
        }
    };

    let setup = gpio
        .set_direction(OUTPUT_PIN, PinDir::Output)
        .and_then(|_| gpio.set_direction(INPUT_PIN, PinDir::Input))
        .and_then(|_| gpio.write_pin(OUTPUT_PIN, false))
        .and_then(|_| gpio.disable_irq(INPUT_PIN))
        .and_then(|_| gpio.clear_eds(0, INPUT_MASK));
    let precheck = gpio.read_eds(0);
    if setup.is_err() || precheck.is_err() || precheck.is_ok_and(|eds| eds & INPUT_MASK != 0) {
        println("[phase03-gpio] FAIL: negative precheck");
        return true;
    }
    println("[phase03-gpio] negative precheck PASS");

    let armed = gpio.enable_edge_irq(INPUT_PIN, Edge::Rising).is_ok();
    let toggled = armed && gpio.write_pin(OUTPUT_PIN, true).is_ok();
    if !toggled {
        let _ = gpio.clear_eds(0, INPUT_MASK);
        let _ = gpio.disable_irq(INPUT_PIN);
        let _ = gpio.write_pin(OUTPUT_PIN, false);
        println("[phase03-gpio] FAIL: arm/toggle");
        return true;
    }

    let mut detected = false;
    for _ in 0..50 {
        if gpio.read_eds(0).is_ok_and(|eds| eds & INPUT_MASK != 0) {
            detected = true;
            break;
        }
        let mut message = [0u8; 16];
        let _ = sys_recv_timeout(0, &mut message, 1);
    }
    let _ = gpio.clear_eds(0, INPUT_MASK);
    let _ = gpio.disable_irq(INPUT_PIN);
    let _ = gpio.write_pin(OUTPUT_PIN, false);

    if detected {
        println("[phase03-gpio] PASS: GPIO17->GPIO27 rising edge detected");
    } else {
        println("[phase03-gpio] FAIL: connect physical pin 11 to pin 13");
    }
    true
}
