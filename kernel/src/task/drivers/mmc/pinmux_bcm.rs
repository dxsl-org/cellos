const GPIO_INPUT: u32 = 0;
const GPIO_ALT3: u32 = 7;

#[inline]
fn function_select_register(gpio_base: usize, pin: u32) -> *mut u32 {
    (gpio_base + ((pin / 10) as usize * 4)) as *mut u32
}

fn set_function(gpio_base: usize, pin: u32, function: u32) {
    let register = function_select_register(gpio_base, pin);
    let shift = (pin % 10) * 3;
    // SAFETY: the selected BCM peripheral aperture is mapped before MMC init.
    let current = unsafe { core::ptr::read_volatile(register) };
    let updated = (current & !(0b111 << shift)) | (function << shift);
    // SAFETY: this preserves every function-select field except the requested pin.
    unsafe { core::ptr::write_volatile(register, updated) };
}

pub(super) fn apply(gpio_base: usize, wiring: cellos_boards::WiringLayout) {
    if wiring.pinmux_groups.contains(&"sd-gpio48-53-alt3") {
        for pin in 34..=39 {
            set_function(gpio_base, pin, GPIO_INPUT);
        }
    }

    if wiring
        .pinmux_groups
        .iter()
        .any(|group| matches!(*group, "sd-gpio48-53-alt3" | "emmc2-gpio48-53-alt3"))
    {
        for pin in 48..=53 {
            set_function(gpio_base, pin, GPIO_ALT3);
        }
        log::info!("[mmc] BCM SD pins routed from board wiring");
    }
}
