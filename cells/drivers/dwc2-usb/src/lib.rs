// `deny`, not `forbid`: the DMA payload copies need a documented `allow`
// island under the F1 allowlist (scripts/unsafe-allowlist.toml).
#![no_std]
#![deny(unsafe_code)]

use ostd::syscall::{sys_get_time_ms, sys_yield};

pub mod dispatch;
pub mod dwc2;
pub mod hid;
pub mod hub;
pub mod lan9514;
pub mod regs;
pub mod usb_channel;
pub mod usb_desc;
pub mod usb_hid;

pub use dwc2::Dwc2Controller;
pub use hub::UsbHub;
pub use lan9514::Lan9514Device;
pub use usb_channel::UsbHostEngine;

/// Block for `ms` milliseconds.
///
/// Every `for _ in 0..N { sys_yield(); }` loop in this driver stood in for a
/// delay, but `sys_yield` returns as soon as there is nothing else runnable, so
/// those loops measured nothing at all. USB is full of intervals during which a
/// device is entitled to ignore the bus -- `TRSTRCY` after a port reset,
/// `TDSETADDR` after `SetAddress()` -- and a host that does not wait them out
/// sees a device that looks dead. Busy-waiting on the monotonic clock is the
/// honest form of those loops.
pub fn delay_ms(ms: u64) {
    let Some(start) = sys_get_time_ms() else {
        // The manifest does not grant `GetTime`. Yield a bounded number of
        // times so the caller still gives up the CPU rather than spinning hot.
        for _ in 0..ms.saturating_mul(100) {
            sys_yield();
        }
        return;
    };
    while sys_get_time_ms().is_some_and(|now| now.wrapping_sub(start) < ms) {
        sys_yield();
    }
}
