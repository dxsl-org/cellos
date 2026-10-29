//! Host renderer of guest pixels, for scripts/qemu-phase04-gui-test.sh.
//!
//! Three render modes, all drawing with the very same `ostd::typography::TextFonts` the guest
//! cells call (`GlyphAtlas`, BGRA source-over), so expected pixels are the guest's
//! rasterization and not an approximation of it:
//!
//! * Terminal grid: reproduces `cells/apps/terminal/src/main.rs::TerminalApp::render` for
//!   a grid of text rows (JetBrains Mono Regular, 14 px) with the same geometry constants.
//!   The cursor block is not drawn; the runner masks the cursor cell.
//!     usage: phase04-gui-glyphs <width> <height> <rows.txt>
//!     rows.txt: one UTF-8 text row per line (row 0 first). Missing rows are blank.
//!
//! * Styled terminal grid (Phase 05 `cellos-vt1`): reproduces the attribute-aware
//!   `TerminalApp::render` contract. A full-surface DEFAULT_BG fill, then pass 1 fills every
//!   cell (a wide lead spans 2 cells; wide trailers are skipped) whose resolved background
//!   is not DEFAULT_BG, then pass 2 draws every non-blank cell (char plus at most one
//!   combining mark) in JetBrains Mono Regular, or Mono Bold for bold, clipped to its span.
//!   Inverse swaps the resolved fg/bg. The cursor block is not drawn.
//!     usage: phase04-gui-glyphs styled <width> <height> <screen.txt>
//!     screen.txt: one span per line, tab-separated, applied in order (later spans win):
//!       row col fg bg flags text
//!     row/col: 0-based cell of the first character; each character takes one cell
//!       (two with flag `w`); a combining mark (U+0300..036F, U+1AB0..1AFF, U+1DC0..1DFF,
//!       U+20D0..20FF, U+FE20..FE2F) joins the previous character of the span.
//!     fg/bg: `d` (DEFAULT_FG / DEFAULT_BG), `pN` (indexed 0..255: the 16-colour palette,
//!       the 6x6x6 cube, the gray ramp), or `RRGGBB`.
//!     flags: `-`, or any of `b` (bold), `i` (inverse), `w` (wide).
//!     Cells no span covers are blank with default colours.
//!
//! * ViUI surface ops: replays `libs/viui/src/surface.rs` `fill_rect`, `stroke_rect`,
//!   `draw_label_aligned` and `draw_button` (the primitives every Desktop/Files component
//!   is built from) on a `width x height` BGRA surface, at the same surface coordinates the
//!   guest uses, so float glyph placement is bit-identical.
//!     usage: phase04-gui-glyphs ops <width> <height> <ops.txt>
//!     ops.txt: one op per line, tab-separated, colours as RRGGBBAA hex:
//!       fill    x y w h colour
//!       stroke  x y w h colour
//!       label   x y w h face size colour center(0|1) text
//!       button  x y w h bg border fg text
//!     face: UiRegular | UiSemibold | MonoRegular.
//!
//! * Font metrics: `phase04-gui-glyphs lineheight <face> <size>` prints
//!   `TextFonts::line_height` and `(line_height + 1.0) as u32` (Desktop's row geometry).
//!
//! stdout (render modes): width*height*3 bytes, RGB, row-major.

use ostd::typography::{FontFace, TextFonts};
use std::io::Write;

const CELL_W: f32 = 9.0;
const CELL_H: f32 = 18.0;
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const FONT_PX: f32 = 14.0;
const BG: [u8; 3] = [18, 20, 26];
const FG: [u8; 3] = [220, 225, 235];
// Same bounds and math as `terminal::grid_dims` (cells/apps/terminal/src/main.rs).
const MIN_COLS: usize = 40;
const MAX_COLS: usize = 512;
const MIN_ROWS: usize = 12;
const MAX_ROWS: usize = 256;

/// The Terminal's grid for a window: whole cells right of PAD_X and below PAD_Y.
fn grid_dims(width: u32, height: u32) -> (usize, usize) {
    let cols = ((width as f32 - PAD_X).max(0.0) / CELL_W) as usize;
    let rows = ((height as f32 - PAD_Y).max(0.0) / CELL_H) as usize;
    (cols.clamp(MIN_COLS, MAX_COLS), rows.clamp(MIN_ROWS, MAX_ROWS))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pixels_out = match args.get(1).map(String::as_str) {
        Some("ops") if args.len() == 5 => {
            let (w, h) = (args[2].parse().expect("width"), args[3].parse().expect("height"));
            (w, h, surface_ops(w, h, &std::fs::read_to_string(&args[4]).expect("ops file")))
        }
        Some("lineheight") if args.len() == 4 => {
            let mut fonts = TextFonts::new();
            let lh = fonts.line_height(face(&args[2]), args[3].parse().expect("size"));
            println!("{lh:?} {}", (lh + 1.0) as u32);
            return;
        }
        Some("styled") if args.len() == 5 => {
            let (w, h) = (args[2].parse().expect("width"), args[3].parse().expect("height"));
            (w, h, styled_grid(w, h, &std::fs::read_to_string(&args[4]).expect("screen file")))
        }
        _ if args.len() == 4 => {
            let (w, h) = (args[1].parse().expect("width"), args[2].parse().expect("height"));
            (w, h, terminal_grid(w, h, &std::fs::read_to_string(&args[3]).expect("rows file")))
        }
        _ => {
            eprintln!("usage: {0} <width> <height> <rows.txt> | {0} styled <width> <height> <screen.txt> | {0} ops <width> <height> <ops.txt> | {0} lineheight <face> <size>", args[0]);
            std::process::exit(2);
        }
    };
    let (width, height, pixels): (u32, u32, Vec<u8>) = pixels_out;
    let mut out = Vec::with_capacity(width as usize * height as usize * 3);
    for px in pixels.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    std::io::stdout().write_all(&out).expect("stdout");
}

fn terminal_grid(width: u32, height: u32, text: &str) -> Vec<u8> {
    let lines: Vec<&str> = text.split('\n').collect();
    let (cols, rows) = grid_dims(width, height);
    let stride = width as usize * 4;
    let mut pixels = vec![0u8; stride * height as usize];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&[BG[2], BG[1], BG[0], 255]);
    }
    let mut fonts = TextFonts::new();
    for r in 0..rows {
        let row_y = PAD_Y + r as f32 * CELL_H;
        if row_y + CELL_H > height as f32 {
            break;
        }
        let chars: Vec<char> = lines.get(r).map(|l| l.chars().collect()).unwrap_or_default();
        assert!(chars.len() <= cols, "row {r} wider than {cols} columns");
        for c in 0..cols {
            let col_x = PAD_X + c as f32 * CELL_W;
            if col_x + CELL_W > width as f32 {
                break;
            }
            let ch = chars.get(c).copied().unwrap_or(' ');
            if ch != ' ' {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                let clip = (col_x as i32, row_y as i32, (col_x + CELL_W) as i32 + 2, (row_y + CELL_H) as i32);
                fonts.draw_text(
                    &mut pixels, width, height, stride, col_x, row_y, s,
                    FontFace::MonoRegular, FONT_PX, [FG[2], FG[1], FG[0], 255], clip,
                );
            }
        }
    }
    pixels
}

/// The emulator's colour palette 0..15 (r, g, b).
const PALETTE16: [[u8; 3]; 16] = [
    [0, 0, 0], [205, 49, 49], [13, 188, 121], [229, 229, 16],
    [36, 114, 200], [188, 63, 188], [17, 168, 205], [229, 229, 229],
    [102, 102, 102], [241, 76, 76], [35, 209, 139], [245, 245, 67],
    [59, 142, 234], [214, 112, 214], [41, 184, 219], [255, 255, 255],
];

/// SGR 38;5;n / 48;5;n: palette, 6x6x6 cube, gray ramp.
fn indexed(n: u8) -> [u8; 3] {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match n {
        0..=15 => PALETTE16[n as usize],
        16..=231 => {
            let i = (n - 16) as usize;
            [LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6]]
        }
        _ => {
            let v = 8 + 10 * (n - 232);
            [v, v, v]
        }
    }
}

/// `d` | `pN` | `RRGGBB` -> (r, g, b).
fn spec_colour(s: &str, default: [u8; 3]) -> [u8; 3] {
    if s == "d" {
        return default;
    }
    if let Some(n) = s.strip_prefix('p') {
        return indexed(n.parse().unwrap_or_else(|_| panic!("palette index {s} must be 0..255")));
    }
    assert_eq!(s.len(), 6, "colour {s} must be d, pN or RRGGBB");
    let v = u32::from_str_radix(s, 16).expect("colour hex");
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn is_combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

/// One styled-mode cell; `fg`/`bg` are resolved (inverse already applied).
#[derive(Clone, Copy)]
struct StyledCell {
    ch: char,
    mark: Option<char>,
    fg: [u8; 3],
    bg: [u8; 3],
    bold: bool,
    lead: bool,
    trailer: bool,
}

fn styled_grid(width: u32, height: u32, spec: &str) -> Vec<u8> {
    let (cols, rows) = grid_dims(width, height);
    let blank = StyledCell { ch: ' ', mark: None, fg: FG, bg: BG, bold: false, lead: false, trailer: false };
    let mut grid = vec![blank; cols * rows];
    for (n, line) in spec.lines().enumerate().filter(|(_, l)| !l.is_empty()) {
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        assert_eq!(f.len(), 6, "screen line {n}: malformed {line:?}");
        let r: usize = f[0].parse().unwrap_or_else(|_| panic!("screen line {n}: row"));
        let mut c: usize = f[1].parse().unwrap_or_else(|_| panic!("screen line {n}: col"));
        assert!(r < rows, "screen line {n}: row {r} outside {rows} rows");
        let flags = f[4];
        assert!(flags == "-" || flags.chars().all(|x| matches!(x, 'b' | 'i' | 'w')), "screen line {n}: flags {flags:?}");
        let (mut fg, mut bg) = (spec_colour(f[2], FG), spec_colour(f[3], BG));
        if flags.contains('i') {
            std::mem::swap(&mut fg, &mut bg);
        }
        let (bold, wide) = (flags.contains('b'), flags.contains('w'));
        let mut last: Option<usize> = None;
        for ch in f[5].chars() {
            if is_combining(ch) {
                let i = last.unwrap_or_else(|| panic!("screen line {n}: combining mark without a base character"));
                assert!(grid[i].mark.is_none(), "screen line {n}: more than one combining mark on a cell");
                grid[i].mark = Some(ch);
                continue;
            }
            let span = if wide { 2 } else { 1 };
            assert!(c + span <= cols, "screen line {n}: span past column {cols}");
            let i = r * cols + c;
            grid[i] = StyledCell { ch, mark: None, fg, bg, bold, lead: wide, trailer: false };
            if wide {
                grid[i + 1] = StyledCell { ch: ' ', lead: false, trailer: true, ..grid[i] };
            }
            last = Some(i);
            c += span;
        }
    }

    let mut s = Surface { w: width, h: height, px: vec![0u8; width as usize * height as usize * 4], fonts: TextFonts::new() };
    s.fill(0, 0, width, height, [BG[2], BG[1], BG[0], 255]);
    let stride = width as usize * 4;
    // Pass 0: backgrounds; pass 1: glyphs (same loops and breaks as the guest).
    for pass in 0..2 {
        for r in 0..rows {
            let row_y = PAD_Y + r as f32 * CELL_H;
            if row_y + CELL_H > height as f32 {
                break;
            }
            for c in 0..cols {
                let col_x = PAD_X + c as f32 * CELL_W;
                if col_x + CELL_W > width as f32 {
                    break;
                }
                let cell = grid[r * cols + c];
                if cell.trailer {
                    continue;
                }
                let span = if cell.lead { 2.0 } else { 1.0 };
                if pass == 0 {
                    if cell.bg != BG {
                        let bgra = [cell.bg[2], cell.bg[1], cell.bg[0], 255];
                        s.fill(col_x as i32, row_y as i32, (CELL_W * span) as u32, CELL_H as u32, bgra);
                    }
                } else if cell.ch != ' ' || cell.mark.is_some() {
                    let mut text = String::from(cell.ch);
                    text.extend(cell.mark);
                    let face = if cell.bold { FontFace::MonoBold } else { FontFace::MonoRegular };
                    let clip = (col_x as i32, row_y as i32, (col_x + CELL_W * span) as i32 + 2, (row_y + CELL_H) as i32);
                    s.fonts.draw_text(
                        &mut s.px, width, height, stride, col_x, row_y, &text,
                        face, FONT_PX, [cell.fg[2], cell.fg[1], cell.fg[0], 255], clip,
                    );
                }
            }
        }
    }
    s.px
}

fn face(name: &str) -> FontFace {
    match name {
        "UiRegular" => FontFace::UiRegular,
        "UiSemibold" => FontFace::UiSemibold,
        "MonoRegular" => FontFace::MonoRegular,
        other => panic!("unknown face {other}"),
    }
}

/// `RRGGBBAA` -> the BGRA byte order `viui::canvas::Color` hands to the painters.
fn colour(hex: &str) -> [u8; 4] {
    assert_eq!(hex.len(), 8, "colour {hex} must be RRGGBBAA");
    let v = u32::from_str_radix(hex, 16).expect("colour hex");
    [(v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8, v as u8]
}

struct Surface {
    w: u32,
    h: u32,
    px: Vec<u8>,
    fonts: TextFonts,
}

impl Surface {
    /// `viui::surface::fill_rect`.
    fn fill(&mut self, x: i32, y: i32, w: u32, h: u32, c: [u8; 4]) {
        let (sw, sh, stride) = (self.w as i32, self.h as i32, self.w as usize * 4);
        let x0 = x.clamp(0, sw);
        let y0 = y.clamp(0, sh);
        let x1 = x.saturating_add(w as i32).clamp(0, sw);
        let y1 = y.saturating_add(h as i32).clamp(0, sh);
        if x0 >= x1 || y0 >= y1 || c[3] == 0 {
            return;
        }
        let sa = c[3] as u32;
        let inv = 255 - sa;
        for cy in y0..y1 {
            for cx in x0..x1 {
                let o = cy as usize * stride + cx as usize * 4;
                if sa == 255 {
                    self.px[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                } else {
                    for i in 0..3 {
                        self.px[o + i] = ((c[i] as u32 * sa + self.px[o + i] as u32 * inv + 127) / 255) as u8;
                    }
                    self.px[o + 3] = 255;
                }
            }
        }
    }

    /// `viui::surface::stroke_rect`.
    fn stroke(&mut self, x: i32, y: i32, w: u32, h: u32, c: [u8; 4]) {
        self.fill(x, y, w, 1, c);
        self.fill(x, y + h as i32 - 1, w, 1, c);
        self.fill(x, y, 1, h, c);
        self.fill(x + w as i32 - 1, y, 1, h, c);
    }

    /// `viui::surface::draw_label_aligned`.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, x: i32, y: i32, w: u32, h: u32, text: &str, face: FontFace, size: f32, c: [u8; 4], center: bool) {
        let line_h = self.fonts.line_height(face, size);
        let text_y = y as f32 + (h as f32 - line_h) * 0.5;
        let text_x = if center {
            let text_w = self.fonts.measure(face, text, size);
            x as f32 + (w as f32 - text_w) * 0.5
        } else {
            x as f32 + 8.0
        };
        let clip = (x, y, x + w as i32, y + h as i32);
        let (sw, sh) = (self.w, self.h);
        self.fonts.draw_text(&mut self.px, sw, sh, sw as usize * 4, text_x, text_y, text, face, size, c, clip);
    }
}

fn surface_ops(w: u32, h: u32, ops: &str) -> Vec<u8> {
    let mut s = Surface { w, h, px: vec![0u8; w as usize * h as usize * 4], fonts: TextFonts::new() };
    for (n, line) in ops.lines().enumerate().filter(|(_, l)| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let int = |i: usize| -> i32 { f[i].parse().unwrap_or_else(|_| panic!("op line {n}: field {i}")) };
        let (x, y, bw, bh) = (int(1), int(2), int(3) as u32, int(4) as u32);
        match (f[0], f.len()) {
            ("fill", 6) => s.fill(x, y, bw, bh, colour(f[5])),
            ("stroke", 6) => s.stroke(x, y, bw, bh, colour(f[5])),
            ("label", 10) => {
                let size: f32 = f[6].parse().expect("size");
                s.label(x, y, bw, bh, f[9], face(f[5]), size, colour(f[7]), f[8] == "1");
            }
            ("button", 9) => {
                // `viui::surface::draw_button`.
                s.fill(x, y, bw, bh, colour(f[5]));
                s.stroke(x, y, bw, bh, colour(f[6]));
                s.label(x, y, bw, bh, f[8], FontFace::UiSemibold, 13.0, colour(f[7]), true);
            }
            _ => panic!("op line {n}: malformed {line:?}"),
        }
    }
    s.px
}
