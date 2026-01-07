#![no_std]

//! Input Dispatcher Service Cell - INTERFACE ONLY
//! 
//! Routes input events to focused application.

use ostd::prelude::*;

/// Input event (re-export from driver-input concept).
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPress(u32),
    KeyRelease(u32),
    MouseMove { x: i32, y: i32 },
    MouseButton { button: u8, pressed: bool },
}

/// Input dispatcher.
pub struct InputDispatcher;

impl InputDispatcher {
    pub fn new() -> Self { todo!() }
    pub fn dispatch(&self, _event: InputEvent) -> Result<()> { todo!() }
    pub fn set_focus(&mut self, _app_id: u64) -> Result<()> { todo!() }
}
