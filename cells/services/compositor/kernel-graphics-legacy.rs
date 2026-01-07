//! Graphics and input interfaces.

use crate::*;

/// Compositor interface.
pub trait Compositor: Send + Sync {
    /// Present a surface to the screen.
    fn present(&self, surface: &Surface) -> Result<()>;
    
    /// Get the framebuffer (for direct access).
    fn framebuffer(&self) -> &mut [u8];
}

/// Surface (application render target).
pub struct Surface {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data.
    pub data: alloc::vec::Vec<u8>,
}

/// Input event dispatcher interface.
pub trait InputDispatcher: Send + Sync {
    /// Dispatch an input event to the focused window.
    fn dispatch(&self, event: InputEvent);
}

/// Input event types.
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPress(KeyCode),
    KeyRelease(KeyCode),
    MouseMove { x: i32, y: i32 },
    MouseButton { button: u8, pressed: bool },
}

/// Keyboard key codes (placeholder).
pub type KeyCode = u32;

extern crate alloc;
