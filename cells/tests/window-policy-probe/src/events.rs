use api::display::WindowState;
use api::input::{InputEvent, KeyState, MouseButton};
use ostd::display::{poll_surface_events, SurfaceEvent, ViSurface};
use ostd::io::println;

use crate::roles::{ClosePolicy, ProbeRole};

const RESTORE_DELAY_TICKS: u8 = 50;

pub(crate) struct EventHandler {
    press_logged: bool,
    release_logged: bool,
    key_logged: bool,
    close_requests: u8,
    restore_ticks: Option<u8>,
    reject_first_attach: bool,
}

impl EventHandler {
    pub(crate) fn new(role: &ProbeRole) -> Self {
        Self {
            press_logged: false,
            release_logged: false,
            key_logged: false,
            close_requests: 0,
            restore_ticks: None,
            reject_first_attach: role.name == "wm-primary",
        }
    }

    pub(crate) fn process(&mut self, role: &ProbeRole, surface: &mut ViSurface) -> bool {
        self.handle_input(role);

        let mut destroy = false;
        for event in poll_surface_events(8) {
            match event {
                SurfaceEvent::Configure(configure) if configure.cap == surface.cap() => {
                    println(&alloc::format!(
                        "[window-policy-probe {}] configure {:?} serial {} {}x{}",
                        role.name,
                        configure.kind,
                        configure.serial,
                        configure.rect.w,
                        configure.rect.h
                    ));
                    let serial = configure.serial;
                    if role.apply_configures {
                        if self.reject_first_attach {
                            let mut rejected = configure;
                            rejected.rect.w = rejected.rect.w.saturating_add(1);
                            if surface.apply_configure(rejected).is_err() {
                                println(&alloc::format!(
                                    "[window-policy-probe {}] attach rejected serial {}",
                                    role.name,
                                    serial
                                ));
                            }
                            self.reject_first_attach = false;
                        }
                        match surface.apply_configure(configure) {
                            Ok(()) => {
                                for pixel in surface.pixels_mut().chunks_exact_mut(4) {
                                    pixel.copy_from_slice(&role.color);
                                }
                                surface.damage_all();
                                println(&alloc::format!(
                                    "[window-policy-probe {}] configured serial {}",
                                    role.name,
                                    serial
                                ));
                            }
                            Err(error) => println(&alloc::format!(
                                "[window-policy-probe {}] configure failed {:?}",
                                role.name,
                                error
                            )),
                        }
                    }
                }
                SurfaceEvent::CloseRequest(request) if request.cap == surface.cap() => {
                    self.close_requests = self.close_requests.saturating_add(1);
                    let accept = role.close_policy == ClosePolicy::RejectThenAccept
                        && self.close_requests >= 2;
                    if surface.respond_close(request.serial, accept).is_ok() {
                        let action = if accept { "accept" } else { "reject" };
                        println(&alloc::format!(
                            "[window-policy-probe {}] close {} serial {}",
                            role.name,
                            action,
                            request.serial
                        ));
                        destroy = accept;
                    }
                }
                SurfaceEvent::StateChanged(change) if change.cap == surface.cap() => {
                    println(&alloc::format!(
                        "[window-policy-probe {}] state {:?} serial {}",
                        role.name,
                        change.state,
                        change.serial
                    ));
                    if role.restore_after_minimize && change.state == WindowState::Minimized {
                        self.restore_ticks = Some(RESTORE_DELAY_TICKS);
                    }
                }
                _ => {}
            }
        }

        if !destroy {
            self.restore_if_due(role, surface);
        }
        destroy
    }

    fn handle_input(&mut self, role: &ProbeRole) {
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
                } if !self.press_logged => {
                    self.press_logged = true;
                    println(&alloc::format!("[window-policy-probe {}] press", role.name));
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Released,
                } if !self.release_logged => {
                    self.release_logged = true;
                    println(&alloc::format!(
                        "[window-policy-probe {}] release",
                        role.name
                    ));
                }
                InputEvent::Key(key) if key.state == KeyState::Pressed && !self.key_logged => {
                    self.key_logged = true;
                    println(&alloc::format!("[window-policy-probe {}] key", role.name));
                }
                _ => {}
            }
        }
    }

    fn restore_if_due(&mut self, role: &ProbeRole, surface: &mut ViSurface) {
        if let Some(ticks) = self.restore_ticks {
            if ticks == 0 {
                if surface.restore().is_ok() {
                    println(&alloc::format!(
                        "[window-policy-probe {}] restore request",
                        role.name
                    ));
                }
                self.restore_ticks = None;
            } else {
                self.restore_ticks = Some(ticks - 1);
            }
        }
    }
}
