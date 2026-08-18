use crate::{ArmVirtProfile, QEMU_ARM_VIRT};

#[test]
fn qemu_layout_is_structurally_valid() {
    assert_eq!(QEMU_ARM_VIRT.validate(), Ok(()));
    assert_eq!(QEMU_ARM_VIRT.virtio.region().end(), 0x0A00_4000);
    assert_eq!(QEMU_ARM_VIRT.virtio.slot_base(31), Some(0x0A00_3E00));
    assert_eq!(QEMU_ARM_VIRT.virtio.spi(31), Some(47));
    assert_eq!(ArmVirtProfile::gic_id_for_spi(16), 48);
    assert_eq!(ArmVirtProfile::gic_id_for_spi(47), 79);
    assert_eq!(ArmVirtProfile::gic_id_for_spi(QEMU_ARM_VIRT.gpio.spi), 39);
}

#[test]
fn qemu_layout_preserves_existing_public_windows() {
    assert_eq!(QEMU_ARM_VIRT.uart.mmio.base, 0x0900_0000);
    assert_eq!(QEMU_ARM_VIRT.rtc.base, 0x0901_0000);
    assert_eq!(QEMU_ARM_VIRT.gpio.mmio.base, 0x0903_0000);
    assert_eq!(QEMU_ARM_VIRT.pcie_ecam_bus0.base, 0x3F00_0000);
}
