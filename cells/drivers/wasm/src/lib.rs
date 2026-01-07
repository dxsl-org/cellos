#![no_std]

//! WASM Driver Runtime Cell - INTERFACE ONLY
//! 
//! Sandboxes legacy C drivers compiled to WASM.

use ostd::prelude::*;

/// WASM driver runtime.
pub struct WasmRuntime;

impl WasmRuntime {
    pub fn new() -> Self { todo!() }
    pub fn load_module(&mut self, _wasm_bytes: &[u8]) -> Result<()> { todo!() }
    pub fn call_function(&self, _name: &str, _args: &[u8]) -> Result<Vec<u8>> { todo!() }
}

extern crate alloc;
use alloc::vec::Vec;
