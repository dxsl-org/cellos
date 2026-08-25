//! Graphical two-owner probe for compositor window-policy integration evidence.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use api::display::{PixelFormat, WindowState};
use api::input::{InputEvent, KeyState, MouseButton};
use ostd::display::{poll_surface_events, wait_for_compositor, SurfaceEvent, ViSurface};
use ostd::io::println;
use ostd::task::yield_now;

const SIZE: u32 = 160;
const RESTORE_DELAY_TICKS: u8 = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClosePolicy {
    Never,
    RejectThenAccept,
}

struct ProbeRole {
    name: &'static str,
    title: &'static str,
    x: i32,
    y: i32,
    color: [u8; 4],
    background: bool,
    restore_after_minimize: bool,
    apply_configures: bool,
    close_policy: ClosePolicy,
}

ostd::cell_main!(cell_main);

fn cell_main() {
    let Some(role) = parse_role() else {
        println("[window-policy-probe] usage: window-policy-probe back|front|background|wm-primary|wm-silent|wm-close");
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
    if !role.background && surface.set_title(role.title).is_ok() {
        println(&alloc::format!("[window-policy-probe {}] title set", role.name));
    }
    println(&alloc::format!("[window-policy-probe {}] ready", role.name));

    let mut press_logged = false;
    let mut release_logged = false;
    let mut key_logged = false;
    let mut close_requests = 0u8;
    let mut restore_ticks = None;
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

        let mut destroy = false;
        for event in poll_surface_events(8) {
            match event {
                SurfaceEvent::Configure(configure) if configure.cap == surface.cap() => {
                    println(&alloc::format!(
                        "[window-policy-probe {}] configure {:?} serial {} {}x{}",
                        role.name, configure.kind, configure.serial, configure.rect.w, configure.rect.h
                    ));
                    let serial = configure.serial;
                    if role.apply_configures {
                        match surface.apply_configure(configure) {
                            Ok(()) => {
                                for pixel in surface.pixels_mut().chunks_exact_mut(4) {
                                    pixel.copy_from_slice(&role.color);
                                }
                                surface.damage_all();
                                println(&alloc::format!(
                                    "[window-policy-probe {}] configured serial {}",
                                    role.name, serial
                                ));
                            }
                            Err(error) => println(&alloc::format!(
                                "[window-policy-probe {}] configure failed {:?}",
                                role.name, error
                            )),
                        }
                    }
                }
                SurfaceEvent::CloseRequest(request) if request.cap == surface.cap() => {
                    close_requests = close_requests.saturating_add(1);
                    let accept = role.close_policy == ClosePolicy::RejectThenAccept
                        && close_requests >= 2;
                    if surface.respond_close(request.serial, accept).is_ok() {
                        let action = if accept { "accept" } else { "reject" };
                        println(&alloc::format!(
                            "[window-policy-probe {}] close {} serial {}",
                            role.name, action, request.serial
                        ));
                        destroy = accept;
                    }
                }
                SurfaceEvent::StateChanged(change) if change.cap == surface.cap() => {
                    println(&alloc::format!(
                        "[window-policy-probe {}] state {:?} serial {}",
                        role.name, change.state, change.serial
                    ));
                    if role.restore_after_minimize && change.state == WindowState::Minimized {
                        restore_ticks = Some(RESTORE_DELAY_TICKS);
                    }
                }
                _ => {}
            }
        }
        if destroy {
            println(&alloc::format!("[window-policy-probe {}] destroy", role.name));
            surface.destroy();
            return;
        }
        if let Some(ticks) = restore_ticks {
            if ticks == 0 {
                if surface.restore().is_ok() {
                    println(&alloc::format!("[window-policy-probe {}] restore request", role.name));
                }
                restore_ticks = None;
            } else {
                restore_ticks = Some(ticks - 1);
            }
        }
        yield_now();
    }
}

fn parse_role() -> Option<ProbeRole> {
    let role = match ostd::args().first().map(|arg| arg.as_str()) {
        Some("back") => ("back", "Back", 80, 80, [0x00, 0x00, 0xFF, 0xFF], false, false, true, ClosePolicy::Never),
        Some("front") => ("front", "Front", 160, 120, [0xFF, 0x00, 0x00, 0xFF], false, false, true, ClosePolicy::Never),
        Some("background") => ("background", "", 0, 0, [0x00, 0xFF, 0x00, 0xFF], true, false, false, ClosePolicy::Never),
        Some("wm-primary") => ("wm-primary", "Primary", 400, 100, [0xFF, 0x00, 0xFF, 0xFF], false, true, true, ClosePolicy::Never),
        Some("wm-silent") => ("wm-silent", "Silent", 400, 300, [0xFF, 0xFF, 0x00, 0xFF], false, false, false, ClosePolicy::Never),
        Some("wm-close") => ("wm-close", "Close", 600, 100, [0x00, 0xFF, 0xFF, 0xFF], false, false, true, ClosePolicy::RejectThenAccept),
        _ => return None,
    };
    Some(ProbeRole {
        name: role.0,
        title: role.1,
        x: role.2,
        y: role.3,
        color: role.4,
        background: role.5,
        restore_after_minimize: role.6,
        apply_configures: role.7,
        close_policy: role.8,
    })
}
