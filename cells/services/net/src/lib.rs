#![no_std]
#![forbid(unsafe_code)]

//! TCP/IP Stack Service Cell - INTERFACE ONLY

use api::net::*;
use ostd::prelude::*;

pub struct NetStack;

impl ViTcpStack for NetStack {
    fn connect(&self, _addr: IpEndpoint) -> Result<Box<dyn ViTcpStream>> {
        todo!()
    }
    fn listen(&self, _port: u16) -> Result<Box<dyn ViTcpListener>> {
        todo!()
    }
}
