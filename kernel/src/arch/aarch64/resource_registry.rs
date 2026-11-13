// SPDX-License-Identifier: MPL-2.0
//! aarch64 items of `crate::resource_registry`, compiled as its `aarch64` child module
//! (`#[path]`), so they keep reading that module's private items unchanged.

use super::*;

/// `(base, len, device_class)` triples a Driver Cell may request. The device
/// class scopes the capability: a cell may claim a range only if it declared
/// the matching device class, so one peripheral class cannot claim another
/// class's controller window.
///
/// AArch64: one table per SoC, kept here so the `DEV_*` bits stay kernel-local;
/// the board backend's [`ArmSoc`](hal::aarch64::backend::ArmSoc) selects it.
pub(super) const ALLOWED: &[(usize, usize, u8)] =
    match <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SOC {
        hal::aarch64::backend::ArmSoc::QemuArmVirt => QEMU_ARM_VIRT_ALLOWED,
        hal::aarch64::backend::ArmSoc::Bcm2837 => BCM2837_ALLOWED,
        hal::aarch64::backend::ArmSoc::Bcm2711 => BCM2711_ALLOWED,
        hal::aarch64::backend::ArmSoc::AppleT8103 => APPLE_T8103_ALLOWED,
    };

pub(super) const APPLE_T8103_ALLOWED: &[(usize, usize, u8)] = &[];

pub(super) const BCM2837_ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_bcm27xx::BCM2837.mmio.gpio_base,
        hal_soc_bcm27xx::BCM2837.mmio.gpio_grant_size,
        DEV_GPIO,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.aux_base,
        hal_soc_bcm27xx::BCM2837.mmio.aux_grant_size,
        DEV_UART,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.bsc1_base,
        hal_soc_bcm27xx::BCM2837.mmio.bsc1_grant_size,
        DEV_I2C,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.spi0_base,
        hal_soc_bcm27xx::BCM2837.mmio.spi0_grant_size,
        DEV_SPI,
    ),
    (
        hal_soc_bcm27xx::BCM2837.mmio.mailbox_base,
        hal_soc_bcm27xx::BCM2837.mmio.mailbox_grant_size,
        DEV_DISPLAY,
    ),
];

pub(super) const QEMU_ARM_VIRT_ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base,
        hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.size,
        DEV_UART,
    ),
    (
        hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.mmio.base,
        hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.mmio.size,
        DEV_GPIO,
    ),
];

pub(super) const BCM2711_ALLOWED: &[(usize, usize, u8)] = &[
    (
        hal_soc_bcm27xx::BCM2711.mmio.uart_base,
        hal_soc_bcm27xx::BCM2711.mmio.uart_grant_size,
        DEV_UART,
    ),
    (
        hal_soc_bcm27xx::BCM2711.mmio.gpio_base,
        hal_soc_bcm27xx::BCM2711.mmio.gpio_grant_size,
        DEV_GPIO,
    ),
];

// Authority-table equality (Q5): every AArch64 build checks all three tables at
// compile time against the pre-plan snapshot (baseline `6d5cdeb0c`, values
// copied once from the SoC profiles). A drifted profile or a mis-moved entry
// fails the build instead of widening a Cell's MMIO reach.
pub(super) const BCM2837_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0x3F20_0000, 0x1000, DEV_GPIO),
    (0x3F21_5000, 0x1000, DEV_UART),
    (0x3F80_4000, 0x1000, DEV_I2C),
    (0x3F20_4000, 0x1000, DEV_SPI),
    // Narrowed from 0x1000 (H-18): the mailbox registers only, never the
    // IRQ-controller page they sit on; accesses are kernel-mediated.
    (0x3F00_B880, 0x40, DEV_DISPLAY),
];

pub(super) const QEMU_ARM_VIRT_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0x0900_0000, 0x1000, DEV_UART),
    (0x0903_0000, 0x1000, DEV_GPIO),
];

pub(super) const BCM2711_ALLOWED_SNAPSHOT: &[(usize, usize, u8)] = &[
    (0xFE20_1000, 0x1000, DEV_UART),
    (0xFE20_0000, 0x1000, DEV_GPIO),
];

const _: () = assert!(allowlists_equal(BCM2837_ALLOWED, BCM2837_ALLOWED_SNAPSHOT));

const _: () = assert!(allowlists_equal(
    QEMU_ARM_VIRT_ALLOWED,
    QEMU_ARM_VIRT_ALLOWED_SNAPSHOT
));

const _: () = assert!(allowlists_equal(BCM2711_ALLOWED, BCM2711_ALLOWED_SNAPSHOT));

/// Entry-by-entry, order-sensitive equality of two allowlists.
pub(super) const fn allowlists_equal(a: &[(usize, usize, u8)], b: &[(usize, usize, u8)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].0 != b[i].0 || a[i].1 != b[i].1 || a[i].2 != b[i].2 {
            return false;
        }
        i += 1;
    }
    true
}

/// Power-on self-test of the QEMU ARM virt production MMIO allowlist (Q5).
///
/// Pins, on the REAL table (`ALLOWED`): it is the virt table, entry for entry;
/// every window authorizes its own device class; and no window authorizes any
/// other class, so one peripheral class cannot claim another's controller.
pub(super) fn qemu_arm_virt_self_test() -> bool {
    if !allowlists_equal(ALLOWED, QEMU_ARM_VIRT_ALLOWED_SNAPSHOT) {
        log::error!("[selftest] mmio-allowlist: FAIL — selected table is not the QEMU virt table");
        return false;
    }
    for &(base, len, class) in ALLOWED {
        let end = base + len;
        if static_range_class(ALLOWED, base, end, class) != Some(class) {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — window {base:#x} no longer authorizes \
                 class {class:#04x}"
            );
            return false;
        }
        if static_range_class(ALLOWED, base, end, !class).is_some() {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — window {base:#x} authorized for a class \
                 other than {class:#04x}"
            );
            return false;
        }
    }
    true
}

/// Power-on self-test of the RPi3 production MMIO allowlist.
///
/// Pins two properties on the REAL table (`ALLOWED`), not a synthetic copy:
///
/// 1. Positive control — a known window (mailbox/DEV_DISPLAY) still authorizes.
///    Without this, an accidentally emptied table would make every denial
///    below vacuous.
/// 2. DWC2 denial — the DWC2 USB window must be rejected for every device
///    class, full masks included, over the whole aperture AND at its edges.
///    The entry was previously present mis-tagged `DEV_PCIE`; it may return
///    only via policy v3 with a signed USB-host authority byte.
pub(super) fn bcm2837_self_test() -> bool {
    let mmio = hal_soc_bcm27xx::BCM2837.mmio;

    // 1. Positive control: the table is live.
    let mbox_base = mmio.mailbox_base;
    let mbox_end = mbox_base + mmio.mailbox_grant_size;
    if static_range_class(ALLOWED, mbox_base, mbox_end, DEV_DISPLAY).is_none() {
        log::error!("[selftest] mmio-allowlist: FAIL — mailbox window no longer authorized");
        return false;
    }

    // 2. DWC2 denial across the full aperture, per class and combined masks.
    let dwc2_base = mmio.dwc2_base;
    let dwc2_end = dwc2_base + mmio.dwc2_grant_size;
    let classes = [
        DEV_UART,
        DEV_GPIO,
        DEV_PCIE,
        DEV_CAN,
        DEV_ADC,
        DEV_I2C,
        DEV_SPI,
        DEV_DISPLAY,
        0xFF,
    ];
    for class in classes {
        if static_range_class(ALLOWED, dwc2_base, dwc2_end, class).is_some() {
            log::error!(
                "[selftest] mmio-allowlist: FAIL — DWC2 window authorized for class {class:#04x}"
            );
            return false;
        }
    }

    // Edge words: same full class sweep as the whole aperture — a future
    // sub-range entry tagged with ANY class must fail the boot self-test.
    let last_word = dwc2_end - 4;
    for addr in [dwc2_base, last_word] {
        for class in classes {
            if static_range_class(ALLOWED, addr, addr + 4, class).is_some() {
                log::error!(
                    "[selftest] mmio-allowlist: FAIL — DWC2 edge word {addr:#x} authorized \
                     for class {class:#04x}"
                );
                return false;
            }
        }
    }
    true
}
