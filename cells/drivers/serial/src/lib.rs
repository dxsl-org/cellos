#![no_std]

//! Serial/UART Driver Cell - INTERFACE ONLY

use ostd::prelude::*;

/// Serial driver for UART console.
pub struct SerialDriver;

impl SerialDriver {
    pub fn new() -> Self { todo!() }
    pub fn init(&mut self) -> Result<()> { todo!() }
    pub fn write(&self, _data: &[u8]) -> Result<usize> { todo!() }
    pub fn read(&self, _buf: &mut [u8]) -> Result<usize> { todo!() }
}
