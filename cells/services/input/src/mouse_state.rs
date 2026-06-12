use api::input::{InputEvent, MouseButton};

// EV_REL axis codes
const REL_X: u32 = 0;
const REL_Y: u32 = 1;
const REL_WHEEL: u32 = 8;

// EV_ABS axis codes
const ABS_X: u32 = 0;
const ABS_Y: u32 = 1;

// Mouse button EV_KEY codes (BTN_*)
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// Tracks cumulative mouse position and maps raw VirtIO events to `InputEvent`s.
pub struct MouseState {
    x: i32,
    y: i32,
}

impl MouseState {
    pub fn new() -> Self {
        Self { x: 0, y: 0 }
    }

    /// Handle an EV_REL event. Returns the `InputEvent` to dispatch, if any.
    pub fn apply_rel(&mut self, code: u32, raw_value: u32) -> Option<InputEvent> {
        let value = raw_value as i32;
        match code {
            REL_X => {
                self.x = self.x.saturating_add(value);
                Some(InputEvent::MouseMove { x: self.x, y: self.y, dx: value, dy: 0 })
            }
            REL_Y => {
                self.y = self.y.saturating_add(value);
                Some(InputEvent::MouseMove { x: self.x, y: self.y, dx: 0, dy: value })
            }
            REL_WHEEL => Some(InputEvent::MouseScroll { dx: 0, dy: value }),
            _ => None,
        }
    }

    /// Handle an EV_ABS event. Returns the `InputEvent` to dispatch, if any.
    pub fn apply_abs(&mut self, code: u32, raw_value: u32) -> Option<InputEvent> {
        let value = raw_value as i32;
        match code {
            ABS_X => {
                let dx = value.wrapping_sub(self.x);
                self.x = value;
                Some(InputEvent::MouseMove { x: self.x, y: self.y, dx, dy: 0 })
            }
            ABS_Y => {
                let dy = value.wrapping_sub(self.y);
                self.y = value;
                Some(InputEvent::MouseMove { x: self.x, y: self.y, dx: 0, dy })
            }
            _ => None,
        }
    }
}

/// Map a BTN_* scancode to a `MouseButton`, or `None` if not a recognised button.
pub fn btn_to_mouse_button(code: u32) -> Option<MouseButton> {
    match code {
        BTN_LEFT   => Some(MouseButton::Left),
        BTN_RIGHT  => Some(MouseButton::Right),
        BTN_MIDDLE => Some(MouseButton::Middle),
        _          => None,
    }
}
