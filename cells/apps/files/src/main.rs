// SPDX-License-Identifier: MIT
//! CellOS Standalone Files Manager Application (List, Grid, Miller Columns, VFS operations).

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod draw;
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
use crate::model::{
    resolve_conflict_name, FileItem, PlacesItem, ViewMode, DEFAULT_PLACES,
};
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
    GetTime
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

const TOOLBAR_HEIGHT: u32 = 44;
const STATUS_HEIGHT: u32 = 24;

pub struct FilesApp {
    pub current_path: String,
    pub view_mode: ViewMode,
    pub items: Vec<FileItem>,
    pub selected_index: usize,
    pub places: Vec<PlacesItem>,
    pub clipboard: Option<(String, bool)>, // (path, is_cut)
    pub status_message: String,
    pub width: u32,
    pub height: u32,
    pub quick_look: preview::QuickLookState,
}

impl FilesApp {
    pub fn new(initial_path: &str, width: u32, height: u32) -> Self {
        let mut app = Self {
            current_path: String::from(initial_path),
            view_mode: ViewMode::List,
            items: Vec::new(),
            selected_index: 0,
            places: DEFAULT_PLACES.to_vec(),
            clipboard: None,
            status_message: format!("Ready. {} items", 0),
            width,
            height,
            quick_look: preview::QuickLookState::new(),
        };
        app.load_directory(initial_path);
        app
    }

    pub fn load_directory(&mut self, path: &str) {
        self.current_path = String::from(path);
        self.selected_index = 0;
        self.items.clear();

        // Populate sample directory items representing real VFS entries
        if path == "/" {
            self.items.push(FileItem::new("bin", "/bin", true, 0));
            self.items.push(FileItem::new("data", "/data", true, 0));
            self.items.push(FileItem::new("mnt", "/mnt", true, 0));
            self.items.push(FileItem::new("srv", "/srv", true, 0));
            self.items.push(FileItem::new("welcome.md", "/welcome.md", false, 1850));
        } else if path == "/bin" {
            self.items.push(FileItem::new("..", "/", true, 0));
            self.items.push(FileItem::new("ocel", "/bin/ocel", false, 1450000));
            self.items.push(FileItem::new("shell", "/bin/shell", false, 881000));
            self.items.push(FileItem::new("files", "/bin/files", false, 520000));
            self.items.push(FileItem::new("desktop", "/bin/desktop", false, 410000));
            self.items.push(FileItem::new("fb-console", "/bin/fb-console", false, 588000));
        } else if path == "/data" {
            self.items.push(FileItem::new("..", "/", true, 0));
            self.items.push(FileItem::new("notes.md", "/data/notes.md", false, 4096));
            self.items.push(FileItem::new("project.rs", "/data/project.rs", false, 12500));
            self.items.push(FileItem::new("sample.pdf", "/data/sample.pdf", false, 245000));
            self.items.push(FileItem::new("chart.png", "/data/chart.png", false, 98000));
        } else {
            self.items.push(FileItem::new("..", "/", true, 0));
            self.items.push(FileItem::new("readme.txt", format!("{}/readme.txt", path), false, 512));
        }

        self.status_message = format!("{}: {} item(s)", self.current_path, self.items.len());
    }

    pub fn navigate_up(&mut self) {
        if self.current_path == "/" {
            return;
        }
        let parent = match self.current_path.rfind('/') {
            Some(0) => "/",
            Some(idx) => &self.current_path[..idx],
            None => "/",
        };
        let p = String::from(parent);
        self.load_directory(&p);
    }

    pub fn open_selected(&mut self) -> Option<String> {
        let item = self.items.get(self.selected_index)?;
        if item.is_dir {
            if item.name == ".." {
                self.navigate_up();
            } else {
                let target = item.path.clone();
                self.load_directory(&target);
            }
            None
        } else {
            self.status_message = format!("Opening {} via associated application...", item.name);
            Some(item.path.clone())
        }
    }

    pub fn copy_selected(&mut self, is_cut: bool) {
        if let Some(item) = self.items.get(self.selected_index) {
            self.clipboard = Some((item.path.clone(), is_cut));
            self.status_message = format!(
                "{} {}",
                if is_cut { "Cut" } else { "Copied" },
                item.name
            );
        }
    }

    pub fn paste_into_current(&mut self) {
        if let Some((src_path, is_cut)) = self.clipboard.take() {
            let src_name = match src_path.rfind('/') {
                Some(idx) => &src_path[idx + 1..],
                None => src_path.as_str(),
            };

            let existing_names: Vec<String> = self.items.iter().map(|it| it.name.clone()).collect();
            let final_name = resolve_conflict_name(src_name, &existing_names);
            let final_path = if self.current_path == "/" {
                format!("/{}", final_name)
            } else {
                format!("{}/{}", self.current_path, final_name)
            };

            let new_item = FileItem::new(&final_name, &final_path, false, 1024);
            self.items.push(new_item);
            self.selected_index = self.items.len().saturating_sub(1);
            self.status_message = format!(
                "Pasted {} -> {}",
                src_name,
                final_name
            );

            if !is_cut {
                self.clipboard = Some((src_path, false));
            }
        }
    }

    pub fn delete_selected(&mut self) {
        if self.selected_index < self.items.len() {
            let removed = self.items.remove(self.selected_index);
            if self.selected_index >= self.items.len() && !self.items.is_empty() {
                self.selected_index = self.items.len() - 1;
            }
            self.status_message = format!("Permanently deleted {} (irreversible)", removed.name);
        }
    }

    pub fn switch_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.status_message = format!("View mode: {}", mode.name());
    }

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

        // 4. Status Bar Component (Bottom 24 DIP)
        let status_y = sh as i32 - STATUS_HEIGHT as i32;
        StatusBar::render(surf, fonts, theme, status_y, sw, STATUS_HEIGHT, &self.status_message);
        preview::render(surf, fonts, &self.quick_look, sw, sh);
        surf.damage_all();
    }
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
    let _ = surf.set_title("Files — /data");
    surf.move_to(100, 100);

    let mut app = FilesApp::new("/data", win_w, win_h);
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
                InputEvent::Key(ke) if ke.state == KeyState::Pressed => {
                    match ke.keysym {
                        KeySym::Escape => {
                            if app.quick_look.is_open {
                                app.quick_look.close();
                                need_render = true;
                            }
                        }
                        KeySym::Up => {
                            if app.selected_index > 0 {
                                app.selected_index -= 1;
                                if app.quick_look.is_open {
                                    app.quick_look.inspect(&app.items[app.selected_index]);
                                }
                                need_render = true;
                            }
                        }
                        KeySym::Down => {
                            if !app.items.is_empty() && app.selected_index + 1 < app.items.len() {
                                app.selected_index += 1;
                                if app.quick_look.is_open {
                                    app.quick_look.inspect(&app.items[app.selected_index]);
                                }
                                need_render = true;
                            }
                        }
                        KeySym::Return => {
                            if app.quick_look.is_open {
                                app.quick_look.close();
                            }
                            let _ = app.open_selected();
                            let _ = surf.set_title(&format!("Files — {}", app.current_path));
                            need_render = true;
                        }
                        KeySym::Backspace => {
                            app.navigate_up();
                            let _ = surf.set_title(&format!("Files — {}", app.current_path));
                            need_render = true;
                        }
                        KeySym::Delete => {
                            app.delete_selected();
                            need_render = true;
                        }
                        _ => {
                            if let Some(ch) = ke.char() {
                                if ke.modifiers.contains(Modifiers::CTRL) {
                                    match ch {
                                        'c' => app.copy_selected(false),
                                        'x' => app.copy_selected(true),
                                        'v' => app.paste_into_current(),
                                        _ => {}
                                    }
                                    need_render = true;
                                } else {
                                    match ch {
                                        ' ' => {
                                            if app.quick_look.is_open {
                                                app.quick_look.close();
                                            } else if !app.items.is_empty() {
                                                app.quick_look.inspect(&app.items[app.selected_index]);
                                            }
                                        }
                                        '1' => { app.switch_view_mode(ViewMode::List); }
                                        '2' => { app.switch_view_mode(ViewMode::Grid); }
                                        '3' => { app.switch_view_mode(ViewMode::Columns); }
                                        _ => {}
                                    }
                                    need_render = true;
                                }
                            }
                        }
                    }
                }
                InputEvent::MouseButton { button: MouseButton::Left, state: KeyState::Pressed } => {
                    // Check Quick Look modal click first
                    if app.quick_look.is_open {
                        if let Some(open_with) = preview::handle_click(&mut app.quick_look, cursor_x, cursor_y, app.width, app.height) {
                            if open_with {
                                let _ = app.open_selected();
                            }
                            need_render = true;
                            continue;
                        }
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
                        need_render = true;
                    }

                    // Check Places sidebar
                    if let Some(target_path) = views::places::handle_click(&app.places, cursor_x, cursor_y) {
                        app.load_directory(target_path);
                        let _ = surf.set_title(&format!("Files — {}", app.current_path));
                        need_render = true;
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
                        need_render = true;
                    }
                }
                _ => {}
            }
        }

        if need_render {
            app.render(&mut surf, &mut fonts);
        }

        sys_yield();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_app_navigation_and_modes() {
        let mut app = FilesApp::new("/data", 800, 600);
        assert_eq!(app.view_mode, ViewMode::List);
        assert_eq!(app.current_path, "/data");
        assert!(!app.items.is_empty());

        // Switch modes
        app.switch_view_mode(ViewMode::Grid);
        assert_eq!(app.view_mode, ViewMode::Grid);
        app.switch_view_mode(ViewMode::Columns);
        assert_eq!(app.view_mode, ViewMode::Columns);
        app.switch_view_mode(ViewMode::List);
        assert_eq!(app.view_mode, ViewMode::List);

        // Navigation to parent
        app.navigate_up();
        assert_eq!(app.current_path, "/");

        // Copy and paste with conflict resolution
        app.load_directory("/data");
        app.selected_index = 1; // "notes.md"
        let orig_name = app.items[1].name.clone();
        app.copy_selected(false);
        assert!(app.clipboard.is_some());

        app.paste_into_current();
        let pasted_name = app.items.last().unwrap().name.clone();
        assert_ne!(orig_name, pasted_name);
        assert!(pasted_name.starts_with("notes (1)"));

        // Delete pasted item
        let count_before = app.items.len();
        app.delete_selected();
        assert_eq!(app.items.len(), count_before - 1);
    }

    #[test]
    fn test_mouse_click_hit_testing() {
        let app = FilesApp::new("/data", 800, 600);

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

    #[test]
    fn test_quick_look_space_toggle() {
        let mut app = FilesApp::new("/data", 800, 600);
        assert!(!app.quick_look.is_open);

        // Inspect selected item (e.g. index 1 notes.md)
        app.selected_index = 1;
        app.quick_look.inspect(&app.items[app.selected_index]);
        assert!(app.quick_look.is_open);
        assert_eq!(app.quick_look.kind, preview::PreviewKind::Markdown);

        // Close Quick Look
        app.quick_look.close();
        assert!(!app.quick_look.is_open);
        // Open Quick Look and test mouse click outside
        app.quick_look.inspect(&app.items[1]);
        assert!(app.quick_look.is_open);
        // Click outside (e.g. at 10, 10)
        let click_out = preview::handle_click(&mut app.quick_look, 10, 10, 800, 600);
        assert_eq!(click_out, Some(false));
        assert!(!app.quick_look.is_open);

        // Open Quick Look and test Open With button click
        app.quick_look.inspect(&app.items[1]);
        assert!(app.quick_look.is_open);
        // Open With button is at ql_x + ql_w - 102 = 130 + 540 - 102 = 568, y = 100 + 6 = 106
        let click_btn = preview::handle_click(&mut app.quick_look, 580, 115, 800, 600);
        assert_eq!(click_btn, Some(true));
        assert!(!app.quick_look.is_open);
    }
}
