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

/// Tracks screen-pixel position. EV_ABS producers must scale device units once,
/// before forwarding them here; service-input never rescales pixel coordinates.
pub struct MouseState {
    x: i32,
    y: i32,
    max_x: i32,
    max_y: i32,
}

impl MouseState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            max_x: width.saturating_sub(1).min(i32::MAX as u32) as i32,
            max_y: height.saturating_sub(1).min(i32::MAX as u32) as i32,
        }
    }

    /// Handle an EV_REL event. Returns the `InputEvent` to dispatch, if any.
    pub fn apply_rel(&mut self, code: u32, raw_value: u32) -> Option<InputEvent> {
        let value = raw_value as i32;
        match code {
            REL_X => {
                let old = self.x;
                self.x = self.x.saturating_add(value).clamp(0, self.max_x);
                Some(InputEvent::MouseMove {
                    x: self.x,
                    y: self.y,
                    dx: self.x - old,
                    dy: 0,
                })
            }
            REL_Y => {
                let old = self.y;
                self.y = self.y.saturating_add(value).clamp(0, self.max_y);
                Some(InputEvent::MouseMove {
                    x: self.x,
                    y: self.y,
                    dx: 0,
                    dy: self.y - old,
                })
            }
            REL_WHEEL => Some(InputEvent::MouseScroll { dx: 0, dy: value }),
            _ => None,
        }
    }

    /// Handle EV_ABS guest-pixel coordinates (not normalized device units).
    pub fn apply_abs(&mut self, code: u32, raw_value: u32) -> Option<InputEvent> {
        let value = raw_value.min(i32::MAX as u32) as i32;
        match code {
            ABS_X => {
                let value = value.clamp(0, self.max_x);
                let dx = value - self.x;
                self.x = value;
                Some(InputEvent::MouseMove {
                    x: self.x,
                    y: self.y,
                    dx,
                    dy: 0,
                })
            }
            ABS_Y => {
                let value = value.clamp(0, self.max_y);
                let dy = value - self.y;
                self.y = value;
                Some(InputEvent::MouseMove {
                    x: self.x,
                    y: self.y,
                    dx: 0,
                    dy,
                })
            }
            _ => None,
        }
    }
}

/// Map a BTN_* scancode to a `MouseButton`, or `None` if not a recognised button.
pub fn btn_to_mouse_button(code: u32) -> Option<MouseButton> {
    match code {
        BTN_LEFT => Some(MouseButton::Left),
        BTN_RIGHT => Some(MouseButton::Right),
        BTN_MIDDLE => Some(MouseButton::Middle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(event: Option<InputEvent>) -> (i32, i32, i32, i32) {
        match event {
            Some(InputEvent::MouseMove { x, y, dx, dy }) => (x, y, dx, dy),
            _ => panic!("expected pointer movement"),
        }
    }

    #[test]
    fn relative_edges_do_not_accumulate_offscreen_debt() {
        let mut mouse = MouseState::new(1920, 1080);
        assert_eq!(position(mouse.apply_rel(REL_X, (-1200i32) as u32)), (0, 0, 0, 0));
        assert_eq!(position(mouse.apply_rel(REL_X, 10)), (10, 0, 10, 0));
        assert_eq!(position(mouse.apply_rel(REL_X, i32::MAX as u32)), (1919, 0, 1909, 0));
        assert_eq!(position(mouse.apply_rel(REL_X, (-1i32) as u32)), (1918, 0, -1, 0));
        assert_eq!(position(mouse.apply_rel(REL_Y, 2000)), (1918, 1079, 0, 1079));
        assert_eq!(position(mouse.apply_rel(REL_Y, (-1i32) as u32)), (1918, 1078, 0, -1));
    }

    #[test]
    fn absolute_coordinates_are_pixels_not_scaled_again() {
        let mut mouse = MouseState::new(1280, 800);
        assert_eq!(position(mouse.apply_abs(ABS_X, 295)), (295, 0, 295, 0));
        assert_eq!(position(mouse.apply_abs(ABS_Y, 119)), (295, 119, 0, 119));
        assert_eq!(position(mouse.apply_abs(ABS_X, 1279)), (1279, 119, 984, 0));
        assert_eq!(position(mouse.apply_abs(ABS_Y, 799)), (1279, 799, 0, 680));
        assert_eq!(position(mouse.apply_abs(ABS_X, u32::MAX)), (1279, 799, 0, 0));
    }

    #[test]
    fn wheel_remains_signed_and_zero_extent_is_safe() {
        let mut mouse = MouseState::new(0, 0);
        assert_eq!(position(mouse.apply_rel(REL_X, 500)), (0, 0, 0, 0));
        assert!(matches!(
            mouse.apply_rel(REL_WHEEL, (-2i32) as u32),
            Some(InputEvent::MouseScroll { dx: 0, dy: -2 })
        ));
    }
}
