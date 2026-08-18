use crate::milk_v_pioneer::MILK_V_PIONEER;
use crate::qemu_virt_aarch64::QEMU_VIRT_AARCH64;
use crate::qemu_virt_riscv64::QEMU_VIRT_RISCV64;
use crate::raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B;
use crate::raspberry_pi_4_model_b::RASPBERRY_PI_4_MODEL_B;
use crate::starfive_visionfive_2::STARFIVE_VISIONFIVE_2;
use crate::{Architecture, DriverId, FirmwareInterface, SocId};

#[test]
fn qemu_descriptor_matches_current_kernel_constants() {
    let board = QEMU_VIRT_RISCV64;

    assert_eq!(board.architecture, Architecture::Riscv64);
    assert_eq!(board.compatibles, &["riscv-virtio", "qemu,virt"]);
    assert_eq!(board.boot.firmware, FirmwareInterface::OpenSbi);
    assert!(!board.boot.requires_firmware_dtb);
    assert_eq!(board.fallback_memory[0].base, 0x8000_0000);
    assert_eq!(board.fallback_memory[1].base, 0x8020_0000);
    assert_eq!(board.fallback_memory[2].base, 0x8420_0000);
    assert_eq!(board.uart.base, 0x1000_0000);
    assert_eq!(board.uart.irq, Some(10));
    assert_eq!(board.plic.unwrap().base, 0x0C00_0000);
    assert_eq!(board.plic.unwrap().size, 0x0400_0000);
    assert_eq!(board.clint.unwrap().base, 0x0200_0000);
    assert_eq!(board.rtc.unwrap().base, 0x0010_1000);
    assert_eq!(board.virtio_mmio.len(), 5);
    assert_eq!(board.virtio_mmio[4].irq, Some(5));
    assert_eq!(board.validate_for(Architecture::Riscv64), Ok(()));
}

#[test]
fn rpi3_descriptor_matches_current_fallback_contract() {
    let board = RASPBERRY_PI_3_MODEL_B;

    assert_eq!(board.architecture, Architecture::Aarch64);
    assert_eq!(board.boot.firmware, FirmwareInterface::VideoCore);
    assert!(!board.boot.requires_firmware_dtb);
    assert_eq!(board.uart.base, 0x3F21_5040);
    assert_eq!(board.plic, None);
    assert_eq!(board.clint, None);
    assert_eq!(board.rtc, None);
    assert!(board.virtio_mmio.is_empty());
    assert_eq!(board.fallback_memory[0].base, 0x0008_0000);
    assert_eq!(board.fallback_memory[0].size, 0x0100_0000);
    assert_eq!(board.fallback_memory[1].base, 0x0108_0000);
    assert_eq!(board.fallback_memory[1].size, 0x3DF8_0000);
    assert_eq!(
        board.fallback_memory[1].base + board.fallback_memory[1].size,
        0x3F00_0000
    );
    assert_eq!(board.validate_for(Architecture::Aarch64), Ok(()));
}

#[test]
fn catalog_covers_every_current_board_selection() {
    let boards = [
        QEMU_VIRT_RISCV64,
        QEMU_VIRT_AARCH64,
        STARFIVE_VISIONFIVE_2,
        MILK_V_PIONEER,
        RASPBERRY_PI_3_MODEL_B,
        RASPBERRY_PI_4_MODEL_B,
    ];
    for board in boards {
        assert_eq!(board.validate(), Ok(()), "{}", board.slug);
        assert!(!board.boot.fallback_dts_path.is_empty());
    }
}

#[test]
fn soc_and_driver_selection_is_typed() {
    assert_eq!(QEMU_VIRT_AARCH64.soc, SocId::QemuArmVirt);
    assert!(QEMU_VIRT_AARCH64.has_driver(DriverId::GicV2));
    assert_eq!(QEMU_VIRT_AARCH64.virtio_mmio.len(), 32);
    assert_eq!(QEMU_VIRT_AARCH64.virtio_mmio[31].irq, Some(47));
    assert_eq!(STARFIVE_VISIONFIVE_2.soc, SocId::Jh7110);
    assert!(STARFIVE_VISIONFIVE_2.has_driver(DriverId::SdhciDwCqe));
    assert_eq!(MILK_V_PIONEER.soc, SocId::Sg2042);
    assert!(!MILK_V_PIONEER.has_driver(DriverId::VirtioMmio));
    assert_eq!(RASPBERRY_PI_4_MODEL_B.soc, SocId::Bcm2711);
    assert!(RASPBERRY_PI_4_MODEL_B.has_driver(DriverId::SdhciArasan));
}
