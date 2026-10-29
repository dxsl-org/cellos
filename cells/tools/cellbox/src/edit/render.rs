//! Draw the editor into a Ratatui buffer: text rows, a status line and a
//! prompt / message / key-hint line. Control characters are shown escaped
//! (`^X`, `<XX>`), never sent as terminal data; tabs expand to 8-column stops.
//! Long lines scroll horizontally; no bytes are ever inserted for display.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use text_engine::editor::highlight::{highlight, Carry, Kind, Span};
use text_engine::editor::TAB_STOP;
use vitui::ratatui::core::buffer::Buffer;
use vitui::ratatui::core::layout::{Position, Rect};
use vitui::ratatui::core::style::{Modifier, Style};
use vitui::theme::{Theme, ANSI16};

use super::editor::{cell_width, Editor, Mode, PromptKind};

const HELP: [&str; 13] = [
    "Ctrl-S save          Ctrl-O save as      Ctrl-Q quit",
    "Ctrl-Z undo          Ctrl-Y redo",
    "Ctrl-Space mark      Ctrl-C copy         Ctrl-X cut      Ctrl-V paste",
    "Ctrl-F find          Ctrl-N find next    Ctrl-G go to line",
    "Ctrl-H this help     Escape cancel / clear selection",
    "",
    "Saves publish only if the file is unchanged since it was read;",
    "otherwise nothing is written and your edits stay.",
    "Saving needs a mount with conditional save (/srv, /data).",
    "",
    "Arrows, Home/End, PageUp/PageDown move; Enter keeps the",
    "file's own line ending; tabs stay tab bytes.",
    "Press Ctrl-H or Escape to close.",
];

/// One drawn unit: a base scalar with any zero-width scalars after it.
struct Cell {
    text: String,
    byte: usize,
    width: usize,
}

fn cells(text: &str) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::new();
    let mut col = 0;
    for (byte, c) in text.char_indices() {
        let (shown, width) = match c {
            '\t' => {
                let next = (col / TAB_STOP + 1) * TAB_STOP;
                (" ".repeat(next - col), next - col)
            }
            '\0'..='\u{1f}' | '\u{7f}' => (format!("^{}", char::from((c as u8) ^ 0x40)), 2),
            '\u{80}'..='\u{9f}' => (format!("<{:02X}>", c as u32), 4),
            _ => (String::from(c), cell_width(c)),
        };
        match out.last_mut() {
            Some(last) if width == 0 => last.text.push_str(&shown),
            _ => out.push(Cell { text: shown, byte, width }),
        }
        col += width;
    }
    out
}

fn syntax_style(theme: &Theme, kind: Kind) -> Style {
    let rgb = match kind {
        Kind::Comment => ANSI16[8],
        Kind::String => ANSI16[2],
        Kind::Number => ANSI16[5],
        Kind::Keyword => ANSI16[4],
    };
    theme.base().fg(theme.color(rgb))
}

/// Scroll so the cursor is visible, draw, and return the cursor cell.
pub fn draw(editor: &mut Editor, theme: &Theme, area: Rect, out: &mut Buffer) -> Option<Position> {
    out.set_style(area, theme.base());
    let width = usize::from(area.width);
    let rows = usize::from(area.height.saturating_sub(2)).max(1);
    editor.rows = rows;
    let cursor = editor.buf.cursor();
    let cursor_col = editor.buf.column(cursor.line, cursor.byte, &cell_width);
    if cursor.line < editor.top {
        editor.top = cursor.line;
    } else if cursor.line >= editor.top + rows {
        editor.top = cursor.line + 1 - rows;
    }
    if cursor_col < editor.left {
        editor.left = cursor_col;
    } else if cursor_col >= editor.left + width.max(1) {
        editor.left = cursor_col + 1 - width.max(1);
    }

    let lang = editor.lang();
    let selection = editor.buf.selection();
    // Highlight carry from the start of the file to the first visible line.
    let mut carry = Carry::None;
    let mut spans: Vec<Span> = Vec::new();
    if let Some(lang) = lang {
        for line in &editor.buf.lines()[..editor.top.min(editor.buf.line_count())] {
            spans.clear();
            carry = highlight(lang, carry, &line.text, &mut spans);
        }
    }
    if editor.help {
        for (row, text) in HELP.iter().enumerate().take(rows) {
            out.set_stringn(area.x, area.y + row as u16, text, width, theme.base());
        }
    } else {
        for row in 0..rows {
            let index = editor.top + row;
            let Some(line) = editor.buf.lines().get(index) else { break };
            spans.clear();
            if let Some(lang) = lang {
                carry = highlight(lang, carry, &line.text, &mut spans);
            }
            let y = area.y + row as u16;
            let mut col = 0;
            for cell in cells(&line.text) {
                let start = col;
                col += cell.width;
                if col <= editor.left || start >= editor.left + width {
                    continue;
                }
                let x = area.x + (start.saturating_sub(editor.left)) as u16;
                let selected = selection.is_some_and(|(a, b)| {
                    let here = text_engine::editor::Pos { line: index, byte: cell.byte };
                    a <= here && here < b
                });
                let mut style = spans.iter().find(|s| s.start <= cell.byte && cell.byte < s.end)
                    .map_or(theme.base(), |s| syntax_style(theme, s.kind));
                if selected {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                if start < editor.left || col > editor.left + width {
                    // A wide cell cut by an edge shows as blanks, never half a glyph.
                    let visible = col.min(editor.left + width) - start.max(editor.left);
                    out.set_stringn(x, y, " ".repeat(visible), visible, style);
                } else {
                    out.set_stringn(x, y, &cell.text, cell.width, style);
                }
            }
        }
    }

    let name = editor.path().unwrap_or("[unnamed]");
    let status = format!(
        " {name}{} — Ln {}, Col {} ",
        if editor.buf.is_dirty() { " *" } else { "" },
        cursor.line + 1,
        cursor_col + 1,
    );
    let status_y = area.y + rows as u16;
    out.set_style(Rect::new(area.x, status_y, area.width, 1), theme.status_line_style());
    out.set_stringn(area.x, status_y, &status, width, theme.status_line_style());

    let bottom_y = status_y + 1;
    let (bottom, prompt_cursor) = match &editor.mode {
        Mode::Prompt(kind, input) => {
            let label = match kind {
                PromptKind::SaveAs => "Save as: ",
                PromptKind::Find => "Find: ",
                PromptKind::GoTo => "Go to line: ",
            };
            let text = format!("{label}{input}");
            let col = text.chars().map(cell_width).sum::<usize>();
            (text, Some(col))
        }
        Mode::DirtyExit => (String::from("Unsaved changes: (s)ave, (d)iscard, (c)ancel"), None),
        Mode::Overwrite { path, .. } => (format!("{path} exists: overwrite? (y/n)"), None),
        Mode::Edit => (
            editor.message.clone().unwrap_or_else(|| String::from("^S save  ^O save as  ^Q quit  ^H help")),
            None,
        ),
    };
    if area.height >= 2 {
        out.set_stringn(area.x, bottom_y, &bottom, width, theme.base());
    }
    match prompt_cursor {
        Some(col) if area.height >= 2 => Some(Position::new(area.x + col.min(width.saturating_sub(1)) as u16, bottom_y)),
        _ if editor.help || !matches!(editor.mode, Mode::Edit) => None,
        _ => Some(Position::new(
            area.x + (cursor_col - editor.left) as u16,
            area.y + (cursor.line - editor.top) as u16,
        )),
    }
}
