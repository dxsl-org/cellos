// SPDX-License-Identifier: MIT
//! CellOS Standalone Files Manager Application (List, Grid, Miller Columns) on the VFS service.
//!
//! Keys: arrows select, Enter opens (folder) or previews (file), Backspace goes up, Space
//! toggles Quick Look, 1/2/3 switch views, Ctrl+C/X/V copy/cut/paste, Delete trashes
//! (Shift+Delete deletes for good), Ctrl+Z restores from Trash, Ctrl+L goes to a path,
//! Ctrl+N creates a folder and Ctrl+Shift+N an empty file. F3 or Ctrl+E (or the toolbar
//! Split button) shows/hides a second pane; while split, Tab moves focus between panes
//! and F5/F6 copy/move the selection into the other pane's folder.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
use alloc::format;

mod app;
mod draw;
mod fs;
mod layout;
mod model;
mod preview;
mod render;
mod split;
mod views;
use crate::app::{FilesApp, PromptKind};
use crate::fs::FileSystem;
use crate::model::ViewMode;
use api::display::PixelFormat;
use ostd::display::linear::icons::LineIcon;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{
    sys_get_resolution, sys_spawn_from_path, sys_spawn_from_path_raw, sys_yield, SyscallResult,
};
use ostd::typography::TextFonts;
use viui::window_lifecycle::{ClosePolicy, WindowEvent, WindowLifecycle};
api::declare_manifest!(block_io = false, network = false, spawn = true);
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
    // Mkdir/Write/Append/Rename/Unlink/Rmdir through the VFS service (path policy applies).
    VfsMutate,
    // Stages a LaunchIntent target as the handler's argv.
    StateStash
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

/// Keys while the path/name prompt is open. Returns whether the prompt consumed the key.
fn prompt_key<F: FileSystem>(
    app: &mut FilesApp<F>,
    keysym: KeySym,
    ch: Option<char>,
    modifiers: Modifiers,
) -> bool {
    if app.prompt.is_none() {
        return false;
    }
    match keysym {
        KeySym::Escape => app.prompt_cancel(),
        KeySym::Return => app.prompt_commit(),
        KeySym::Backspace => app.prompt_backspace(),
        _ => {
            if let Some(ch) = ch.filter(|_| !modifiers.contains(Modifiers::CTRL)) {
                app.prompt_input(ch);
            }
        }
    }
    true
}

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

    ostd::io::println("[files] CellOS Files Manager initializing...");
    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = 800.min(screen_w);
    let win_h = 600.min(screen_h - 36);

    let mut surf = ViSurface::create(comp_tid, win_w, win_h, PixelFormat::Bgra8888)
        .expect("create files surface");
    surf.move_to(100, 100);

    let mut app = FilesApp::new(fs::Vfs::new(), "/data", win_w, win_h);
    let mut title_path = app.current_path.clone();
    let _ = surf.set_title(&format!("Files — {}", title_path));
    if surf.set_icon(LineIcon::Folder).is_err() {
        ostd::io::println("[files] set_icon failed; Cell Tab keeps the generic icon");
    }
    let mut fonts = TextFonts::new();

    while !ostd::input::request_focus() {
        sys_yield();
    }

    // Palette generation the last frame was painted with; a change repaints everything.
    let mut painted_serial = viui::theme::system_serial();
    app.render(&mut surf, &mut fonts);
    ostd::io::println("[files] Files surface ready. Entering event loop.");
    let mut cursor_x = 0i32;
    let mut cursor_y = 0i32;
    // Resizes, minimize and the window's close button, answered like every
    // other ViUI window: closing Files simply closes it.
    let mut window = WindowLifecycle::new(ClosePolicy::Accept);

    loop {
        let mut need_render = false;
        window.poll(&mut surf, |event| match event {
            WindowEvent::Resized { width, height } => {
                app.resize(width, height);
                need_render = true;
            }
            WindowEvent::Restored => need_render = true,
            WindowEvent::Minimized | WindowEvent::CloseRequested { .. } | WindowEvent::Closed => {}
        });
        if window.is_closed() {
            drop(surf);
            ostd::io::println("[files] Window closed; exiting.");
            return;
        }

        for ev in ostd::input::poll_events(16) {
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    cursor_x = x;
                    cursor_y = y;
                }
                InputEvent::Key(ke)
                    if matches!(ke.state, KeyState::Pressed | KeyState::Repeated) =>
                {
                    let ch = ke.char();
                    // Only navigation and prompt typing repeat; opening, deletion, paste and
                    // toggles are one-shot.
                    let repeatable =
                        matches!(ke.keysym, KeySym::Up | KeySym::Down | KeySym::Backspace)
                            || (app.prompt.is_some() && ch.is_some_and(|c| !c.is_control()));
                    if ke.state == KeyState::Repeated && !repeatable {
                        continue;
                    }
                    need_render = true;
                    if prompt_key(&mut app, ke.keysym, ch, ke.modifiers) {
                        continue;
                    }
                    match ke.keysym {
                        KeySym::Escape => app.quick_look.close(),
                        KeySym::Up | KeySym::Down => {
                            let down = ke.keysym == KeySym::Down;
                            if down && app.selected_index + 1 < app.items.len() {
                                app.selected_index += 1;
                            } else if !down && app.selected_index > 0 {
                                app.selected_index -= 1;
                            }
                            if app.quick_look.is_open {
                                app.preview_selected();
                            }
                        }
                        KeySym::Return => {
                            app.quick_look.close();
                            app.open_selected();
                        }
                        KeySym::Backspace => app.navigate_up(),
                        KeySym::Delete => {
                            if ke.modifiers.contains(Modifiers::SHIFT) {
                                app.delete_selected();
                            } else {
                                app.trash_selected();
                            }
                        }
                        KeySym::Tab => app.switch_active_pane(),
                        KeySym::F3 => app.toggle_split_pane(),
                        KeySym::F5 => app.copy_to_other_pane(false),
                        KeySym::F6 => app.copy_to_other_pane(true),
                        _ => match ch {
                            Some(c) if ke.modifiers.contains(Modifiers::CTRL) => {
                                match c.to_ascii_lowercase() {
                                    'c' => app.copy_selected(false),
                                    'x' => app.copy_selected(true),
                                    'v' => app.paste_into_current(),
                                    'z' => app.undo_last_trash(),
                                    'e' => app.toggle_split_pane(),
                                    'l' => app.begin_prompt(PromptKind::GoTo),
                                    'n' if ke.modifiers.contains(Modifiers::SHIFT) => {
                                        app.begin_prompt(PromptKind::NewFile)
                                    }
                                    'n' => app.begin_prompt(PromptKind::NewFolder),
                                    _ => {}
                                }
                            }
                            Some(' ') => {
                                if app.quick_look.is_open {
                                    app.quick_look.close();
                                } else {
                                    app.preview_selected();
                                }
                            }
                            Some(digit @ '1'..='9') if app.quick_look.is_open => {
                                let idx = (digit as usize) - ('1' as usize);
                                if idx < app.quick_look.candidates.len() {
                                    let app_id = app.quick_look.candidates[idx].app_id;
                                    app.quick_look.close();
                                    app.open_selected_with(app_id);
                                }
                            }
                            Some('1') => app.switch_view_mode(ViewMode::List),
                            Some('2') => app.switch_view_mode(ViewMode::Grid),
                            Some('3') => app.switch_view_mode(ViewMode::Columns),
                            _ => {}
                        },
                    }
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Pressed,
                } => {
                    // A click in the window takes the keyboard back, as the Terminal does.
                    let _ = ostd::input::request_focus();
                    need_render = true;
                    if preview::handle_click(
                        &mut app.quick_look,
                        cursor_x,
                        cursor_y,
                        app.width,
                        app.height,
                    ) {
                        continue;
                    }
                    if let Some(idx) = layout::view_switch(app.width).hit(cursor_x, cursor_y) {
                        let new_mode = match idx {
                            0 => ViewMode::List,
                            1 => ViewMode::Grid,
                            _ => ViewMode::Columns,
                        };
                        app.switch_view_mode(new_mode);
                    } else if layout::split_button(app.width)
                        .hit(cursor_x, cursor_y)
                        .is_some()
                    {
                        app.toggle_split_pane();
                    } else if let Some(target_path) =
                        views::places::handle_click(&app.places, cursor_x, cursor_y)
                    {
                        // Places open in the focused pane.
                        app.load_directory(target_path);
                    } else {
                        app.click_listing(cursor_x, cursor_y);
                    }
                }
                _ => {}
            }
        }
        if let Some(intent) = app.take_launch_intent() {
            dispatch_launch_intent(&intent);
        }

        let serial = viui::theme::system_serial();
        if serial != painted_serial {
            painted_serial = serial;
            need_render = true;
        }
        if need_render && !window.is_minimized() {
            if app.current_path != title_path {
                title_path = app.current_path.clone();
                let _ = surf.set_title(&format!("Files — {}", title_path));
            }
            app.render(&mut surf, &mut fonts);
        }

        sys_yield();
    }
}
fn dispatch_launch_intent(intent: &api::services::mime::LaunchIntent) {
    ostd::io::print("[files] LaunchIntent: ");
    ostd::io::print(intent.action.as_str());
    ostd::io::print(" target=");
    ostd::io::print(&intent.target_path);
    ostd::io::print(" app=");
    ostd::io::println(intent.app_binary);

    let spawned = if intent.action == api::services::mime::FileAction::Run
        && intent.target_path.starts_with("/bin/")
    {
        sys_spawn_from_path(&intent.target_path)
    } else {
        // The handler gets the target as argv[0]. A rejected spawn discards the
        // staged argv, so stage it again before the VFS-route fallback.
        let argv = core::slice::from_ref(&intent.target_path);
        if !ostd::set_spawn_argv(argv) {
            ostd::io::println("[files] ERROR: launch target exceeds the argv transport limit.");
            return;
        }
        let mut spawned = sys_spawn_from_path_raw(intent.app_binary);
        if matches!(spawned, SyscallResult::Err(_)) && ostd::set_spawn_argv(argv) {
            spawned = sys_spawn_from_path(intent.app_binary);
        }
        spawned
    };
    match spawned {
        SyscallResult::Ok(tid) => {
            ostd::io::println(&format!("[files] Spawned application PID {}", tid));
        }
        SyscallResult::Err(_) => {
            ostd::io::println("[files] ERROR: Application spawn rejected by kernel.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fake::FakeFs;

    fn sample() -> FilesApp<FakeFs> {
        let fs = FakeFs::new()
            .dir("/data")
            .file("/data/notes.md", b"# n\n")
            .file("/data/b.rs", b"");
        FilesApp::new(fs, "/data", 800, 600)
    }

    #[test]
    fn view_modes_switch_and_parent_navigation_lists_the_vfs_root() {
        let mut app = sample();
        assert_eq!(app.view_mode, ViewMode::List);
        app.switch_view_mode(ViewMode::Grid);
        assert_eq!(app.view_mode, ViewMode::Grid);
        app.switch_view_mode(ViewMode::Columns);
        assert_eq!(app.view_mode, ViewMode::Columns);
        app.navigate_up();
        assert_eq!(app.current_path, "/");
        assert_eq!(app.items.len(), 1);
        assert_eq!(app.items[0].name, "data");
    }

    #[test]
    fn prompt_keys_are_routed_before_shortcuts() {
        let mut app = sample();
        app.begin_prompt(PromptKind::GoTo);
        assert!(prompt_key(
            &mut app,
            KeySym::Printable,
            Some('/'),
            Modifiers::default()
        ));
        assert!(prompt_key(
            &mut app,
            KeySym::Printable,
            Some('c'),
            Modifiers::CTRL
        ));
        assert_eq!(app.status_message, "Go to: /_");
        assert!(prompt_key(
            &mut app,
            KeySym::Return,
            None,
            Modifiers::default()
        ));
        assert_eq!(app.current_path, "/");
        assert!(!prompt_key(
            &mut app,
            KeySym::Return,
            None,
            Modifiers::default()
        ));
    }

    #[test]
    fn test_mouse_click_hit_testing() {
        let mut app = sample();

        // Click on Places item (e.g. Applications /bin at y=104)
        let place_clicked = views::places::handle_click(&app.places, 50, 110);
        assert_eq!(place_clicked, Some("/bin"));

        // Click outside places sidebar
        let place_miss = views::places::handle_click(&app.places, 200, 110);
        assert_eq!(place_miss, None);

        // The second listing entry (after "..") in each view mode, unsplit: the content
        // area starts right of Places (x 160) and under the toolbar (y 44).
        let content = layout::content_rect(800, 600);
        for (mode, x, y) in [
            (ViewMode::List, 50, 28 + 32 + 5),
            (ViewMode::Grid, 12 + 116 + 5, 12 + 5),
            (ViewMode::Columns, 30, 28 + 5),
        ] {
            app.switch_view_mode(mode);
            app.selected_index = 0;
            assert!(app.click_listing(content.x + x, content.y + y));
            assert_eq!(app.selected_index, 1, "{mode:?}");
        }

        // Toolbar buttons are found where they are drawn.
        assert_eq!(layout::view_switch(800).hit(800 - 180 + 60, 20), Some(1));
        assert_eq!(layout::split_button(800).hit(800 - 180 - 30, 20), Some(0));
    }
}
