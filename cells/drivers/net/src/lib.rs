#![no_std]

//! Network Driver Cell - INTERFACE ONLY

use ostd::prelude::*;

pub struct NetDriver;

impl NetDriver {
    pub fn new() -> Self { todo!() }
    pub fn init(&mut self) -> Result<()> { todo!() }
    pub fn send_packet(&self, _data: &[u8]) -> Result<()> { todo!() }
    pub fn recv_packet(&self, _buf: &mut [u8]) -> Result<usize> { todo!() }
}
