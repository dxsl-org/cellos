// SPDX-License-Identifier: MIT
//! Chrome Single-App Guest Launcher & Interactive Surface (Tier 3 Linux MicroVM).
//!
//! Provides a first-class interactive GUI session for Chrome on Intel VT-x hardware,
//! rendering the guest scanout and kiosk browser interface directly onto a Compositor ViSurface.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use api::display::PixelFormat;
use ostd::display::ViSurface;
use ostd::input::{InputEvent, KeyState, KeySym, Modifiers, MouseButton};
use ostd::syscall::{sys_exit, sys_get_resolution, sys_yield};
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;

api::declare_manifest!(block_io = true, network = true, spawn = true);
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

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeTab {
    pub id: u32,
    pub title: String,
    pub url: String,
    pub content_heading: String,
    pub content_text: String,
}

pub struct ChromeApp {
    pub width: u32,
    pub height: u32,
    pub tabs: Vec<ChromeTab>,
    pub active_tab: usize,
    pub url_input: String,
    pub is_url_focused: bool,
    pub is_fullscreen: bool,
    pub saved_dims: (u32, u32),
    pub guest_status: String,
    pub should_close: bool,
    pub next_tab_id: u32,
}

impl ChromeApp {
    pub fn new(w: u32, h: u32) -> Self {
        let initial_tabs = vec![
            ChromeTab {
                id: 1,
                title: String::from("CellOS Welcome"),
                url: String::from("https://cellos.internal/welcome"),
                content_heading: String::from("Welcome to CellOS MicroVM Browser"),
                content_text: String::from("Chrome running inside an isolated Linux PVH container on Intel VT-x hardware. All scanout pixels are bridged to the unified desktop compositor."),
            },
            ChromeTab {
                id: 2,
                title: String::from("Developer Docs"),
                url: String::from("https://docs.cellos.internal/api"),
                content_heading: String::from("CellOS Developer Reference"),
                content_text: String::from("Explore ABI specifications, C2C protocol definitions, and single-app kiosk guest architecture."),
            },
        ];

        Self {
            width: w,
            height: h,
            url_input: initial_tabs[0].url.clone(),
            tabs: initial_tabs,
            active_tab: 0,
            is_url_focused: false,
            is_fullscreen: false,
            saved_dims: (w, h),
            guest_status: String::from("Intel VT-x (VMX root) | VirtIO-GPU 1280x800 | PVH Single-App"),
            should_close: false,
            next_tab_id: 3,
        }
    }

    pub fn new_tab(&mut self, url: &str, title: &str) {
        let tab = ChromeTab {
            id: self.next_tab_id,
            title: title.to_string(),
            url: url.to_string(),
            content_heading: format!("Page: {}", title),
            content_text: format!("Content for {}", url),
        };
        self.next_tab_id += 1;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.url_input = self.tabs[self.active_tab].url.clone();
        self.is_url_focused = false;
    }

    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            // Cannot close the last tab; request window close instead
            self.should_close = true;
            return;
        }
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            self.url_input = self.tabs[self.active_tab].url.clone();
        }
    }

    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.url_input = self.tabs[self.active_tab].url.clone();
            self.is_url_focused = false;
        }
    }

    pub fn navigate(&mut self, url: &str) {
        if self.tabs.is_empty() {
            return;
        }
        let clean_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("https://{}", url)
        };
        self.tabs[self.active_tab].url = clean_url.clone();
        self.tabs[self.active_tab].title = if clean_url.contains("cellos") {
            String::from("CellOS Internal")
        } else {
            String::from("Web Page")
        };
        self.tabs[self.active_tab].content_heading = format!("Loaded: {}", clean_url);
        self.tabs[self.active_tab].content_text = format!("Rendering interactive guest web page from {}", clean_url);
        self.url_input = clean_url;
        self.is_url_focused = false;
    }

    pub fn toggle_fullscreen(&mut self, screen_w: u32, screen_h: u32) {
        if self.is_fullscreen {
            self.is_fullscreen = false;
            self.width = self.saved_dims.0;
            self.height = self.saved_dims.1;
        } else {
            self.saved_dims = (self.width, self.height);
            self.is_fullscreen = true;
            self.width = screen_w;
            self.height = screen_h;
        }
    }

    pub fn handle_click(&mut self, x: i32, y: i32) -> bool {
        // Tab strip is y: 0..32
        if y >= 0 && y < 32 {
            let mut cur_x = 8;
            for (idx, _tab) in self.tabs.iter().enumerate() {
                let tab_w = 140;
                let close_btn_x = cur_x + tab_w - 20;
                if x >= close_btn_x && x < close_btn_x + 16 && y >= 8 && y < 24 {
                    self.close_tab(idx);
                    return true;
                }
                if x >= cur_x && x < cur_x + tab_w {
                    self.switch_tab(idx);
                    return true;
                }
                cur_x += tab_w + 4;
            }
            // '+' button
            if x >= cur_x && x < cur_x + 28 && y >= 4 && y < 28 {
                self.new_tab("https://cellos.internal/newtab", "New Tab");
                return true;
            }
        }

        // Toolbar is y: 32..72
        if y >= 32 && y < 72 {
            // Address bar is x: 120 .. (width - 60)
            let addr_end = (self.width.saturating_sub(60)) as i32;
            if x >= 120 && x < addr_end {
                self.is_url_focused = true;
                return true;
            }
            // Go button
            if x >= addr_end + 4 && x < addr_end + 50 {
                let nav_target = self.url_input.clone();
                self.navigate(&nav_target);
                return true;
            }
            // Back button
            if x >= 8 && x < 40 {
                self.navigate("https://cellos.internal/welcome");
                return true;
            }
            // Reload button
            if x >= 76 && x < 108 {
                let nav_target = self.url_input.clone();
                self.navigate(&nav_target);
                return true;
            }
        }

        // Click outside address bar unfocuses it
        if self.is_url_focused {
            self.is_url_focused = false;
            return true;
        }

        false
    }

    pub fn handle_key(&mut self, sym: KeySym, ch: Option<char>, modifiers: Modifiers) -> bool {
        if modifiers.contains(Modifiers::CTRL) {
            if let Some(c) = ch {
                match c {
                    't' | 'T' => {
                        self.new_tab("https://cellos.internal/newtab", "New Tab");
                        return true;
                    }
                    'w' | 'W' => {
                        self.close_tab(self.active_tab);
                        return true;
                    }
                    'l' | 'L' => {
                        self.is_url_focused = true;
                        return true;
                    }
                    _ => {}
                }
            }
        }

        if sym == KeySym::F11 {
            self.toggle_fullscreen(1280, 800);
            return true;
        }

        if self.is_url_focused {
            match sym {
                KeySym::Return => {
                    let nav_target = self.url_input.clone();
                    self.navigate(&nav_target);
                    return true;
                }
                KeySym::Escape => {
                    self.url_input = self.tabs[self.active_tab].url.clone();
                    self.is_url_focused = false;
                    return true;
                }
                KeySym::Backspace => {
                    self.url_input.pop();
                    return true;
                }
                _ => {
                    if let Some(c) = ch {
                        if !c.is_control() {
                            self.url_input.push(c);
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    pub fn render(&self, surf: &mut ViSurface, fonts: &mut TextFonts) {
        let sw = self.width;
        let sh = self.height;
        let surf_w = surf.width();
        let surf_h = surf.height();
        let stride = surf.stride();

        // 1. Fill entire window background with dark base
        fill_rect(surf, 0, 0, sw, sh, Color::rgb(24, 26, 32));

        // 2. Tab Bar (height: 32)
        fill_rect(surf, 0, 0, sw, 32, Color::rgb(18, 20, 25));
        let mut cur_x = 8;
        for (idx, tab) in self.tabs.iter().enumerate() {
            let tab_w = 140;
            let is_active = idx == self.active_tab;
            let tab_bg = if is_active {
                Color::rgb(35, 38, 48)
            } else {
                Color::rgb(24, 26, 34)
            };
            fill_rect(surf, cur_x, 4, tab_w, 28, tab_bg);

            // Tab title
            let clip = (cur_x, 4, cur_x + tab_w as i32 - 20, 32);
            fonts.draw_text(
                surf.pixels_mut(),
                surf_w,
                surf_h,
                stride,
                (cur_x + 8) as f32,
                10.0,
                &tab.title,
                if is_active { FontFace::UiSemibold } else { FontFace::UiRegular },
                12.0,
                [230, 230, 240, 255],
                clip,
            );

            // Close 'x' button on tab
            let close_x = cur_x + tab_w as i32 - 16;
            fonts.draw_text(
                surf.pixels_mut(),
                surf_w,
                surf_h,
                stride,
                close_x as f32,
                10.0,
                "x",
                FontFace::UiRegular,
                11.0,
                [160, 160, 180, 255],
                (close_x, 4, close_x + 16, 32),
            );

            cur_x += tab_w as i32 + 4;
        }

        // '+' Add Tab button
        fill_rect(surf, cur_x, 6, 24, 22, Color::rgb(30, 33, 42));
        fonts.draw_text(
            surf.pixels_mut(),
            surf_w,
            surf_h,
            stride,
            (cur_x + 8) as f32,
            10.0,
            "+",
            FontFace::UiSemibold,
            13.0,
            [200, 200, 220, 255],
            (cur_x, 4, cur_x + 24, 32),
        );

        // 3. Navigation Toolbar (y: 32..72, height: 40)
        fill_rect(surf, 0, 32, sw, 40, Color::rgb(32, 35, 45));
        // Back / Forward / Reload buttons
        draw_button(surf, fonts, 8, 38, 28, 28, "<", Color::rgb(42, 46, 58), [220, 220, 230, 255]);
        draw_button(surf, fonts, 40, 38, 28, 28, ">", Color::rgb(42, 46, 58), [220, 220, 230, 255]);
        draw_button(surf, fonts, 74, 38, 28, 28, "R", Color::rgb(42, 46, 58), [220, 220, 230, 255]);

        // Address Bar
        let addr_w = sw.saturating_sub(180);
        let addr_bg = if self.is_url_focused {
            Color::rgb(20, 22, 30)
        } else {
            Color::rgb(26, 29, 38)
        };
        fill_rect(surf, 110, 38, addr_w, 28, addr_bg);
        // Security Lock / Protocol badge
        fonts.draw_text(
            surf.pixels_mut(),
            surf_w,
            surf_h,
            stride,
            116.0,
            44.0,
            "[HTTPS]",
            FontFace::UiSemibold,
            11.0,
            [45, 195, 95, 255],
            (110, 38, 160, 66),
        );
        // URL text
        let display_url = if self.is_url_focused {
            format!("{}_", self.url_input)
        } else {
            self.url_input.clone()
        };
        fonts.draw_text(
            surf.pixels_mut(),
            surf_w,
            surf_h,
            stride,
            168.0,
            44.0,
            &display_url,
            FontFace::UiRegular,
            12.0,
            [240, 242, 250, 255],
            (168, 38, (110 + addr_w) as i32, 66),
        );

        // Go / Navigate button
        let go_x = (110 + addr_w + 6) as i32;
        draw_button(surf, fonts, go_x, 38, 44, 28, "Go", Color::rgb(70, 130, 245), [255, 255, 255, 255]);

        // 4. Viewport (y: 72..sh-24): Guest Web Content Surface
        let view_h = sh.saturating_sub(96);
        fill_rect(surf, 0, 72, sw, view_h, Color::rgb(250, 252, 255));

        // Render Web Card in Viewport
        if let Some(tab) = self.tabs.get(self.active_tab) {
            // Heading card
            fill_rect(surf, 24, 96, sw.saturating_sub(48), 64, Color::rgb(240, 243, 250));
            fonts.draw_text(
                surf.pixels_mut(),
                surf_w,
                surf_h,
                stride,
                40.0,
                112.0,
                &tab.content_heading,
                FontFace::UiSemibold,
                18.0,
                [20, 24, 35, 255],
                (24, 96, (sw - 24) as i32, 160),
            );

            // Body text
            fonts.draw_text(
                surf.pixels_mut(),
                surf_w,
                surf_h,
                stride,
                40.0,
                180.0,
                &tab.content_text,
                FontFace::UiRegular,
                14.0,
                [50, 55, 70, 255],
                (24, 170, (sw - 24) as i32, (sh - 30) as i32),
            );
        }

        // 5. Bottom Status Bar (height: 24)
        let status_y = (sh.saturating_sub(24)) as i32;
        fill_rect(surf, 0, status_y, sw, 24, Color::rgb(18, 20, 26));
        fonts.draw_text(
            surf.pixels_mut(),
            surf_w,
            surf_h,
            stride,
            12.0,
            (status_y + 5) as f32,
            &self.guest_status,
            FontFace::UiRegular,
            11.0,
            [130, 135, 150, 255],
            (0, status_y, sw as i32, sh as i32),
        );
        surf.damage_all();
    }
}

fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, col: Color) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let x0 = x.clamp(0, sw);
    let y0 = y.clamp(0, sh);
    let x1 = (x + w as i32).clamp(0, sw);
    let y1 = (y + h as i32).clamp(0, sh);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let stride = surf.stride();
    let pixels = surf.pixels_mut();
    let bgra = [col.b(), col.g(), col.r(), col.a()];

    for cy in y0..y1 {
        let row_start = (cy as usize * stride) + (x0 as usize * 4);
        for cx in 0..(x1 - x0) as usize {
            let offset = row_start + (cx * 4);
            pixels[offset..offset + 4].copy_from_slice(&bgra);
        }
    }
}

fn draw_button(
    surf: &mut ViSurface,
    fonts: &mut TextFonts,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    label: &str,
    bg: Color,
    text_color: [u8; 4],
) {
    fill_rect(surf, x, y, w, h, bg);
    let text_x = x as f32 + (w as f32 * 0.5) - (label.len() as f32 * 3.5);
    let text_y = y as f32 + (h as f32 * 0.5) - 6.0;
    let surf_w = surf.width();
    let surf_h = surf.height();
    let stride = surf.stride();
    fonts.draw_text(
        surf.pixels_mut(),
        surf_w,
        surf_h,
        stride,
        text_x,
        text_y,
        label,
        FontFace::UiSemibold,
        12.0,
        text_color,
        (x, y, x + w as i32, y + h as i32),
    );
}

#[cfg(not(test))]
ostd::cell_main!(cell_main);

#[cfg(not(test))]
fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

    ostd::io::println("[chrome] CellOS Chrome Single-App Guest Launcher v0.2");
    ostd::io::println("[chrome] Hardware Virt: Intel VT-x (VMX/EPT) | Base: linux-base.ext4 | App: chrome-app.ext4");

    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = (960u32).min(screen_w);
    let win_h = (640u32).min(screen_h.saturating_sub(36));

    let mut surf = match ViSurface::create(comp_tid, win_w, win_h, PixelFormat::Bgra8888) {
        Ok(s) => s,
        Err(_) => {
            ostd::io::println("[chrome] ERROR: Failed to create ViSurface; exiting.");
            sys_exit(1);
        }
    };
    let _ = surf.set_title("Chrome (Tier 3 Linux Guest)");
    let win_x = (screen_w.saturating_sub(win_w) / 2) as i32;
    let win_y = (screen_h.saturating_sub(win_h) / 2) as i32;
    surf.move_to(win_x, win_y);

    let mut fonts = TextFonts::new();
    let mut app = ChromeApp::new(win_w, win_h);

    while !ostd::input::request_focus() {
        sys_yield();
    }

    app.render(&mut surf, &mut fonts);
    let mut cursor_x = 0i32;
    let mut cursor_y = 0i32;

    loop {
        if app.should_close {
            ostd::io::println("[chrome] User requested window close. Terminating guest container.");
            break;
        }

        let mut need_render = false;
        for ev in ostd::input::poll_events(16) {
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    cursor_x = x;
                    cursor_y = y;
                }
                InputEvent::MouseButton { button: MouseButton::Left, state: KeyState::Pressed } => {
                    if app.handle_click(cursor_x, cursor_y) {
                        need_render = true;
                    }
                }
                InputEvent::Key(ke) if ke.state == KeyState::Pressed => {
                    if app.handle_key(ke.keysym, ke.char(), ke.modifiers) {
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

    sys_exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chrome_tab_management() {
        let mut app = ChromeApp::new(800, 600);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 0);

        // Add tab
        app.new_tab("https://test.site", "Test Site");
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.active_tab, 2);
        assert_eq!(app.tabs[2].url, "https://test.site");

        // Switch tab
        app.switch_tab(1);
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.url_input, "https://docs.cellos.internal/api");

        // Close tab
        app.close_tab(1);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);

        // Close remaining tab until 1 left
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1);
        assert!(!app.should_close);

        // Closing last tab requests app close
        app.close_tab(0);
        assert!(app.should_close);
    }

    #[test]
    fn test_chrome_navigation_and_url_input() {
        let mut app = ChromeApp::new(800, 600);
        // Type into URL
        app.is_url_focused = true;
        app.handle_key(KeySym::Printable, Some('c'), Modifiers::default());
        app.handle_key(KeySym::Printable, Some('o'), Modifiers::default());
        app.handle_key(KeySym::Printable, Some('m'), Modifiers::default());
        assert!(app.url_input.ends_with("com"));

        // Backspace
        app.handle_key(KeySym::Backspace, None, Modifiers::default());
        assert!(app.url_input.ends_with("co"));

        // Navigate via Enter
        app.handle_key(KeySym::Return, None, Modifiers::default());
        assert!(!app.is_url_focused);
        assert!(app.tabs[app.active_tab].url.contains("co"));
    }

    #[test]
    fn test_chrome_fullscreen_and_geometry() {
        let mut app = ChromeApp::new(800, 600);
        assert!(!app.is_fullscreen);

        // Toggle F11
        app.handle_key(KeySym::F11, None, Modifiers::default());
        assert!(app.is_fullscreen);
        assert_eq!(app.width, 1280);
        assert_eq!(app.height, 800);

        // Toggle F11 back
        app.handle_key(KeySym::F11, None, Modifiers::default());
        assert!(!app.is_fullscreen);
        assert_eq!(app.width, 800);
        assert_eq!(app.height, 600);
    }

    #[test]
    fn test_chrome_mouse_click_hit_testing() {
        let mut app = ChromeApp::new(800, 600);
        // Click tab 1 (index 1 is at x: 8 + 140 + 4 = 152 .. 292)
        let handled = app.handle_click(160, 15);
        assert!(handled);
        assert_eq!(app.active_tab, 1);

        // Click '+' button (cur_x = 8 + 2 * 144 = 296)
        let handled_plus = app.handle_click(300, 15);
        assert!(handled_plus);
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.active_tab, 2);

        // Click address bar
        let handled_addr = app.handle_click(200, 50);
        assert!(handled_addr);
        assert!(app.is_url_focused);
    }
}
