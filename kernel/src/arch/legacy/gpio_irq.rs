// SPDX-License-Identifier: MPL-2.0
//! `gpio_irq`: no board GPIO controller or USB one-shot IRQ on this target.

/// No GPIO MMIO window: no cell owns it, so no GPIO notification is sent.
pub(crate) const GPIO_MMIO_BASE: usize = 0;

/// No USB one-shot IRQ to signal.
pub(crate) fn signal_usb_irq() {}
