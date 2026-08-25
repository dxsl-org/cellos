//! Graphical two-owner probe for compositor window-policy integration evidence.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use api::display::PixelFormat;
use api::input::{InputEvent, KeyState, MouseButton};
use ostd::display::{wait_for_compositor, ViSurface};
use ostd::io::println;
use ostd::task::yield_now;

const SIZE: u32 = 160;

struct ProbeRole {
    name: &'static str,
    x: i32,
    y: i32,
    color: [u8; 4],
    background: bool,
}

ostd::cell_main!(cell_main);

fn cell_main() {
    let Some(role) = parse_role() else {
        println("[window-policy-probe] usage: window-policy-probe back|front|background");
        return;
    };
    let comp = wait_for_compositor();
    let mut surface = match if role.background {
        ViSurface::create_background(comp, SIZE, SIZE, PixelFormat::Bgra8888)
    } else {
        ViSurface::create(comp, SIZE, SIZE, PixelFormat::Bgra8888)
    } {
        Ok(surface) => surface,
        Err(_) => {
            println(&alloc::format!(
                "[window-policy-probe {}] surface create failed",
                role.name
            ));
            return;
        }
    };
    surface.move_to(role.x, role.y);
    for pixel in surface.pixels_mut().chunks_exact_mut(4) {
        pixel.copy_from_slice(&role.color);
    }
    surface.damage_all();
    println(&alloc::format!("[window-policy-probe {}] ready", role.name));

    let mut press_logged = false;
    let mut release_logged = false;
    let mut key_logged = false;
    loop {
        for event in ostd::input::poll_events(8) {
            match event {
                InputEvent::MouseMove { x, y, .. } => {
                    println(&alloc::format!(
                        "[window-policy-probe {}] move {x},{y}",
                        role.name
                    ));
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Pressed,
                } if !press_logged => {
                    press_logged = true;
                    println(&alloc::format!("[window-policy-probe {}] press", role.name));
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Released,
                } if !release_logged => {
                    release_logged = true;
                    println(&alloc::format!(
                        "[window-policy-probe {}] release",
                        role.name
                    ));
                }
                InputEvent::Key(key) if key.state == KeyState::Pressed && !key_logged => {
                    key_logged = true;
                    println(&alloc::format!("[window-policy-probe {}] key", role.name));
                }
                _ => {}
            }
        }
        yield_now();
    }
}

fn parse_role() -> Option<ProbeRole> {
    match ostd::args().first().map(|arg| arg.as_str()) {
        Some("back") => Some(ProbeRole {
            name: "back",
            x: 80,
            y: 80,
            color: [0x00, 0x00, 0xFF, 0xFF],
            background: false,
        }),
        Some("front") => Some(ProbeRole {
            name: "front",
            x: 160,
            y: 120,
            color: [0xFF, 0x00, 0x00, 0xFF],
            background: false,
        }),
        Some("background") => Some(ProbeRole {
            name: "background",
            x: 0,
            y: 0,
            color: [0x00, 0xFF, 0x00, 0xFF],
            background: true,
        }),
        _ => None,
    }
}
