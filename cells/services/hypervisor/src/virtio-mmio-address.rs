//! Architecture-specific VirtIO-MMIO guest address window.

#[cfg(target_arch = "aarch64")]
pub const BASE: u64 = 0x0a00_0000;
#[cfg(target_arch = "x86_64")]
pub const BASE: u64 = 0xd000_0000;
pub const STRIDE: u64 = 0x200;
const SLOTS: u64 = 32;

pub fn owns(address: u64) -> bool {
    (BASE..BASE + SLOTS * STRIDE).contains(&address)
}

pub fn slot_and_offset(address: u64) -> (usize, u64) {
    let relative = address - BASE;
    ((relative / STRIDE) as usize, relative % STRIDE)
}
