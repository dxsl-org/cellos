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

#[path = "../qemu/q35-x86_64/board.rs"]
pub mod qemu_q35_x86_64;

/// Generic x86_64 PC/server descriptor (no machine-specific facts).
#[path = "../pc/x86_64-pc/board.rs"]
pub mod pc_x86_64;

/// Apple Mac mini M1 (t8103, J274) descriptor (ADR-0032 §4–5).
#[path = "../apple/macmini-m1/board.rs"]
pub mod apple_macmini_m1;
/// Generic AArch64 virtual machine descriptor (ADR-0032 §5).
#[path = "../vm/aarch64-vm/board.rs"]
pub mod vm_aarch64;
/// Generic x86_64 virtual machine descriptor (ADR-0031 D-VM).
#[path = "../vm/x86_64-vm/board.rs"]
pub mod vm_x86_64;

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

pub fn qemu_q35_x86_64() -> &'static BoardDescriptor {
    &qemu_q35_x86_64::QEMU_Q35_X86_64
}

/// Returns the generic x86_64 PC/server descriptor.
pub fn pc_x86_64() -> &'static BoardDescriptor {
    &pc_x86_64::X86_64_PC
}
pub fn vm_aarch64() -> &'static BoardDescriptor {
    &vm_aarch64::AARCH64_VM
}
pub fn macmini_m1() -> &'static BoardDescriptor {
    &apple_macmini_m1::MACMINI_M1
}

/// Every x86_64 descriptor one x86_64 image may bind at boot, strictest first.
///
/// Membership is the whole approval: a slug absent here cannot be bound, and
/// nothing detected at boot adds an entry. The order carries no selection
/// meaning beyond documentation; which entry applies is decided by the caller.
static X86_CATALOG: [&BoardDescriptor; 3] = [
    &pc_x86_64::X86_64_PC,
    &qemu_q35_x86_64::QEMU_Q35_X86_64,
    &vm_x86_64::X86_64_VM,
];

/// Returns the compiled x86_64 catalog.
pub fn x86_catalog() -> &'static [&'static BoardDescriptor] {
    &X86_CATALOG
}
static AARCH64_CATALOG: [&BoardDescriptor; 5] = [
    &qemu_virt_aarch64::QEMU_VIRT_AARCH64,
    &raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B,
    &raspberry_pi_4_model_b::RASPBERRY_PI_4_MODEL_B,
    &vm_aarch64::AARCH64_VM,
    &apple_macmini_m1::MACMINI_M1,
];

/// Returns the compiled AArch64 catalog.
pub fn aarch64_catalog() -> &'static [&'static BoardDescriptor] {
    &AARCH64_CATALOG
}

/// Returns the descriptor in `catalog` whose slug equals `slug` byte for byte.
///
/// The search is confined to the catalog the caller names, so a slug from
/// another architecture's board list can never be found through it.
pub fn find_by_slug(
    catalog: &[&'static BoardDescriptor],
    slug: &str,
) -> Option<&'static BoardDescriptor> {
    catalog.iter().copied().find(|board| board.slug == slug)
}
