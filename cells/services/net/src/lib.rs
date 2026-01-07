#![no_std]

//! TCP/IP Stack Service Cell - INTERFACE ONLY

use ostd::prelude::*;
use api::net::*;

pub struct NetStack;

impl ViTcpStack for NetStack {
    fn connect(&self, _addr: IpEndpoint) -> Result<Box<dyn ViTcpStream>> { todo!() }
    fn listen(&self, _port: u16) -> Result<Box<dyn ViTcpListener>> { todo!() }
}
