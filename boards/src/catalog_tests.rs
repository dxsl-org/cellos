use crate::qemu_virt_riscv64::QEMU_VIRT_RISCV64;
use crate::raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B;
use crate::{Architecture, FirmwareInterface};

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
