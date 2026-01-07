#![no_std]

//! VFS Manager Service Cell - INTERFACE ONLY

use ostd::prelude::*;
use api::fs::*;

pub struct VfsManager;

impl VfsManager {
    pub fn mount(&mut self, _path: &str, _fs: Box<dyn ViFileSystem>) -> Result<()> { todo!() }
    pub fn unmount(&mut self, _path: &str) -> Result<()> { todo!() }
}
