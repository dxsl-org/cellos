// SPDX-License-Identifier: MIT
//! CellOS Native GUI Terminal Cell: monospace grid over a real `cellbox sh` that controls
//! its own session, attached through `terminal_sdk::FrontendHost`.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

extern crate alloc;
use alloc::format;

mod clipboard;
mod grid;
mod keymap;
#[cfg(test)]
mod roundtrip;
mod selection;
mod session;
mod sink;

use ostd::display::linear::icons::LineIcon;
use ostd::display::{poll_surface_events, SurfaceEvent, ViSurface};
use ostd::input::{InputEvent, KeyState, MouseButton};
use ostd::syscall::{sys_get_resolution, sys_set_timer, sys_yield};
use ostd::typography::{FontFace, TextFonts};
use viui::canvas::Color;
use viui::surface::fill_rect;
use viui::theme::{self, ViTheme};
use wire::display::PixelFormat;

use crate::grid::{Color as GridColor, TerminalCell, Width};
use crate::keymap::{clip_chord, ClipChord};
use crate::selection::{Geometry, Selector};
use crate::session::{Pasted, TerminalSession, MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS};

abi::declare_manifest!(block_io = false, network = false, spawn = true);
abi::declare_syscalls![
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
    // FrontendHost reaping and the idle backoff below sleep one tick at a time.
    SetTimer
];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

pub struct TerminalApp {
    pub session: TerminalSession,
    pub width: u32,
    pub height: u32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub selector: Selector,
}

impl TerminalApp {
    pub fn new(width: u32, height: u32) -> Self {
        let cell_w = 9.0;
        let cell_h = 18.0;
        let (cols, rows) = grid_dims(width, height, cell_w, cell_h);

        Self {
            session: TerminalSession::new(cols, rows),
            width,
            height,
            cell_w,
            cell_h,
            selector: Selector::default(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let (cols, rows) = grid_dims(width, height, self.cell_w, self.cell_h);
        self.session.resize(cols, rows);
        self.selector.clear();
    }

    pub fn render(&self, surf: &mut ViSurface, fonts: &mut TextFonts) {
        let colors = TermColors::of(theme::system());
        let sw = self.width;
        let sh = self.height;
        let surf_w = surf.width();
        let surf_h = surf.height();
        let stride = surf.stride();
        let grid = &self.session.grid;
        fill_rect(surf, 0, 0, sw, sh, colors.bg);

        // Pass 1: cell backgrounds that differ from the window background. All fills come
        // before any glyph so a glyph's 2 px clip overhang is never painted over.
        for (col_x, row_y, cell, span, selected) in self.visible_cells() {
            let (_, bg) = cell_colors(cell, colors, selected);
            if bg != colors.bg {
                fill_rect(
                    surf,
                    col_x as i32,
                    row_y as i32,
                    (self.cell_w * span as f32) as u32,
                    self.cell_h as u32,
                    bg,
                );
            }
        }

        // Pass 2: glyphs (character plus its combining mark), bold in the bold face.
        for (col_x, row_y, cell, span, selected) in self.visible_cells() {
            if cell.ch == ' ' && cell.mark.is_none() {
                continue;
            }
            let mut buf = [0u8; 8];
            let len = cell.ch.encode_utf8(&mut buf).len();
            let len = len
                + cell
                    .mark
                    .map_or(0, |mark| mark.encode_utf8(&mut buf[len..]).len());
            let text = core::str::from_utf8(&buf[..len]).unwrap_or("\u{fffd}");
            let (fg, _) = cell_colors(cell, colors, selected);
            let face = if cell.pen.bold {
                FontFace::MonoBold
            } else {
                FontFace::MonoRegular
            };
            let clip = (
                col_x as i32,
                row_y as i32,
                (col_x + self.cell_w * span as f32) as i32 + 2,
                (row_y + self.cell_h) as i32,
            );
            fonts.draw_text(
                surf.pixels_mut(),
                surf_w,
                surf_h,
                stride,
                col_x,
                row_y,
                text,
                face,
                FONT_PX,
                [fg.b(), fg.g(), fg.r(), fg.a()],
                clip,
            );
        }

        // Cursor block, unless the program hid it (?25l).
        if grid.cursor_visible() {
            let cur_x = PAD_X + grid.cursor_x as f32 * self.cell_w;
            let cur_y = PAD_Y + grid.cursor_y as f32 * self.cell_h;
            if cur_x + self.cell_w <= sw as f32 && cur_y + self.cell_h <= sh as f32 {
                fill_rect(
                    surf,
                    cur_x as i32,
                    cur_y as i32 + 2,
                    self.cell_w as u32,
                    (self.cell_h - 4.0) as u32,
                    colors.cursor,
                );
            }
        }
        surf.damage_all();
    }

    /// Every cell that fits the window and draws something itself (no wide-character
    /// trailing halves), with its pixel origin, column span and whether it is selected.
    fn visible_cells(&self) -> impl Iterator<Item = (f32, f32, &TerminalCell, usize, bool)> + '_ {
        let grid = &self.session.grid;
        let selection = self.selector.selection();
        let (sw, sh) = (self.width as f32, self.height as f32);
        let (cell_w, cell_h) = (self.cell_w, self.cell_h);
        (0..grid.rows)
            .map(move |r| (r, PAD_Y + r as f32 * cell_h))
            .take_while(move |&(_, row_y)| row_y + cell_h <= sh)
            .flat_map(move |(r, row_y)| {
                (0..grid.cols)
                    .map(move |c| (c, PAD_X + c as f32 * cell_w))
                    .take_while(move |&(_, col_x)| col_x + cell_w <= sw)
                    .filter_map(move |(c, col_x)| {
                        let cell = grid.cell_at(c, r)?;
                        let span = match cell.width {
                            Width::Trail => return None,
                            Width::Wide => 2,
                            Width::Narrow => 1,
                        };
                        let selected = selection.is_some_and(|s| s.contains(c, r));
                        Some((col_x, row_y, cell, span, selected))
                    })
            })
    }

    /// Where the grid sits in the window, for mouse selection.
    fn geometry(&self) -> Geometry {
        Geometry {
            cell_w: self.cell_w,
            cell_h: self.cell_h,
            cols: self.session.grid.cols,
            rows: self.session.grid.rows,
        }
    }

    /// Ctrl+Shift+C: the selection's text onto the clipboard; no selection, nothing.
    fn copy_selection(&self, surf: &ViSurface) {
        if let Some(selection) = self.selector.selection() {
            clipboard::copy(surf, &selection.text(&self.session.grid));
        }
    }

    /// Paste clipboard text into the session; returns whether the grid changed.
    fn paste(&mut self, text: &str) -> bool {
        match self.session.paste(text) {
            Pasted::Sent => {
                ostd::io::println(&format!("[terminal] pasted {} bytes", text.len()));
                false
            }
            Pasted::Dropped(Some(status)) => {
                ostd::io::println(&format!("[terminal] paste refused: {:?}", status));
                false
            }
            Pasted::Dropped(None) => false,
            Pasted::Failed => true,
        }
    }
}

/// Default text, window background and cursor colours, taken from the session palette so
/// the Terminal matches the desktop; explicit ANSI/indexed/RGB colours keep their values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TermColors {
    fg: Color,
    bg: Color,
    /// Translucent block over the cursor cell, so the glyph under it stays visible.
    cursor: Color,
}

impl TermColors {
    const CURSOR_ALPHA: u8 = 160;

    fn of(theme: &dyn ViTheme) -> Self {
        Self {
            fg: theme.text_primary(),
            bg: theme.bg(),
            cursor: theme.accent().with_alpha(Self::CURSOR_ALPHA),
        }
    }
}

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const FONT_PX: f32 = 14.0;

/// Grid size for a window: the whole cells that fit right of `PAD_X` and below `PAD_Y`,
/// where the renderer places cell (0, 0), so every cell of the grid is drawn in full and
/// the session is told exactly the drawn size. Clamped to what the session accepts; only
/// a window below the session minimum (40x12 cells) loses its trailing cells.
pub fn grid_dims(width: u32, height: u32, cell_w: f32, cell_h: f32) -> (usize, usize) {
    let cols = ((width as f32 - PAD_X).max(0.0) / cell_w) as usize;
    let rows = ((height as f32 - PAD_Y).max(0.0) / cell_h) as usize;
    (
        cols.clamp(MIN_COLS, MAX_COLS),
        rows.clamp(MIN_ROWS, MAX_ROWS),
    )
}

/// The 16 ANSI colours (SGR 30-37 / 90-97 and indexes 0-15).
const ANSI: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 49, 49),
    (13, 188, 121),
    (229, 229, 16),
    (36, 114, 200),
    (188, 63, 188),
    (17, 168, 205),
    (229, 229, 229),
    (102, 102, 102),
    (241, 76, 76),
    (35, 209, 139),
    (245, 245, 67),
    (59, 142, 234),
    (214, 112, 214),
    (41, 184, 219),
    (255, 255, 255),
];

/// Pixel colour of a cell colour: ANSI 16, the 6x6x6 cube (16-231), the grey ramp
/// (232-255) or RGB.
fn resolve(color: GridColor, default: Color) -> Color {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match color {
        GridColor::Default => default,
        GridColor::Rgb(r, g, b) => Color::rgb(r, g, b),
        GridColor::Indexed(n @ 0..=15) => {
            let (r, g, b) = ANSI[n as usize];
            Color::rgb(r, g, b)
        }
        GridColor::Indexed(n @ 16..=231) => {
            let i = (n - 16) as usize;
            Color::rgb(LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6])
        }
        GridColor::Indexed(n) => {
            let v = 8 + 10 * (n - 232);
            Color::rgb(v, v, v)
        }
    }
}

/// Foreground and background pixels of a cell; inverse swaps the resolved pair, and so
/// does selection (a selected inverse cell shows its plain colours).
fn cell_colors(cell: &TerminalCell, colors: TermColors, selected: bool) -> (Color, Color) {
    let fg = resolve(cell.pen.fg, colors.fg);
    let bg = resolve(cell.pen.bg, colors.bg);
    if cell.pen.inverse != selected {
        (bg, fg)
    } else {
        (fg, bg)
    }
}

/// The Terminal window. Closing it ends the shell and the Cell.
fn open_window(comp_tid: usize, width: u32, height: u32) -> ViSurface {
    let surf = ViSurface::create(comp_tid, width, height, PixelFormat::Bgra8888)
        .expect("create terminal surface");
    let _ = surf.set_title("Terminal — CellOS Shell");
    if surf.set_icon(LineIcon::Terminal).is_err() {
        ostd::io::println("[terminal] set_icon failed; Cell Tab keeps the generic icon");
    }
    surf.move_to(140, 100);
    surf
}

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

    ostd::io::println("[terminal] CellOS GUI Terminal initializing...");
    let comp_tid = ostd::display::wait_for_compositor();

    let (mut screen_w, mut screen_h) = sys_get_resolution();
    if screen_w == 0 || screen_h == 0 || screen_w > 4096 || screen_h > 4096 {
        screen_w = 1280;
        screen_h = 800;
    }

    let win_w = 720.min(screen_w);
    let win_h = 480.min(screen_h - 36);

    let mut surf = open_window(comp_tid, win_w, win_h);

    let mut app = TerminalApp::new(win_w, win_h);
    let mut fonts = TextFonts::new();

    while !ostd::input::request_focus() {
        sys_yield();
    }

    // The shell's own prompt and output are the first thing on screen.
    app.session.start();
    // Palette generation the last frame was painted with; a change repaints everything.
    let mut painted_serial = theme::system_serial();
    app.render(&mut surf, &mut fonts);
    ostd::io::println("[terminal] Terminal surface ready. Entering event loop.");
    let mut paste = clipboard::PasteRequest::default();

    loop {
        let mut need_render = false;
        let mut busy = false;

        for ev in ostd::input::poll_events(16) {
            busy = true;
            let geom = app.geometry();
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    need_render |= app.selector.on_move(x, y, geom);
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Pressed,
                } => {
                    let _ = ostd::input::request_focus();
                    need_render |= app.selector.on_press(geom);
                }
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state: KeyState::Released,
                } => app.selector.on_release(),
                // Clipboard chords are the Terminal's: never mapped to shell input.
                InputEvent::Key(ke) => match clip_chord(&ke) {
                    Some(_) if ke.state != KeyState::Pressed => {}
                    Some(ClipChord::Copy) => app.copy_selection(&surf),
                    Some(ClipChord::Paste) => paste.request(&surf),
                    None => need_render |= app.session.on_key(&ke),
                },
                _ => {}
            }
        }

        let mut close_serial = None;
        for ev in poll_surface_events(8) {
            match ev {
                SurfaceEvent::Configure(configure)
                    if configure.cap == surf.cap() && surf.apply_configure(configure).is_ok() =>
                {
                    app.resize(surf.width(), surf.height());
                    need_render = true;
                    busy = true;
                }
                SurfaceEvent::CloseRequest(request) if request.cap == surf.cap() => {
                    close_serial = Some(request.serial);
                }
                _ => {}
            }
        }

        if let Some(text) = paste.take_text(&surf) {
            need_render |= app.paste(&text);
            busy = true;
        }

        if let Some(serial) = close_serial {
            let _ = surf.respond_close(serial, true);
            break;
        }

        if app.session.pump() {
            need_render = true;
            busy = true;
        }

        if app.session.should_close {
            break;
        }

        let serial = theme::system_serial();
        if serial != painted_serial {
            painted_serial = serial;
            need_render = true;
        }
        if need_render {
            app.render(&mut surf, &mut fonts);
        }

        if busy {
            sys_yield();
        } else {
            // Idle: sleep one tick instead of spinning on the session RPCs.
            let _ = sys_set_timer(1);
        }
    }

    // Window first, then the shell, then the log line: the console repaint for the log
    // must not race the window's teardown repaint.
    surf.destroy();
    app.session.shutdown();
    ostd::io::println("[terminal] Window closed; shell ended; exiting.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_app_sizes_the_grid_from_the_window_and_tracks_resize() {
        let mut app = TerminalApp::new(720, 480);
        assert_eq!((app.session.grid.cols, app.session.grid.rows), (79, 26));
        assert!(!app.session.should_close);

        app.resize(900, 600);
        assert_eq!((app.session.grid.cols, app.session.grid.rows), (99, 33));
        assert_eq!((app.width, app.height), (900, 600));
        assert_eq!(app.visible_cells().count(), 99 * 33);

        // Tiny windows never shrink the grid below the usable minimum.
        app.resize(10, 10);
        assert!(app.session.grid.cols >= 40);
        assert!(app.session.grid.rows >= 12);
    }

    #[test]
    fn grid_dims_follow_the_window_minus_padding_and_stay_inside_the_session_limits() {
        assert_eq!(grid_dims(720, 480, 9.0, 18.0), (79, 26));
        assert_eq!(grid_dims(100, 100, 9.0, 18.0), (MIN_COLS, MIN_ROWS));
        assert_eq!(grid_dims(4096, 4096, 9.0, 18.0), (454, 227));
        assert_eq!(grid_dims(60_000, 60_000, 9.0, 18.0), (MAX_COLS, MAX_ROWS));
        let (cols, rows) = grid_dims(60_000, 60_000, 9.0, 18.0);
        assert!(wire::session::validate_dimensions(rows as u16, cols as u16).is_ok());
    }

    #[test]
    fn every_grid_cell_is_drawn_in_full_and_one_more_would_not_fit() {
        // Exact fits (728 = 8 + 80 * 9, 474 = 6 + 26 * 18), one pixel short of them, and
        // ordinary sizes.
        for (w, h) in [
            (720, 480),
            (727, 473),
            (728, 474),
            (729, 475),
            (800, 600),
            (900, 600),
            (1280, 764),
            (4096, 4096),
        ] {
            let mut app = TerminalApp::new(w, h);
            let (cols, rows) = (app.session.grid.cols, app.session.grid.rows);
            let (cell_w, cell_h) = (app.cell_w, app.cell_h);
            assert!(
                PAD_X + cols as f32 * cell_w <= w as f32,
                "{w}x{h}: {cols} columns overflow"
            );
            assert!(
                PAD_X + (cols + 1) as f32 * cell_w > w as f32,
                "{w}x{h}: column {} would fit",
                cols + 1
            );
            assert!(
                PAD_Y + rows as f32 * cell_h <= h as f32,
                "{w}x{h}: {rows} rows overflow"
            );
            assert!(
                PAD_Y + (rows + 1) as f32 * cell_h > h as f32,
                "{w}x{h}: row {} would fit",
                rows + 1
            );
            // The renderer draws every cell the grid (and so the session) has, the last
            // column and row included.
            assert_eq!(app.visible_cells().count(), cols * rows, "{w}x{h}");
            let last = (
                PAD_X + (cols - 1) as f32 * cell_w,
                PAD_Y + (rows - 1) as f32 * cell_h,
            );
            assert_eq!(
                app.visible_cells().last().map(|(x, y, ..)| (x, y)),
                Some(last),
                "{w}x{h}"
            );
            // The resize path sizes the grid with the same function.
            app.resize(w + 1, h + 1);
            app.resize(w, h);
            assert_eq!(
                (app.session.grid.cols, app.session.grid.rows),
                (cols, rows),
                "{w}x{h} after resize"
            );
        }
        assert_eq!(grid_dims(727, 473, 9.0, 18.0), (79, 25));
        assert_eq!(grid_dims(728, 474, 9.0, 18.0), (80, 26));
    }

    #[test]
    fn colours_resolve_to_the_documented_palette() {
        let default = Color::rgb(9, 9, 9);
        assert_eq!(resolve(GridColor::Default, default), default);
        assert_eq!(
            resolve(GridColor::Indexed(1), default),
            Color::rgb(205, 49, 49)
        );
        assert_eq!(
            resolve(GridColor::Indexed(15), default),
            Color::rgb(255, 255, 255)
        );
        assert_eq!(
            resolve(GridColor::Indexed(16), default),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(
            resolve(GridColor::Indexed(196), default),
            Color::rgb(255, 0, 0)
        );
        assert_eq!(
            resolve(GridColor::Indexed(231), default),
            Color::rgb(255, 255, 255)
        );
        assert_eq!(
            resolve(GridColor::Indexed(232), default),
            Color::rgb(8, 8, 8)
        );
        assert_eq!(
            resolve(GridColor::Indexed(255), default),
            Color::rgb(238, 238, 238)
        );
        assert_eq!(
            resolve(GridColor::Rgb(1, 2, 3), default),
            Color::rgb(1, 2, 3)
        );
    }

    #[test]
    fn default_colours_follow_the_theme_and_explicit_colours_do_not() {
        let light = TermColors::of(&viui::theme::LightTheme);
        assert_eq!(light.fg, Color::rgb(20, 20, 28));
        assert_eq!(light.bg, Color::rgb(245, 245, 248));
        assert_eq!(light.cursor, Color::rgb(35, 90, 215).with_alpha(160));
        let dark = TermColors::of(&viui::theme::DarkTheme);
        assert_ne!(light, dark);

        for colors in [light, dark] {
            // Default cell: theme text on theme background; inverse swaps them, so an
            // inverse default cell paints a text-coloured block on a light theme too.
            let mut cell = TerminalCell::BLANK;
            assert_eq!(cell_colors(&cell, colors, false), (colors.fg, colors.bg));
            // Selection swaps the pair too, and undoes inverse.
            assert_eq!(cell_colors(&cell, colors, true), (colors.bg, colors.fg));
            cell.pen.inverse = true;
            assert_eq!(cell_colors(&cell, colors, false), (colors.bg, colors.fg));
            assert_eq!(cell_colors(&cell, colors, true), (colors.fg, colors.bg));
            assert_ne!(cell_colors(&cell, colors, false).1, colors.bg);

            // Explicit colours keep their values whatever the theme.
            cell.pen.inverse = false;
            cell.pen.fg = GridColor::Indexed(1);
            cell.pen.bg = GridColor::Rgb(1, 2, 3);
            assert_eq!(
                cell_colors(&cell, colors, false),
                (Color::rgb(205, 49, 49), Color::rgb(1, 2, 3))
            );
            cell.pen.inverse = true;
            assert_eq!(
                cell_colors(&cell, colors, false),
                (Color::rgb(1, 2, 3), Color::rgb(205, 49, 49))
            );
        }
    }

    #[test]
    fn visible_cells_skip_trailing_halves_and_stop_at_the_window_edge() {
        let mut app = TerminalApp::new(720, 480);
        app.session.grid.write_str("a\u{4e16}b");
        let row0: std::vec::Vec<_> = app
            .visible_cells()
            .take(3)
            .map(|(x, y, cell, span, _)| (x, y, cell.ch, span))
            .collect();
        assert_eq!(
            row0,
            [
                (8.0, 6.0, 'a', 1),
                (17.0, 6.0, '\u{4e16}', 2),
                (35.0, 6.0, 'b', 1)
            ]
        );
        // A drag onto a wide character's trailing half selects the character.
        let geom = app.geometry();
        app.selector.on_move(8, 6, geom);
        app.selector.on_press(geom);
        app.selector.on_move(8 + 9 * 2, 6, geom);
        let selected: std::vec::Vec<_> = app.visible_cells().take(4).map(|c| c.4).collect();
        assert_eq!(selected, [true, true, false, false]);
        // 720 px fit 79 whole columns after the 8 px pad, the grid's own width; 26 rows;
        // minus one trailing half.
        assert_eq!(app.visible_cells().count(), 79 * 26 - 1);
    }
}
