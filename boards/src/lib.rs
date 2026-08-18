#![no_std]

#[cfg(test)]
mod catalog_tests;
mod descriptor;
#[cfg(test)]
mod descriptor_tests;

#[path = "../qemu/virt-riscv64/board.rs"]
pub mod qemu_virt_riscv64;

#[path = "../raspberry-pi/3-model-b/board.rs"]
pub mod raspberry_pi_3_model_b;

pub use descriptor::*;

pub fn qemu_virt_riscv64() -> &'static BoardDescriptor {
    &qemu_virt_riscv64::QEMU_VIRT_RISCV64
}

/// Returns the audited Raspberry Pi 3 Model B board descriptor.
pub fn raspberry_pi_3_model_b() -> &'static BoardDescriptor {
    &raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B
}
