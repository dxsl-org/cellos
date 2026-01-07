#![no_std]

//! GPU Driver Cell
//! INTERFACE ONLY - NO IMPLEMENTATION

use ostd::prelude::*;

/// GPU driver (stub).
pub struct GpuDriver;

impl GpuDriver {
    pub fn new() -> Self {
        todo!("Implementation phase")
    }
    
    pub fn init(&mut self) -> Result<()> {
        todo!("Implementation phase")
    }
    
    pub fn get_framebuffer(&self) -> &mut [u8] {
        todo!("Implementation phase")
    }
}
