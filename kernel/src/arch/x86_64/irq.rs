// SPDX-License-Identifier: MPL-2.0
//! `irq`: local interrupt delivery.

/// Enable interrupts on this CPU (STI through the Arch trait).
pub(crate) fn enable() {
    use crate::hal::Arch;
    crate::hal::ARCH.enable_interrupts();
}
