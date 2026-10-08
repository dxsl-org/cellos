//! fb-console — mirrors kernel user-log output to the HDMI display.
//!
//! Creates a full-screen background surface in the compositor and renders
//! incoming UTF-8 log bytes with antialiased JetBrains Mono at 16px. Wraps at
//! the right edge and scrolls at the bottom. No UART I/O or VT escape parser.
//!
//! Requires `ReadLog` capability (allowlist bit 54).

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate ostd;
extern crate alloc;

use api::display::PixelFormat;
use alloc::string::String;
use ostd::display::{wait_for_compositor, ViSurface};
use ostd::syscall::{sys_exit, sys_read_log, sys_yield};
use ostd::typography::{is_combining_mark, FontFace, TextFonts};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Log,
    GpuGetResolution,
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    Send,
    Recv,
    LookupService,
    ReadLog
];

// Foreground/background colours (BGRA).
const FG: [u8; 4] = [0xCC, 0xCC, 0xCC, 0xFF]; // light grey
const BG: [u8; 4] = [0x00, 0x00, 0x00, 0xFF]; // black
const FONT_PX: f32 = 16.0;
const FACE: FontFace = FontFace::MonoRegular;

fn cell_extent(metric: f32) -> usize {
    let whole = metric as usize;
    (whole + usize::from(metric > whole as f32)).max(1)
}

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(4 * 1024 * 1024);

ostd::cell_main!(cell_main);

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();
    ostd::io::println("[fb-console] starting");
    let (width, height) = ostd::syscall::sys_get_resolution();
    let width = width as usize;
    let height = height as usize;

    let comp = wait_for_compositor();
    ostd::io::println("[fb-console] compositor found");
    let mut surf = match ViSurface::create_background(
        comp,
        width as u32,
        height as u32,
        PixelFormat::Bgra8888,
    ) {
        Ok(s) => s,
        Err(_) => {
            sys_exit(1);
        }
    };
    ostd::io::println("[fb-console] background surface created");

    // Clear to background.
    let px = surf.pixels_mut();
    for chunk in px.chunks_exact_mut(4) {
        chunk.copy_from_slice(&BG);
    }
    surf.damage_all();
    ostd::io::println("[fb-console] initial damage submitted");

    let mut console = Console::new(&surf);
    let mut decoder = Utf8Decoder::new();
    let mut buf = [0u8; 256];
    let mut first_log_batch = true;

    loop {
        let n = sys_read_log(&mut buf);
        if n == 0 {
            sys_yield();
            continue;
        }
        if first_log_batch {
            first_log_batch = false;
            ostd::io::println("[fb-console] first log batch received");
        }

        let mut dirty = false;
        for &byte in &buf[..n] {
            for c in decoder.push(byte).into_iter().flatten() {
                dirty |= console.write_char(&mut surf, c);
            }
        }
        if dirty {
            surf.damage_all();
        }
    }
}

/// A measured fixed-cell grid. A full final cell wraps only when the next base
/// character arrives, so a combining mark from a later read still rewrites it.
struct Console {
    fonts: TextFonts,
    width: usize,
    height: usize,
    cell_width: usize,
    cell_height: usize,
    cols: usize,
    rows: usize,
    col: usize,
    row: usize,
    // A cell retains at most 32 UTF-8 bytes, including its base and marks.
    cluster: String,
}

impl Console {
    fn new(surf: &ViSurface) -> Self {
        let mut fonts = TextFonts::new();
        let cell_width = cell_extent(fonts.measure(FACE, "M", FONT_PX));
        let cell_height = cell_extent(fonts.line_height(FACE, FONT_PX));
        let width = surf.width() as usize;
        let height = surf.height() as usize;
        Self {
            fonts,
            width,
            height,
            cell_width,
            cell_height,
            cols: (width / cell_width).max(1),
            rows: (height / cell_height).max(1),
            col: 0,
            row: 0,
            cluster: String::with_capacity(32),
        }
    }

    fn write_char(&mut self, surf: &mut ViSurface, c: char) -> bool {
        match c {
            '\n' => {
                self.newline(surf);
                true
            }
            '\r' => {
                self.cluster.clear();
                self.col = 0;
                false
            }
            '\t' => {
                self.cluster.clear();
                // Expand to the next eight-column tab stop, including wrapping.
                if self.col == self.cols {
                    self.newline(surf);
                }
                let spaces = 8 - self.col % 8;
                for _ in 0..spaces {
                    self.write_char(surf, ' ');
                }
                self.cluster.clear();
                true
            }
            '\u{8}' | '\u{7f}' => {
                self.cluster.clear();
                // The shell emits BS, space, BS to erase a cell.
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.cols - 1;
                }
                false
            }
            c if c.is_control() => {
                self.cluster.clear();
                false
            }
            c if is_combining_mark(c) && !self.cluster.is_empty() => {
                if !append_combining_mark(&mut self.cluster, c) {
                    return false;
                }
                self.paint_cluster(surf, self.col - 1);
                true
            }
            _ => {
                if self.col == self.cols {
                    self.newline(surf);
                }
                self.cluster.clear();
                self.cluster.push(c);
                self.paint_cluster(surf, self.col);
                self.col += 1;
                true
            }
        }
    }

    fn newline(&mut self, surf: &mut ViSurface) {
        self.cluster.clear();
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll_up(surf);
            self.row = self.rows - 1;
        }
    }

    fn paint_cluster(&mut self, surf: &mut ViSurface, col: usize) {
        let x = col * self.cell_width;
        let y = self.row * self.cell_height;
        let right = (x + self.cell_width).min(self.width);
        let bottom = (y + self.cell_height).min(self.height);
        let stride = surf.stride();
        let px = surf.pixels_mut();
        // Clear the whole old cell before composing/repainting, not just the
        // new glyph's ink: accents and CR/backspace overwrites leave no residue.
        for row in y..bottom {
            let start = row * stride + x * 4;
            let end = row * stride + right * 4;
            for pixel in px[start..end].chunks_exact_mut(4) {
                pixel.copy_from_slice(&BG);
            }
        }
        // TextFonts composes NFC before painting, including Vietnamese marks
        // received in a different sys_read_log chunk from their base character.
        self.fonts.draw_text(
            px,
            self.width as u32,
            self.height as u32,
            stride,
            x as f32,
            y as f32,
            &self.cluster,
            FACE,
            FONT_PX,
            FG,
            (x as i32, y as i32, right as i32, bottom as i32),
        );
    }

    fn scroll_up(&self, surf: &mut ViSurface) {
        let stride = surf.stride();
        let row_height = self.cell_height.min(self.height);
        let keep_height = (self.rows - 1) * self.cell_height;
        let keep_bytes = keep_height * stride;
        let px = surf.pixels_mut();
        px.copy_within(row_height * stride..row_height * stride + keep_bytes, 0);
        // Also clear any partial grid row below the last complete cell.
        for pixel in px[keep_bytes..self.height * stride].chunks_exact_mut(4) {
            pixel.copy_from_slice(&BG);
        }
    }
}

fn append_combining_mark(cluster: &mut String, c: char) -> bool {
    const MAX_CLUSTER_BYTES: usize = 32;
    if cluster.len() + c.len_utf8() > MAX_CLUSTER_BYTES {
        return false;
    }
    cluster.push(c);
    true
}

/// Streaming UTF-8, retaining incomplete sequences across log reads. Malformed
/// input replaces each maximal invalid prefix with U+FFFD and reprocesses the
/// first non-continuation byte, so an interrupted sequence never eats CR/LF.
struct Utf8Decoder {
    bytes: [u8; 4],
    len: usize,
    expected: usize,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self { bytes: [0; 4], len: 0, expected: 0 }
    }

    fn push(&mut self, byte: u8) -> [Option<char>; 2] {
        let mut replacement = None;
        if self.len > 0 {
            let continuation = (0x80..=0xbf).contains(&byte)
                && (self.len != 1
                    || match self.bytes[0] {
                        0xe0 => byte >= 0xa0,
                        0xed => byte <= 0x9f,
                        0xf0 => byte >= 0x90,
                        0xf4 => byte <= 0x8f,
                        _ => true,
                    });
            if continuation {
                self.bytes[self.len] = byte;
                self.len += 1;
                if self.len == self.expected {
                    let c = core::str::from_utf8(&self.bytes[..self.len])
                        .ok()
                        .and_then(|text| text.chars().next())
                        .unwrap_or('\u{fffd}');
                    self.len = 0;
                    return [Some(c), None];
                }
                return [None, None];
            }
            self.len = 0;
            replacement = Some('\u{fffd}');
        }

        let c = match byte {
            0x00..=0x7f => Some(byte as char),
            0xc2..=0xdf | 0xe0..=0xef | 0xf0..=0xf4 => {
                self.bytes[0] = byte;
                self.len = 1;
                self.expected = match byte {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    _ => 4,
                };
                None
            }
            _ => Some('\u{fffd}'),
        };
        [replacement, c]
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8Decoder;
    use alloc::string::String;

    fn decode(chunks: &[&[u8]]) -> String {
        let mut decoder = Utf8Decoder::new();
        let mut text = String::new();
        for chunk in chunks {
            for &byte in *chunk {
                for c in decoder.push(byte).into_iter().flatten() {
                    text.push(c);
                }
            }
        }
        text
    }

    #[test]
    fn combining_cluster_stays_bounded_and_preserves_vietnamese() {
        let mut cluster = String::from("a");
        assert!(super::append_combining_mark(&mut cluster, '\u{302}'));
        assert!(super::append_combining_mark(&mut cluster, '\u{301}'));
        assert_eq!(ostd::typography::normalized_chars(&cluster).collect::<String>(), "ấ");
        for _ in 0..10_000 {
            super::append_combining_mark(&mut cluster, '\u{301}');
        }
        assert!(cluster.len() <= 32);
        let retained = cluster.clone();
        assert!(!super::append_combining_mark(&mut cluster, '\u{301}'));
        assert_eq!(cluster, retained);
    }

    #[test]
    fn unicode_survives_every_chunk_boundary() {
        let text = "Tiếng Việt: á ề a\u{302}\u{301} 🐈\r\n";
        let bytes = text.as_bytes();
        for split in 0..=bytes.len() {
            assert_eq!(decode(&[&bytes[..split], &[], &bytes[split..]]), text);
        }

        let mut decoder = Utf8Decoder::new();
        let mut decoded = String::new();
        for &byte in bytes {
            for c in decoder.push(byte).into_iter().flatten() {
                decoded.push(c);
            }
        }
        assert_eq!(decoded, text);
    }

    #[test]
    fn incomplete_sequence_waits_for_later_bytes() {
        let mut decoder = Utf8Decoder::new();
        assert_eq!(decoder.push(0xf0), [None, None]);
        assert_eq!(decoder.push(0x9f), [None, None]);
        assert_eq!(decoder.push(0x90), [None, None]);
        assert_eq!(decoder.push(0x88), [Some('🐈'), None]);
        assert_eq!(decoder.push(b'A'), [None, Some('A')]);
    }

    #[test]
    fn interrupted_prefix_preserves_carriage_return_and_line_feed() {
        assert_eq!(decode(&[&[0xe1], &[0x80, b'\r'], b"\n"]), "\u{fffd}\r\n");
        assert_eq!(decode(&[&[0xf0, 0x9f], &[0x90, b'\n']]), "\u{fffd}\n");
        assert_eq!(decode(&[&[0xe2, b'\r', b'\n']]), "\u{fffd}\r\n");
    }

    #[test]
    fn malformed_utf8_replaces_maximal_invalid_prefixes() {
        let cases: &[(&[u8], &str)] = &[
            (&[0x80], "\u{fffd}"),
            (&[0xc0, 0xaf], "\u{fffd}\u{fffd}"),
            (&[0xe0, 0x80, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
            (&[0xed, 0xa0, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
            (&[0xf4, 0x90, 0x80, 0x80], "\u{fffd}\u{fffd}\u{fffd}\u{fffd}"),
            (&[0xf5, 0xff], "\u{fffd}\u{fffd}"),
            (&[0xe2, 0x82, b'A'], "\u{fffd}A"),
            (&[0xe2, 0x82, 0xc3, 0xa9], "\u{fffd}é"),
        ];
        for &(bytes, expected) in cases {
            for split in 0..=bytes.len() {
                assert_eq!(decode(&[&bytes[..split], &bytes[split..]]), expected);
            }
        }
    }
}
