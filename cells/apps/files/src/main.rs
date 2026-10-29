// SPDX-License-Identifier: MIT
//! CellOS Standalone Files Manager Application (List, Grid, Miller Columns) on the VFS service.
//!
//! Keys: arrows select, Enter opens (folder) or previews (file), Backspace goes up, Space
//! toggles Quick Look, 1/2/3 switch views, Ctrl+C/X/V copy/cut/paste, Delete removes,
//! Ctrl+L goes to a path, Ctrl+N creates a folder and Ctrl+Shift+N an empty file.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod app;
mod draw;
mod fs;
mod model;
mod preview;
mod views;
use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{sys_get_resolution, sys_yield};
use ostd::typography::TextFonts;
use viui::surface::components::{
    GridItemDef, GridView, ListView, ListViewItemDef, PlacesItemDef, PlacesSidebar,
    SegmentedControl, StatusBar, Toolbar,
};
use viui::theme::{ViTheme, LIGHT_THEME};
use crate::app::{FilesApp, PromptKind};
use crate::fs::FileSystem;
use crate::model::ViewMode;
use crate::views::places::PLACES_WIDTH;
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
    Recv,
    TryRecv,
    LookupService,
    GpuGetResolution,
    GetTime,
    // Mkdir/Write/Append/Rename/Unlink/Rmdir through the VFS service (path policy applies).
    VfsMutate
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

const TOOLBAR_HEIGHT: u32 = 44;
const STATUS_HEIGHT: u32 = 24;

impl<F: FileSystem> FilesApp<F> {
    pub fn render(&self, surf: &mut ViSurface, fonts: &mut TextFonts) {
        let sw = self.width;
        let sh = self.height;
        let theme = &LIGHT_THEME;

        // 1. Toolbar Component (Top 44 DIP)
        let path_title = format!("Files: {}", self.current_path);
        Toolbar::render(surf, fonts, theme, sw, TOOLBAR_HEIGHT, "[F]", &path_title);

        // View Mode SegmentedControl: [List] [Grid] [Cols]
        let btn_start_x = sw as i32 - 180;
        let cur_mode_idx = match self.view_mode {
            ViewMode::List => 0,
            ViewMode::Grid => 1,
            ViewMode::Columns => 2,
        };
        SegmentedControl::render(
            surf,
            fonts,
            theme,
            btn_start_x,
            8,
            54,
            28,
            &["List", "Grid", "Cols"],
            cur_mode_idx,
        );

        // 2. Sidebar: Places Component
        let content_y = TOOLBAR_HEIGHT as i32;
        let content_h = sh.saturating_sub(TOOLBAR_HEIGHT + STATUS_HEIGHT);
        let places_defs: Vec<PlacesItemDef> = self
            .places
            .iter()
            .map(|p| PlacesItemDef {
                name: String::from(p.name),
                path: String::from(p.path),
                icon: String::from(p.icon),
            })
            .collect();
        PlacesSidebar::render(
            surf,
            fonts,
            theme,
            0,
            content_y,
            PLACES_WIDTH,
            content_h,
            "PLACES",
            &places_defs,
            &self.current_path,
        );

        // 3. Main Content View
        let content_x = PLACES_WIDTH as i32;
        let content_w = sw.saturating_sub(PLACES_WIDTH);

        match self.view_mode {
            ViewMode::List => {
                let list_defs: Vec<ListViewItemDef> = self
                    .items
                    .iter()
                    .map(|item| ListViewItemDef {
                        icon: String::from(item.icon),
                        name: item.name.clone(),
                        size_str: item.formatted_size(),
                        kind_str: String::from(if item.is_dir { "Folder" } else { "File" }),
                        is_dir: item.is_dir,
                    })
                    .collect();
                ListView::render(
                    surf,
                    fonts,
                    theme,
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    &list_defs,
                    Some(self.selected_index),
                );
            }
            ViewMode::Grid => {
                let grid_defs: Vec<GridItemDef> = self
                    .items
                    .iter()
                    .map(|item| GridItemDef {
                        icon: String::from(item.icon),
                        name: item.name.clone(),
                        is_dir: item.is_dir,
                    })
                    .collect();
                GridView::render(
                    surf,
                    fonts,
                    theme,
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    &grid_defs,
                    Some(self.selected_index),
                );
            }
            ViewMode::Columns => {
                views::columns::render(
                    surf,
                    fonts,
                    &self.items,
                    self.selected_index,
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    theme.bg(),
                    theme.text_primary(),
                    theme.accent(),
                    theme.border(),
                );
            }
        }

        // 4. Status Bar Component (Bottom 24 DIP): listing summary, prompt or operation result.
        let status_y = sh as i32 - STATUS_HEIGHT as i32;
        StatusBar::render(surf, fonts, theme, status_y, sw, STATUS_HEIGHT, &self.status_message);
        preview::render(surf, fonts, &self.quick_look, sw, sh);
        surf.damage_all();
    }
}

/// Keys while the path/name prompt is open. Returns whether the prompt consumed the key.
fn prompt_key<F: FileSystem>(app: &mut FilesApp<F>, keysym: KeySym, ch: Option<char>, modifiers: Modifiers) -> bool {
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
    let mut fonts = TextFonts::new();

    while !ostd::input::request_focus() {
        sys_yield();
    }

    app.render(&mut surf, &mut fonts);
    ostd::io::println("[files] Files surface ready. Entering event loop.");
    let mut cursor_x = 0i32;
    let mut cursor_y = 0i32;

    loop {
        let mut need_render = false;

        for ev in ostd::input::poll_events(16) {
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    cursor_x = x;
                    cursor_y = y;
                }
                InputEvent::Key(ke) if matches!(ke.state, KeyState::Pressed | KeyState::Repeated) => {
                    let ch = ke.char();
                    // Only navigation and prompt typing repeat; opening, deletion, paste and
                    // toggles are one-shot.
                    let repeatable = matches!(ke.keysym, KeySym::Up | KeySym::Down | KeySym::Backspace)
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
                        KeySym::Delete => app.delete_selected(),
                        _ => match ch {
                            Some(c) if ke.modifiers.contains(Modifiers::CTRL) => {
                                match c.to_ascii_lowercase() {
                                    'c' => app.copy_selected(false),
                                    'x' => app.copy_selected(true),
                                    'v' => app.paste_into_current(),
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
                            Some('1') => app.switch_view_mode(ViewMode::List),
                            Some('2') => app.switch_view_mode(ViewMode::Grid),
                            Some('3') => app.switch_view_mode(ViewMode::Columns),
                            _ => {}
                        },
                    }
                }
                InputEvent::MouseButton { button: MouseButton::Left, state: KeyState::Pressed } => {
                    // A click in the window takes the keyboard back, as the Terminal does.
                    let _ = ostd::input::request_focus();
                    need_render = true;
                    if preview::handle_click(&mut app.quick_look, cursor_x, cursor_y, app.width, app.height) {
                        continue;
                    }
                    // Check toolbar view mode SegmentedControl
                    let btn_start_x = app.width as i32 - 180;
                    if let Some(idx) = SegmentedControl::hit_test(cursor_x, cursor_y, btn_start_x, 8, 54, 28, 3) {
                        let new_mode = match idx {
                            0 => ViewMode::List,
                            1 => ViewMode::Grid,
                            _ => ViewMode::Columns,
                        };
                        app.switch_view_mode(new_mode);
                    }

                    // Check Places sidebar
                    if let Some(target_path) = views::places::handle_click(&app.places, cursor_x, cursor_y) {
                        app.load_directory(target_path);
                    }

                    // Check Main Content Area
                    let content_x = PLACES_WIDTH as i32;
                    let content_y = TOOLBAR_HEIGHT as i32;
                    let content_w = app.width.saturating_sub(PLACES_WIDTH);
                    let clicked_idx = match app.view_mode {
                        ViewMode::List => ListView::hit_test(cursor_x, cursor_y, content_x, content_y, content_w, app.items.len()),
                        ViewMode::Grid => GridView::hit_test(cursor_x, cursor_y, content_x, content_y, content_w, app.items.len()),
                        ViewMode::Columns => views::columns::handle_click(app.items.len(), content_x, content_y, cursor_x, cursor_y),
                    };
                    if let Some(idx) = clicked_idx {
                        app.selected_index = idx;
                    }
                }
                _ => {}
            }
        }

        if need_render {
            if app.current_path != title_path {
                title_path = app.current_path.clone();
                let _ = surf.set_title(&format!("Files — {}", title_path));
            }
            app.render(&mut surf, &mut fonts);
        }

        sys_yield();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fake::FakeFs;

    fn sample() -> FilesApp<FakeFs> {
        let fs = FakeFs::new().dir("/data").file("/data/notes.md", b"# n\n").file("/data/b.rs", b"");
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
        assert!(prompt_key(&mut app, KeySym::Printable, Some('/'), Modifiers::default()));
        assert!(prompt_key(&mut app, KeySym::Printable, Some('c'), Modifiers::CTRL));
        assert_eq!(app.status_message, "Go to: /_");
        assert!(prompt_key(&mut app, KeySym::Return, None, Modifiers::default()));
        assert_eq!(app.current_path, "/");
        assert!(!prompt_key(&mut app, KeySym::Return, None, Modifiers::default()));
    }

    #[test]
    fn test_mouse_click_hit_testing() {
        let app = sample();

        // Click on Places item (e.g. Applications /bin at y=104)
        let place_clicked = views::places::handle_click(&app.places, 50, 110);
        assert_eq!(place_clicked, Some("/bin"));

        // Click outside places sidebar
        let place_miss = views::places::handle_click(&app.places, 200, 110);
        assert_eq!(place_miss, None);

        // Click on List row 0
        let content_x = PLACES_WIDTH as i32;
        let content_y = TOOLBAR_HEIGHT as i32;
        let content_w = 800 - PLACES_WIDTH;
        let list_clicked = views::list::handle_click(app.items.len(), content_x, content_y, content_w, content_x + 50, content_y + 35);
        assert_eq!(list_clicked, Some(0));

        // Click on Grid item 0
        let grid_clicked = views::grid::handle_click(app.items.len(), content_x, content_y, content_w, content_x + 30, content_y + 30);
        assert_eq!(grid_clicked, Some(0));

        // Click on Columns item 0
        let col_clicked = views::columns::handle_click(app.items.len(), content_x, content_y, content_x + 30, content_y + 10);
        assert_eq!(col_clicked, Some(0));
    }
}
