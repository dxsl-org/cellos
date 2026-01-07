#![no_std]

pub mod common;
pub mod rv64;
// pub mod rv32; // TODO: Implement 32-bit support

// Export architecture specific modules based on target
#[cfg(target_arch = "riscv64")]
pub use rv64::*;



pub mod rv32;

#[cfg(target_arch = "riscv32")]
pub use rv32::*;
