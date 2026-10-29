// SPDX-License-Identifier: Apache-2.0 OR MIT
use alloc::string::String;

use ratatui_core::buffer::{Buffer, Cell};
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::symbols::Marker;
use ratatui_core::text::Span;
use ratatui_core::widgets::Widget;
use ratatui_widgets::gauge::LineGauge;
use ratatui_widgets::sparkline::Sparkline;

use super::*;

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn cell(symbol: &str, style: Style) -> Cell {
    let mut cell = Cell::default();
    cell.set_symbol(symbol);
    cell.set_style(style);
    cell
}

#[test]
fn indexed_256_fallback_picks_the_nearest_cube_or_gray_entry() {
    let cases: [((u8, u8, u8), u8); 16] = [
        // Pure primaries and secondaries: cube corners.
        ((0, 0, 0), 16),
        ((255, 255, 255), 231),
        ((255, 0, 0), 196),
        ((0, 255, 0), 46),
        ((0, 0, 255), 21),
        ((255, 255, 0), 226),
        ((0, 255, 255), 51),
        ((255, 0, 255), 201),
        // Grays: the ramp unless a cube gray is exactly as near (the cube wins a tie).
        ((128, 128, 128), 244),
        ((8, 8, 8), 232),
        ((238, 238, 238), 255),
        ((115, 115, 115), 243),
        ((4, 4, 4), 16),
        // Cube edges hit exactly.
        ((95, 95, 95), 59),
        ((95, 135, 175), 67),
        // A palette colour.
        ((203, 166, 247), 183),
    ];
    for ((r, g, b), index) in cases {
        assert_eq!(nearest_256(Rgb(r, g, b)), index, "{r},{g},{b}");
    }
}

#[test]
fn ansi_16_fallback_picks_the_nearest_vt_colour() {
    let cases: [((u8, u8, u8), u8); 12] = [
        ((0, 0, 0), 0),
        ((255, 255, 255), 15),
        ((255, 0, 0), 1),
        ((0, 255, 0), 2),
        ((0, 0, 255), 4),
        ((255, 255, 0), 3),
        ((0, 255, 255), 14),
        ((255, 0, 255), 5),
        ((128, 128, 128), 8),
        ((238, 238, 238), 7),
        ((30, 30, 46), 0),
        ((137, 180, 250), 12),
    ];
    for ((r, g, b), index) in cases {
        assert_eq!(nearest_16(Rgb(r, g, b)), index, "{r},{g},{b}");
    }
    // Every ANSI colour maps to itself.
    for (i, colour) in ANSI16.iter().enumerate() {
        assert_eq!(nearest_16(*colour) as usize, i);
    }
}

#[test]
fn colour_mode_selects_the_representation() {
    let accent = Palette::DEFAULT.accent;
    assert_eq!(Theme::new(ColorMode::TrueColor).color(accent), Color::Rgb(0xcb, 0xa6, 0xf7));
    assert_eq!(Theme::new(ColorMode::Indexed256).color(accent), Color::Indexed(183));
    assert_eq!(Theme::new(ColorMode::Ansi16).color(accent), Color::LightMagenta);
    let ansi = Theme::new(ColorMode::Ansi16);
    assert_eq!(ansi.base(), Style::new().fg(Color::Gray).bg(Color::Black));
    assert_eq!(ansi.color(Rgb(255, 0, 0)), Color::Red);
    assert_eq!(ansi.color(Rgb(128, 128, 128)), Color::DarkGray);
    assert_eq!(Theme::new(ColorMode::Indexed256).base(), Style::new().fg(Color::Indexed(189)).bg(Color::Indexed(235)));
}

#[test]
fn border_sets_are_ratatuis() {
    assert_eq!(Border::Plain.set(), ratatui_core::symbols::border::PLAIN);
    assert_eq!(Border::Rounded.set(), ratatui_core::symbols::border::ROUNDED);
    assert_eq!(Border::Thick.set(), ratatui_core::symbols::border::THICK);
    assert_eq!(Border::Rounded.set().top_left, "\u{256d}");
    assert_eq!(Border::Thick.set().vertical_left, "\u{2503}");
    for border in [Border::Plain, Border::Rounded, Border::Thick] {
        assert_eq!(border.border_type().to_border_set(), border.set());
    }
}

#[test]
fn helpers_produce_the_palette_styles() {
    let theme = Theme::new(ColorMode::TrueColor);
    let p = Palette::DEFAULT;
    let bold = Modifier::BOLD;
    assert_eq!(theme.base(), Style::new().fg(rgb(p.fg)).bg(rgb(p.bg)));
    assert_eq!(theme.border_style(), Style::new().fg(rgb(p.border)));
    assert_eq!(theme.title_style(), Style::new().fg(rgb(p.accent)).add_modifier(bold));
    assert_eq!(theme.title_bar_style(), Style::new().fg(rgb(p.bg)).bg(rgb(p.accent)).add_modifier(bold));
    assert_eq!(theme.status_line_style(), Style::new().fg(rgb(p.fg)).bg(rgb(p.surface)));
    assert_eq!(theme.highlight_style(), Style::new().fg(rgb(p.accent)).bg(rgb(p.highlight)).add_modifier(bold));
    assert_eq!(HIGHLIGHT_SYMBOL, "\u{258c} ");

    let title = theme.title_bar(" app ");
    assert_eq!(title.style, theme.title_bar_style());
    assert_eq!(title.spans, [Span::raw(" app ")]);
    let status = theme.status_line("ready");
    assert_eq!(status.style, theme.status_line_style());
    assert_eq!(status.spans, [Span::raw("ready")]);

    let hints = theme.key_hints(&[("q", "quit"), ("^C", "interrupt")]);
    assert_eq!(hints.style, theme.muted_style());
    assert_eq!(
        hints.spans,
        [
            Span::styled("q", theme.key_style()),
            Span::raw(" "),
            Span::raw("quit"),
            Span::raw("  "),
            Span::styled("^C", theme.key_style()),
            Span::raw(" "),
            Span::raw("interrupt"),
        ]
    );
    assert!(theme.key_hints(&[]).spans.is_empty());

    // Rendered, a bar fills its whole row.
    let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
    theme.title_bar("ab").render(buf.area, &mut buf);
    assert_eq!(row(&buf, 0), "ab      ");
    assert!((0..8).all(|x| buf[(x, 0)].style() == theme.title_bar_style()));
}

#[test]
fn chart_markers_are_block_elements_never_braille() {
    assert_eq!(CHART_MARKER, Marker::Block);
    assert_eq!(CANVAS_MARKER, Marker::HalfBlock);
    for marker in [CHART_MARKER, CANVAS_MARKER] {
        assert_ne!(marker, Marker::Braille);
    }
}

#[test]
fn themed_rounded_block_with_a_bar_renders_exactly() {
    let theme = Theme::new(ColorMode::TrueColor);
    let p = Palette::DEFAULT;
    let area = Rect::new(0, 0, 12, 4);
    let mut buf = Buffer::empty(area);
    let block = theme.block("cpu");
    let inner = block.inner(area);
    block.render(area, &mut buf);
    LineGauge::default()
        .ratio(0.5)
        .label("")
        .filled_symbol("\u{2588}")
        .unfilled_symbol("\u{2591}")
        .filled_style(Style::new().fg(rgb(p.accent)))
        .unfilled_style(Style::new().fg(rgb(p.muted)))
        .render(Rect { height: 1, ..inner }, &mut buf);
    Sparkline::default()
        .data([0u64, 2, 4, 6, 8, 10, 12, 14, 16, 16])
        .max(16)
        .style(Style::new().fg(rgb(p.success)))
        .render(Rect { y: inner.y + 1, height: 1, ..inner }, &mut buf);

    assert_eq!(row(&buf, 0), "╭cpu───────╮");
    assert_eq!(row(&buf, 1), "│ ████░░░░░│");
    assert_eq!(row(&buf, 2), "│ ▁▂▃▄▅▆▇██│");
    assert_eq!(row(&buf, 3), "╰──────────╯");

    let base = theme.base();
    let edge = base.patch(theme.border_style());
    let title = base.patch(theme.title_style());
    let mut expected = Buffer::empty(area);
    for (x, symbol) in "╭cpu───────╮".chars().enumerate() {
        let style = if (1..4).contains(&x) { title } else { edge };
        expected[(x as u16, 0)] = cell(symbol.encode_utf8(&mut [0; 4]), style);
    }
    for y in 1..3 {
        expected[(0, y)] = cell("│", edge);
        expected[(11, y)] = cell("│", edge);
    }
    for (x, symbol) in "╰──────────╯".chars().enumerate() {
        expected[(x as u16, 3)] = cell(symbol.encode_utf8(&mut [0; 4]), edge);
    }
    // Gauge row: the empty label, one gap cell, then floor(9 * 0.5) = 4 filled cells.
    expected[(1, 1)] = cell(" ", base);
    for x in 2..6 {
        expected[(x, 1)] = cell("█", base.fg(rgb(p.accent)));
    }
    for x in 6..11 {
        expected[(x, 1)] = cell("░", base.fg(rgb(p.muted)));
    }
    // Sparkline row: value * 8 / 16 eighths, one cell per value.
    let spark = base.fg(rgb(p.success));
    for (x, symbol) in [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "█"].iter().enumerate() {
        expected[(x as u16 + 1, 2)] = cell(symbol, spark);
    }
    assert_eq!(buf, expected);
}
