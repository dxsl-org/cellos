const GPIO_BASE: usize = 0x3F20_0000;
const GPIO_INPUT: u32 = 0;
const GPIO_ALT3: u32 = 7;

#[inline]
fn function_select_register(pin: u32) -> *mut u32 {
    (GPIO_BASE + ((pin / 10) as usize * 4)) as *mut u32
}

fn set_function(pin: u32, function: u32) {
    let register = function_select_register(pin);
    let shift = (pin % 10) * 3;
    // SAFETY: board-rpi3 maps the BCM2837 peripheral range before driver init.
    let current = unsafe { core::ptr::read_volatile(register) };
    let updated = (current & !(0b111 << shift)) | (function << shift);
    // SAFETY: this preserves every function-select field except the requested pin.
    unsafe { core::ptr::write_volatile(register, updated) };
}

/// Route the external SD slot to the Arasan controller used by Cellos.
///
/// Raspberry Pi firmware normally connects Arasan to Wi-Fi on GPIO34-39 and
/// SDHOST to the external slot on GPIO48-53. Cellos has an Arasan driver, so it
/// applies the same pin routing as Raspberry Pi's `mmc` overlay before probing.
pub(super) fn route_external_sd_to_arasan() {
    for pin in 34..=39 {
        set_function(pin, GPIO_INPUT);
    }
    for pin in 48..=53 {
        set_function(pin, GPIO_ALT3);
    }

    log::info!("[mmc] RPi3 external SD routed to Arasan (GPIO48-53 ALT3)");
}
