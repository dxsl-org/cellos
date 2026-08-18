#![no_std]

#[cfg(test)]
mod catalog_tests;
mod descriptor;
#[cfg(test)]
mod descriptor_tests;

#[path = "../qemu/virt-riscv64/board.rs"]
pub mod qemu_virt_riscv64;

#[path = "../qemu/virt-aarch64/board.rs"]
pub mod qemu_virt_aarch64;

#[path = "../starfive/visionfive-2/board.rs"]
pub mod starfive_visionfive_2;

#[path = "../milk-v/pioneer/board.rs"]
pub mod milk_v_pioneer;

#[path = "../raspberry-pi/3-model-b/board.rs"]
pub mod raspberry_pi_3_model_b;

#[path = "../raspberry-pi/4-model-b/board.rs"]
pub mod raspberry_pi_4_model_b;

pub use descriptor::*;

pub fn qemu_virt_riscv64() -> &'static BoardDescriptor {
    &qemu_virt_riscv64::QEMU_VIRT_RISCV64
}

pub fn qemu_virt_aarch64() -> &'static BoardDescriptor {
    &qemu_virt_aarch64::QEMU_VIRT_AARCH64
}

pub fn starfive_visionfive_2() -> &'static BoardDescriptor {
    &starfive_visionfive_2::STARFIVE_VISIONFIVE_2
}

pub fn milk_v_pioneer() -> &'static BoardDescriptor {
    &milk_v_pioneer::MILK_V_PIONEER
}

/// Returns the audited Raspberry Pi 3 Model B board descriptor.
pub fn raspberry_pi_3_model_b() -> &'static BoardDescriptor {
    &raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B
}

pub fn raspberry_pi_4_model_b() -> &'static BoardDescriptor {
    &raspberry_pi_4_model_b::RASPBERRY_PI_4_MODEL_B
}
