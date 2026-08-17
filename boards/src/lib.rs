#![no_std]

mod descriptor;
#[cfg(test)]
mod descriptor_tests;

#[path = "../qemu/virt-riscv64/board.rs"]
pub mod qemu_virt_riscv64;

pub use descriptor::*;

pub fn qemu_virt_riscv64() -> &'static BoardDescriptor {
    &qemu_virt_riscv64::QEMU_VIRT_RISCV64
}
