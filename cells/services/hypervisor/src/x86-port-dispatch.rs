//! Legacy x86 port-I/O dispatch.

extern crate alloc;

use crate::{cmos_rtc::CmosRtc, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550};
use ostd::io::println;

pub fn write(
    port: u16,
    value: u32,
    uart: &mut Uart16550,
    pic: &mut Pic8259,
    pit: &mut Pit8253,
    rtc: &mut CmosRtc,
) -> bool {
    if Uart16550::owns(port) {
        uart.write(port, value);
    } else if Pic8259::owns(port) {
        pic.write(port, value);
    } else if Pit8253::owns(port) {
        pit.write(port, value);
    } else if CmosRtc::owns(port) {
        rtc.write(port, value);
    } else if matches!(port, 0x604 | 0x501 | 0xB004) {
        println("[hv-x86] guest power-off port write");
        return false;
    } else {
        println(&alloc::format!(
            "[hv-x86] unhandled OUT port=0x{:x} val=0x{:x}",
            port,
            value
        ));
    }
    true
}

pub fn read(
    port: u16,
    uart: &mut Uart16550,
    pic: &Pic8259,
    pit: &mut Pit8253,
    rtc: &mut CmosRtc,
) -> u32 {
    if Uart16550::owns(port) {
        uart.read(port)
    } else if Pic8259::owns(port) {
        pic.read(port)
    } else if Pit8253::owns(port) {
        pit.read(port)
    } else if CmosRtc::owns(port) {
        rtc.read(port)
    } else {
        0xFFFF_FFFF
    }
}
