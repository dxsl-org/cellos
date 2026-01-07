#![no_std]

//! Input Driver Cell (Keyboard & Mouse) - INTERFACE ONLY

use ostd::prelude::*;

/// Input event types.
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPress(u32),
    KeyRelease(u32),
    MouseMove { x: i32, y: i32 },
    MouseButton { button: u8, pressed: bool },
}

/// Input driver.
pub struct InputDriver;

impl InputDriver {
    pub fn new() -> Self { todo!() }
    pub fn init(&mut self) -> Result<()> { todo!() }
    pub fn poll_event(&self) -> Option<InputEvent> { todo!() }
}
