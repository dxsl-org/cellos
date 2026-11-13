// SPDX-License-Identifier: MPL-2.0
//! `gpio_irq`: the board backend's GPIO controller and USB one-shot IRQ, for
//! `crate::dev::gpio_irq`.

/// GPIO MMIO base — the board backend's GPIO controller (PL061 on QEMU ARM
/// virt, the BCM GPIO block on Raspberry Pi). Must match the allowlist entry in
/// resource_registry.rs for the current target.
pub(crate) const GPIO_MMIO_BASE: usize =
    <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::GPIO_MMIO_BASE;

const _: crate::hal::GpioNotifyIrq = crate::dev::gpio_irq::vi_gpio_notify_irq;

/// Signal the board's USB one-shot IRQ, if it has one.
pub(crate) fn signal_usb_irq() {
    if let Some(usb_irq) =
        <hal::aarch64::backend::Board as hal::aarch64::backend::ArmIrqController>::USB_ONE_SHOT_IRQ
    {
        crate::dev::irq_wait::signal_irq(usb_irq as u8);
    }
}

const _: crate::hal::UsbSignalIrq = crate::dev::gpio_irq::vi_signal_usb_irq;
