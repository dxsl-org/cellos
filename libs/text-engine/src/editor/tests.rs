//! Data-loss and boundary regressions for the edit buffer.

use super::highlight::{highlight, Carry, Kind, Lang, Span};
use super::*;

fn open(text: &str) -> EditBuffer {
    EditBuffer::from_bytes(text.as_bytes()).unwrap()
}

fn text(buf: &EditBuffer) -> String {
    String::from_utf8(buf.to_bytes()).unwrap()
}

/// One column per scalar except CJK-wide (2) and combining marks (0).
fn width(c: char) -> usize {
    match c {
        '\u{0300}'..='\u{036f}' => 0,
        '\u{4e00}'..='\u{9fff}' => 2,
        _ => 1,
    }
}

#[test]
fn stored_bytes_reopen_exactly() {
    for doc in ["", "\n", "\r\n", "a", "a\n", "a\r\nb\nc", "lone\rcr\n", "x\r", "\t tab\r\n\r\n", "é😀\u{301}\n\n"] {
        let buf = open(doc);
        assert_eq!(text(&buf), doc, "{doc:?}");
        assert_eq!(buf.len(), doc.len());
        assert_eq!(buf.line_count(), doc.matches('\n').count() + 1);
    }
}

#[test]
fn unsupported_input_is_refused_not_altered() {
    assert_eq!(EditBuffer::from_bytes(b"a\xffb").err(), Some(OpenError::InvalidUtf8));
    assert_eq!(EditBuffer::from_bytes(b"a\0b").err(), Some(OpenError::Nul));
    assert_eq!(EditBuffer::from_bytes(&vec![b'x'; MAX_BYTES + 1]).err(), Some(OpenError::TooLarge));
    assert_eq!(EditBuffer::from_bytes(&vec![b'\n'; MAX_LINES]).err(), Some(OpenError::TooManyLines));
    assert!(EditBuffer::from_bytes(&vec![b'\n'; MAX_LINES - 1]).is_ok());
}

#[test]
fn backspace_and_delete_remove_whole_scalars_and_whole_terminators() {
    let mut buf = open("aé\r\nb");
    buf.end();
    buf.backspace().unwrap();
    assert_eq!(text(&buf), "a\r\nb");
    buf.delete().unwrap();
    assert_eq!(text(&buf), "ab", "CRLF is one deletion unit");
    buf.newline().unwrap();
    assert_eq!(text(&buf), "a\nb", "no terminator left anywhere: Enter inserts LF");
    buf.backspace().unwrap();
    assert_eq!(text(&buf), "ab");
    buf.home();
    buf.backspace().unwrap();
    assert_eq!(text(&buf), "ab", "backspace at the start is a no-op");
}

#[test]
fn enter_picks_the_nearest_terminator_and_keeps_mixed_endings() {
    let mut buf = open("a\nb\r\nc");
    buf.go_to_line(3);
    buf.end();
    buf.newline().unwrap();
    assert_eq!(text(&buf), "a\nb\r\nc\r\n");
    buf.go_to_line(1);
    buf.end();
    buf.newline().unwrap();
    assert_eq!(text(&buf), "a\n\nb\r\nc\r\n");
    let mut fresh = EditBuffer::new();
    fresh.insert("x").unwrap();
    fresh.newline().unwrap();
    assert_eq!(text(&fresh), "x\n");
}

#[test]
fn bytes_that_join_into_a_terminator_stay_exact_and_undo_restores_them() {
    let mut buf = open("a\nb");
    buf.end();
    buf.insert("\r").unwrap();
    assert_eq!(text(&buf), "a\r\nb");
    assert_eq!(buf.cursor(), Pos { line: 1, byte: 0 }, "never inside a terminator");
    assert!(buf.undo());
    assert_eq!(text(&buf), "a\nb");
    assert_eq!(buf.cursor(), Pos { line: 0, byte: 1 });
    assert!(buf.redo());
    assert_eq!(text(&buf), "a\r\nb");
}

#[test]
fn undo_redo_restore_bytes_and_dirty_tracks_the_saved_state() {
    let mut buf = open("one\ntwo");
    buf.mark_saved();
    buf.end();
    buf.insert("!").unwrap();
    buf.toggle_anchor();
    buf.home();
    buf.cut().unwrap();
    assert_eq!(text(&buf), "\ntwo");
    assert!(buf.is_dirty());
    assert!(buf.undo());
    assert_eq!(text(&buf), "one!\ntwo");
    assert!(buf.undo());
    assert_eq!(text(&buf), "one\ntwo");
    assert!(!buf.is_dirty(), "back at the saved bytes");
    assert!(buf.redo());
    buf.mark_saved();
    // A new edit drops the redo branch.
    buf.insert("?").unwrap();
    assert_eq!(buf.undo_depth().1, 0);
    assert!(!buf.redo());
    assert!(buf.undo());
    assert!(!buf.is_dirty());
}

#[test]
fn undo_evicts_oldest_groups_at_both_limits_and_refuses_oversized_groups() {
    let mut buf = EditBuffer::new();
    for _ in 0..MAX_UNDO_GROUPS + 5 {
        buf.insert("x").unwrap();
    }
    assert_eq!(buf.undo_depth(), (MAX_UNDO_GROUPS, 0));
    let mut buf = EditBuffer::new();
    let chunk = "y".repeat(MAX_CLIPBOARD);
    for _ in 0..5 {
        buf.paste_text(&chunk).unwrap();
    }
    assert!(buf.retained_undo_bytes() <= MAX_UNDO_BYTES);
    assert_eq!(buf.undo_depth().0, MAX_UNDO_BYTES / MAX_CLIPBOARD);
    while buf.undo() {}
    assert_eq!(buf.len(), 5 * MAX_CLIPBOARD - 4 * MAX_CLIPBOARD, "only retained groups undo");
    // Deleting more than the undo budget at once is refused with nothing changed.
    let mut big = open(&"z".repeat(MAX_UNDO_BYTES + 1));
    big.toggle_anchor();
    big.end();
    let before = big.clone();
    assert_eq!(big.delete(), Err(EditError::UndoBudget));
    assert_eq!(big.to_bytes(), before.to_bytes());
    assert_eq!(big.selection(), before.selection());
    assert_eq!(big.undo_depth(), before.undo_depth());
}

#[test]
fn rejected_edits_change_nothing() {
    let mut buf = open("keep");
    buf.end();
    let before = (buf.to_bytes(), buf.cursor(), buf.undo_depth());
    assert_eq!(buf.insert("a\0b"), Err(EditError::Nul));
    assert_eq!(buf.paste_text(&"p".repeat(MAX_CLIPBOARD + 1)), Err(EditError::ClipboardFull));
    assert_eq!((buf.to_bytes(), buf.cursor(), buf.undo_depth()), before);
    let mut full = open(&"\n".repeat(MAX_LINES - 1));
    assert_eq!(full.newline(), Err(EditError::TooManyLines));
}

#[test]
fn selection_copy_cut_paste_replace_and_keep_the_clipboard_private() {
    let mut buf = open("hello wide 世界\r\nnext");
    buf.toggle_anchor();
    for _ in 0..5 {
        buf.right();
    }
    assert_eq!(buf.copy(), Ok(true));
    assert_eq!(buf.clipboard(), "hello");
    assert!(buf.selection().is_some(), "copy keeps the selection");
    buf.end();
    buf.right();
    buf.clear_selection();
    buf.paste().unwrap();
    assert_eq!(text(&buf), "hello wide 世界\r\nhellonext");
    // Selection across the CRLF, replaced by typing.
    buf.go_to_line(1);
    buf.end();
    buf.left();
    buf.toggle_anchor();
    buf.right();
    buf.right();
    buf.insert("+").unwrap();
    assert_eq!(text(&buf), "hello wide 世+hellonext");
    assert_eq!(buf.copy(), Ok(false), "no selection after an edit");
}

#[test]
fn vertical_motion_keeps_the_desired_column_across_tabs_and_wide_cells() {
    let mut buf = open("abcdefghij\n\tx\n世界ab\nz");
    for _ in 0..9 {
        buf.right();
    }
    buf.vertical(1, &width);
    assert_eq!(buf.cursor(), Pos { line: 1, byte: 2 }, "tab spans columns 0..8, x is column 8");
    buf.vertical(1, &width);
    // Column 9 falls on the second cell of nothing: 世(0-1)界(2-3)a(4)b(5); end is 6.
    assert_eq!(buf.cursor(), Pos { line: 2, byte: "世界ab".len() });
    buf.vertical(1, &width);
    assert_eq!(buf.cursor(), Pos { line: 3, byte: 1 });
    buf.vertical(-3, &width);
    assert_eq!(buf.cursor(), Pos { line: 0, byte: 9 }, "desired column survives short lines");
    let mut cjk = open("世界\nabc");
    cjk.go_to_line(2);
    cjk.right();
    cjk.vertical(-1, &width);
    assert!(cjk.lines()[0].text.is_char_boundary(cjk.cursor().byte));
    assert_eq!(cjk.column(0, cjk.cursor().byte, &width) % 2, 0, "never inside a wide cell");
}

#[test]
fn search_wraps_once_and_go_to_line_validates() {
    let mut buf = open("x target\ny\ntarget z");
    assert_eq!(buf.find("target"), Found::At(Pos { line: 0, byte: 2 }));
    assert_eq!(buf.find_next(), Found::At(Pos { line: 2, byte: 0 }));
    assert_eq!(buf.find_next(), Found::Wrapped(Pos { line: 0, byte: 2 }));
    assert_eq!(buf.find("absent"), Found::NotFound);
    assert_eq!(buf.cursor(), Pos { line: 0, byte: 2 });
    assert!(!buf.go_to_line(0) && !buf.go_to_line(4));
    assert_eq!(buf.cursor(), Pos { line: 0, byte: 2 });
    assert!(buf.go_to_line(3));
    assert_eq!(buf.cursor(), Pos { line: 2, byte: 0 });
}

fn spans(lang: Lang, lines: &[&str]) -> Vec<Vec<(String, Kind)>> {
    let mut carry = Carry::None;
    lines.iter().map(|line| {
        let mut out: Vec<Span> = Vec::new();
        carry = highlight(lang, carry, line, &mut out);
        out.iter().map(|s| (String::from(&line[s.start..s.end]), s.kind)).collect()
    }).collect()
}

#[test]
fn highlighting_is_by_suffix_and_carries_only_block_comments_and_triple_strings() {
    assert_eq!(Lang::for_path("/srv/a.toml"), Some(Lang::Toml));
    assert_eq!(Lang::for_path("/srv/run.sh"), Some(Lang::Shell));
    assert_eq!(Lang::for_path("/srv/m.rs"), Some(Lang::Rust));
    assert_eq!(Lang::for_path("/srv/a.TOML"), None);
    assert_eq!(Lang::for_path("/srv/rs"), None);
    let rust = spans(Lang::Rust, &["let x = 1; /* open", "still */ fn f() {}", "'a' 'b: \"s\\\"\" // c"]);
    assert_eq!(rust[0], [("let".into(), Kind::Keyword), ("1".into(), Kind::Number), ("/* open".into(), Kind::Comment)]);
    assert_eq!(rust[1], [("still */".into(), Kind::Comment), ("fn".into(), Kind::Keyword)]);
    assert_eq!(rust[2], [("'a'".into(), Kind::String), ("\"s\\\"\"".into(), Kind::String), ("// c".into(), Kind::Comment)]);
    let toml = spans(Lang::Toml, &["k = \"\"\"multi", "line\"\"\" # c", "n = 3.5"]);
    assert_eq!(toml[0], [("\"\"\"multi".into(), Kind::String)]);
    assert_eq!(toml[1], [("line\"\"\"".into(), Kind::String), ("# c".into(), Kind::Comment)]);
    assert_eq!(toml[2], [("3.5".into(), Kind::Number)]);
    let sh = spans(Lang::Shell, &["echo a#b 'x # y' # real", "\"unterminated"]);
    assert_eq!(sh[0], [("'x # y'".into(), Kind::String), ("# real".into(), Kind::Comment)]);
    assert_eq!(sh[1], [("\"unterminated".into(), Kind::String)], "no carry for ordinary strings");
}
