const GPIO_ALT0: u32 = 4;

#[inline]
fn function_select_register(gpio_base: usize, pin: u32) -> *mut u32 {
    (gpio_base + ((pin / 10) as usize * 4)) as *mut u32
}

fn set_function(gpio_base: usize, pin: u32, function: u32) {
    let register = function_select_register(gpio_base, pin);
    let shift = (pin % 10) * 3;
    // SAFETY: the BCM peripheral aperture is mapped before driver initialization.
    let current = unsafe { core::ptr::read_volatile(register) };
    let updated = (current & !(0b111 << shift)) | (function << shift);
    // SAFETY: this preserves every function-select field except the requested pin.
    unsafe { core::ptr::write_volatile(register, updated) };
}

pub(super) fn apply(gpio_base: usize, wiring: cellos_boards::WiringLayout) {
    if wiring.pinmux_groups.contains(&"i2c1-gpio2-3-alt0") {
        for pin in 2..=3 {
            set_function(gpio_base, pin, GPIO_ALT0);
        }
        log::info!("[pinmux] BCM BSC1 routed to GPIO2-3 ALT0");
    }

    if wiring.pinmux_groups.contains(&"spi0-gpio7-11-alt0") {
        for pin in 7..=11 {
            set_function(gpio_base, pin, GPIO_ALT0);
        }
        log::info!("[pinmux] BCM SPI0 routed to GPIO7-11 ALT0");
    }
}
