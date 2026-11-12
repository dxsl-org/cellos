// SPDX-License-Identifier: MIT
//! CellOS Desktop Cell — Taskbar, Spotlight Search, and Application Launcher.
//!
//! Conforms to Cell Trust Tier 1 (Pure Rust, SAS, `#![forbid(unsafe_code)]`).

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

mod apps;
mod draw;
mod spotlight;
mod taskbar;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{
    sys_get_resolution, sys_get_time, sys_spawn_from_path, sys_spawn_from_path_raw, sys_yield,
    SyscallResult,
};
use ostd::typography::TextFonts;
use viui::theme::{ThemePreferenceState, ViTheme};

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
    SendGather,
    Recv,
    TryRecv,
    LookupServiceBound,
    GpuGetResolution,
    GetTime,
    // Stages the LaunchIntent target as the handler's argv.
    StateStash
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

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
    let mut tb_surf = ViSurface::create_panel(comp_tid, tb_w, tb_h, PixelFormat::Bgra8888)
        .expect("create taskbar surface");
    let _ = tb_surf.set_title("CellOS Taskbar");
    let tb_y = screen_h as i32 - tb_h as i32;
    tb_surf.move_to(0, tb_y);

    // The desktop owns the system appearance; the compositor paints its chrome to match.
    let mut theme_pref = ThemePreferenceState::default();
    let mut theme = theme_pref.create_theme();
    announce_appearance(&tb_surf, theme_pref);

    // 4. Create Spotlight Search Surface (Centered)
    let sp_w = spotlight::SPOTLIGHT_WIDTH;
    let sp_h = spotlight::SPOTLIGHT_HEIGHT;
    let mut sp_surf = ViSurface::create_panel(comp_tid, sp_w, sp_h, PixelFormat::Bgra8888)
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
    let mut fonts = TextFonts::new();

    // Focus is initially left to the shell so terminal / serial prompt works at boot.
    // Spotlight requests focus when opened and releases it when closed.

    // 7. Initial Paint
    tb_state.update_daemons();
    taskbar::render(&mut tb_surf, &mut fonts, &registry, &tb_state, &*theme);

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
                                SpotlightAction::LaunchIntent(intent) => {
                                    launch_intent(&intent);
                                    sp_state.close();
                                    sp_surf.move_to(-2000, -2000);
                                    need_tb_render = true;
                                }
                                SpotlightAction::SwitchTask(_) => {
                                    ostd::io::println(
                                        "[desktop] Spotlight: Switching focus to task",
                                    );
                                    sp_state.close();
                                    sp_surf.move_to(-2000, -2000);
                                    need_tb_render = true;
                                }
                                SpotlightAction::CycleTheme => {
                                    theme = cycle_theme(&tb_surf, &mut theme_pref);
                                    need_tb_render = true;
                                    need_sp_render = true;
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
                                tb_w,
                            ) {
                                TaskbarAction::OpenSpotlight => {
                                    ostd::io::println(
                                        "[desktop] Taskbar: Opening Spotlight Search",
                                    );
                                    sp_state.open();
                                    let _ = ostd::input::request_focus();
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
                InputEvent::Key(ke)
                    if matches!(ke.state, KeyState::Pressed | KeyState::Repeated) =>
                {
                    let is_toggle_shortcut = ke.keysym == KeySym::F1
                        || (ke.char() == Some(' ')
                            && (ke.modifiers.contains(Modifiers::ALT)
                                || ke.modifiers.contains(Modifiers::CTRL)
                                || ke.modifiers.contains(Modifiers::META)));

                    if is_toggle_shortcut {
                        if ke.state != KeyState::Pressed {
                            continue;
                        }
                        if sp_state.is_open {
                            sp_state.close();
                            sp_surf.move_to(-2000, -2000);
                            need_tb_render = true;
                        } else {
                            sp_state.open();
                            let _ = ostd::input::request_focus();
                            sp_surf.move_to(sp_x, sp_y);
                            sp_surf.raise();
                            need_sp_render = true;
                        }
                    } else if sp_state.is_open {
                        // Launching, theme cycling and section changes remain one-shot.
                        if ke.state == KeyState::Repeated
                            && matches!(ke.keysym, KeySym::Return | KeySym::Escape | KeySym::Tab)
                        {
                            continue;
                        }
                        match spotlight::handle_key(&registry, &mut sp_state, ke.keysym, ke.char())
                        {
                            SpotlightAction::LaunchApp(path) => {
                                launch_application(path);
                                sp_state.close();
                                sp_surf.move_to(-2000, -2000);
                                need_tb_render = true;
                            }
                            SpotlightAction::LaunchIntent(intent) => {
                                launch_intent(&intent);
                                sp_state.close();
                                sp_surf.move_to(-2000, -2000);
                                need_tb_render = true;
                            }
                            SpotlightAction::SwitchTask(_) => {
                                ostd::io::println("[desktop] Spotlight: Switching focus to task");
                                sp_state.close();
                                sp_surf.move_to(-2000, -2000);
                                need_tb_render = true;
                            }
                            SpotlightAction::CycleTheme => {
                                theme = cycle_theme(&tb_surf, &mut theme_pref);
                                need_tb_render = true;
                                need_sp_render = true;
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
            taskbar::render(&mut tb_surf, &mut fonts, &registry, &tb_state, &*theme);
        }
        if need_sp_render && sp_state.is_open {
            spotlight::render(
                &mut sp_surf,
                &mut fonts,
                &registry,
                &sp_state,
                &*theme,
                theme_pref.palette,
            );
        }

        sys_yield();
    }
}

/// Advance the system palette (Light -> Dark -> Paper -> Light) under a strictly greater
/// revision. Returns whether the preference changed.
fn advance_theme(pref: &mut ThemePreferenceState) -> bool {
    pref.apply_update(pref.palette.next(), pref.revision.saturating_add(1))
}

/// Spotlight's Appearance action: advance the palette, tell the compositor and hand back the
/// theme the desktop paints with from now on.
fn cycle_theme(
    tb_surf: &ViSurface,
    pref: &mut ThemePreferenceState,
) -> alloc::boxed::Box<dyn ViTheme> {
    if advance_theme(pref) {
        ostd::io::print("[desktop] Appearance: ");
        ostd::io::println(pref.palette.name());
        announce_appearance(tb_surf, *pref);
    } else {
        ostd::io::println("[desktop] Appearance: revision exhausted, palette unchanged");
    }
    pref.create_theme()
}

/// Tell the compositor which palette the desktop's chrome uses (honoured for Panel surfaces).
fn announce_appearance(tb_surf: &ViSurface, pref: ThemePreferenceState) {
    if tb_surf
        .set_appearance(pref.palette.appearance(), pref.revision)
        .is_err()
    {
        ostd::io::println("[desktop] set_appearance failed; compositor chrome keeps its palette");
    }
}

/// Spawn the handler for a Spotlight path intent. A `Run` of a `/bin` program
/// spawns that program; any other action hands the handler the target as argv[0].
fn launch_intent(intent: &api::services::mime::LaunchIntent) {
    ostd::io::print("[desktop] Spotlight: Launching intent ");
    ostd::io::print(intent.action.as_str());
    ostd::io::print(" target=");
    ostd::io::print(&intent.target_path);
    ostd::io::print(" with app=");
    ostd::io::println(intent.app_binary);
    if intent.action == api::services::mime::FileAction::Run
        && intent.target_path.starts_with("/bin/")
    {
        launch_application(&intent.target_path);
        return;
    }
    let argv = core::slice::from_ref(&intent.target_path);
    // A rejected spawn discards the staged argv, so stage it again before the
    // VFS-route fallback (the same order cellbox uses).
    if !ostd::set_spawn_argv(argv) {
        ostd::io::println("[desktop] ERROR: launch target exceeds the argv transport limit.");
        return;
    }
    let mut spawned = sys_spawn_from_path_raw(intent.app_binary);
    if matches!(spawned, SyscallResult::Err(_)) && ostd::set_spawn_argv(argv) {
        spawned = sys_spawn_from_path(intent.app_binary);
    }
    report_spawn(spawned);
}

/// Launch a `/bin` app. The kernel's path route comes first: only it gives the
/// child a measured launch identity, without which the child's own reviewed
/// launch edges (Terminal -> `/bin/cellbox`) are refused. The VFS route stays
/// as the fallback for cells the kernel path route cannot resolve.
fn launch_application(path: &str) {
    ostd::io::print("[desktop] Launching application: ");
    ostd::io::println(path);
    let mut spawned = sys_spawn_from_path_raw(path);
    if matches!(spawned, SyscallResult::Err(_)) {
        spawned = sys_spawn_from_path(path);
    }
    report_spawn(spawned);
}

fn report_spawn(spawned: SyscallResult) {
    match spawned {
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

#[cfg(test)]
mod tests {
    use super::*;
    use viui::theme::SystemPalette;

    #[test]
    fn theme_cycle_advances_palette_under_strictly_growing_revision() {
        let mut pref = ThemePreferenceState::default();
        let mut seen = alloc::vec::Vec::new();
        for _ in 0..3 {
            let before = pref.revision;
            assert!(advance_theme(&mut pref));
            assert!(pref.revision > before);
            seen.push(pref.palette);
        }
        assert_eq!(
            seen,
            [
                SystemPalette::Dark,
                SystemPalette::Paper,
                SystemPalette::Light
            ]
        );

        // A preference at the last revision cannot advance: the compositor would refuse it.
        let mut last = ThemePreferenceState::new(SystemPalette::Dark, u32::MAX);
        assert!(!advance_theme(&mut last));
        assert_eq!(
            last,
            ThemePreferenceState::new(SystemPalette::Dark, u32::MAX)
        );
    }
}
