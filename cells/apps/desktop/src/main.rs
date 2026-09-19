// SPDX-License-Identifier: MIT
//! CellOS Desktop Cell — Taskbar, Spotlight Search, and Application Launcher.
//!
//! Conforms to Cell Trust Tier 1 (Pure Rust, SAS, `#![forbid(unsafe_code)]`).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

mod apps;
mod draw;
mod spotlight;
mod taskbar;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{
    sys_get_resolution, sys_get_time, sys_spawn_from_path, sys_yield, SyscallResult,
};

use crate::apps::AppRegistry;
use crate::spotlight::{SpotlightAction, SpotlightState};
use crate::taskbar::{TaskbarAction, TaskbarState};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Log,
    SpawnFromPath,
    GrantAlloc,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    Send,
    Recv,
    TryRecv,
    LookupService,
    GpuGetResolution,
    GetTime
];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[desktop] CellOS Desktop initializing...");

    // 1. Connect to Display Compositor
    let comp_tid = ostd::display::wait_for_compositor();

    // 2. Query Display Resolution
    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }
    ostd::io::println("[desktop] Display resolution confirmed.");

    // 3. Create Taskbar Surface
    let tb_h = taskbar::TASKBAR_HEIGHT;
    let tb_w = screen_w;
    let mut tb_surf = ViSurface::create(comp_tid, tb_w, tb_h, PixelFormat::Bgra8888)
        .expect("create taskbar surface");
    let _ = tb_surf.set_title("CellOS Taskbar");
    let tb_y = screen_h as i32 - tb_h as i32;
    tb_surf.move_to(0, tb_y);

    // 4. Create Spotlight Search Surface (Centered)
    let sp_w = spotlight::SPOTLIGHT_WIDTH;
    let sp_h = spotlight::SPOTLIGHT_HEIGHT;
    let mut sp_surf = ViSurface::create(comp_tid, sp_w, sp_h, PixelFormat::Bgra8888)
        .expect("create spotlight surface");
    let _ = sp_surf.set_title("Spotlight Search");
    let sp_x = (screen_w as i32 - sp_w as i32) / 2;
    let sp_y = 100i32;
    // Keep offscreen until invoked
    sp_surf.move_to(-2000, -2000);

    // 5. Initialize States
    let mut registry = AppRegistry::new();
    let mut tb_state = TaskbarState::new();
    let mut sp_state = SpotlightState::new();

    // 6. Request Input Focus
    while !ostd::input::request_focus() {
        sys_yield();
    }

    // 7. Initial Paint
    tb_state.update_daemons();
    taskbar::render(&mut tb_surf, &registry, &tb_state);

    let mut last_tick = sys_get_time();
    let mut cursor_x = 0i32;
    let mut cursor_y = 0i32;

    ostd::io::println("[desktop] Taskbar surface ready.");
    ostd::io::println("[desktop] Ready. Entering event loop.");

    loop {
        let mut need_tb_render = false;
        let mut need_sp_render = false;

        // Drain pending user inputs
        for ev in ostd::input::poll_events(16) {
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    cursor_x = x;
                    cursor_y = y;

                    if sp_state.is_open {
                        if x >= 0 && x < sp_w as i32 && y >= 0 && y < sp_h as i32 {
                            sp_state.mouse_x = x;
                            sp_state.mouse_y = y;
                            need_sp_render = true;
                        } else {
                            sp_state.mouse_x = -1;
                            sp_state.mouse_y = -1;
                        }
                    } else if x >= 0 && x < tb_w as i32 && y >= 0 && y < tb_h as i32 {
                        tb_state.mouse_x = x;
                        tb_state.mouse_y = y;
                        need_tb_render = true;
                    } else if tb_state.mouse_x != -1 {
                        tb_state.mouse_x = -1;
                        tb_state.mouse_y = -1;
                        need_tb_render = true;
                    }
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Pressed,
                } => {
                    if sp_state.is_open {
                        if cursor_x >= 0
                            && cursor_x < sp_w as i32
                            && cursor_y >= 0
                            && cursor_y < sp_h as i32
                        {
                            match spotlight::handle_click(
                                &mut registry,
                                &mut sp_state,
                                cursor_x,
                                cursor_y,
                                sp_w,
                            ) {
                                SpotlightAction::LaunchApp(path) => {
                                    ostd::io::print("[desktop] Spotlight: Launching ");
                                    ostd::io::println(path);
                                    launch_application(path);
                                    sp_state.close();
                                    sp_surf.move_to(-2000, -2000);
                                    need_tb_render = true;
                                }
                                SpotlightAction::TogglePin(_) => {
                                    ostd::io::println("[desktop] Spotlight: Toggled pin");
                                    need_sp_render = true;
                                    need_tb_render = true;
                                }
                                SpotlightAction::Close => {
                                    ostd::io::println("[desktop] Spotlight: Closed");
                                    sp_state.close();
                                    sp_surf.move_to(-2000, -2000);
                                }
                                SpotlightAction::None => {
                                    need_sp_render = true;
                                }
                            }
                        } else {
                            // Clicked outside spotlight window -> dismiss
                            sp_state.close();
                            sp_surf.move_to(-2000, -2000);
                        }
                    } else {
                        // Check Taskbar clicks
                        if cursor_x >= 0
                            && cursor_x < tb_w as i32
                            && cursor_y >= 0
                            && cursor_y < tb_h as i32
                        {
                            match taskbar::handle_click(
                                &mut registry,
                                &mut tb_state,
                                cursor_x,
                                cursor_y,
                            ) {
                                TaskbarAction::OpenSpotlight => {
                                    ostd::io::println(
                                        "[desktop] Taskbar: Opening Spotlight Search",
                                    );
                                    sp_state.open();
                                    sp_surf.move_to(sp_x, sp_y);
                                    sp_surf.raise();
                                    need_sp_render = true;
                                }
                                TaskbarAction::LaunchApp(path) => {
                                    ostd::io::print("[desktop] Taskbar: Launching ");
                                    ostd::io::println(path);
                                    launch_application(path);
                                    need_tb_render = true;
                                }
                                TaskbarAction::ToggleSubTaskbar => {
                                    need_tb_render = true;
                                }
                                TaskbarAction::None => {
                                    need_tb_render = true;
                                }
                            }
                        }
                    }
                }
                InputEvent::Key(ke) if ke.state == KeyState::Pressed => {
                    if sp_state.is_open {
                        match spotlight::handle_key(&registry, &mut sp_state, ke.keysym, ke.char())
                        {
                            SpotlightAction::LaunchApp(path) => {
                                launch_application(path);
                                sp_state.close();
                                sp_surf.move_to(-2000, -2000);
                                need_tb_render = true;
                            }
                            SpotlightAction::Close => {
                                ostd::io::println("[desktop] Spotlight: Closed");
                                sp_state.close();
                                sp_surf.move_to(-2000, -2000);
                            }
                            _ => {
                                need_sp_render = true;
                            }
                        }
                    } else {
                        // Global shortcut: Space with Ctrl, or Super / Meta -> Open Spotlight
                        if ke.keysym == KeySym::F1
                            || (ke.char() == Some(' ') && ke.modifiers.contains(Modifiers::CTRL))
                        {
                            sp_state.open();
                            sp_surf.move_to(sp_x, sp_y);
                            sp_surf.raise();
                            need_sp_render = true;
                        }
                    }
                }
                _ => {}
            }
        }

        // Periodic timer updates (every 500 ms)
        let now = sys_get_time();
        if now.saturating_sub(last_tick) > 5_000_000 {
            last_tick = now;
            tb_state.update_daemons();
            need_tb_render = true;
        }

        // Re-paint damaged components
        if need_tb_render {
            taskbar::render(&mut tb_surf, &registry, &tb_state);
        }
        if need_sp_render && sp_state.is_open {
            spotlight::render(&mut sp_surf, &registry, &sp_state);
        }

        sys_yield();
    }
}

fn launch_application(path: &'static str) {
    ostd::io::print("[desktop] Launching application: ");
    ostd::io::println(path);
    match sys_spawn_from_path(path) {
        SyscallResult::Ok(tid) => {
            ostd::io::print("[desktop] Spawned PID ");
            let mut buf = [0u8; 16];
            let mut n = tid;
            let mut len = 0;
            if n == 0 {
                buf[0] = b'0';
                len = 1;
            } else {
                while n > 0 {
                    buf[len] = b'0' + (n % 10) as u8;
                    n /= 10;
                    len += 1;
                }
                buf[..len].reverse();
            }
            if let Ok(s) = core::str::from_utf8(&buf[..len]) {
                ostd::io::println(s);
            } else {
                ostd::io::println("OK");
            }
        }
        SyscallResult::Err(_) => {
            ostd::io::println("[desktop] ERROR: Application spawn rejected by kernel.");
        }
    }
}
