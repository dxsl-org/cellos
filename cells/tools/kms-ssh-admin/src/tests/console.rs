//! The echo-off block reader and the confirmation-word reader over scripted events.

use super::{key, pasted, text, typed_lines, Script, BACKSPACE, ENTER, ESCAPE};
use crate::armor::TEXT_MAX;
use crate::console::{read_block, read_word, Block, Word};
use crate::secret::Secret;
use terminal_sdk::input::EventKind;

const BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const END: &str = "-----END OPENSSH PRIVATE KEY-----";

fn body() -> std::string::String {
    std::format!("{BEGIN}\nQUJD\nREVG\n{END}\n")
}

fn read(events: Vec<EventKind>) -> (Block, std::string::String, usize) {
    let mut script = Script::new(events);
    let mut buf = Secret::with_capacity(TEXT_MAX);
    let outcome = read_block(&mut script, &mut buf);
    (
        outcome,
        std::string::String::from_utf8_lossy(buf.as_slice()).into_owned(),
        script.remaining(),
    )
}

#[test]
fn typed_lines_end_at_the_end_marker_line_and_nothing_after_is_read() {
    let mut events = typed_lines(&body());
    events.push(text("left over"));
    let (outcome, got, remaining) = read(events);
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, body());
    assert_eq!(remaining, 1, "the event after the END line is not consumed");
}

#[test]
fn a_bracketed_paste_in_one_or_many_events_is_accepted() {
    let (outcome, got, _) = read(pasted(&body()));
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, body());
    // Split at arbitrary points, including inside the END marker.
    let whole = body();
    let mut events = std::vec![EventKind::PasteBegin];
    for chunk in whole.as_bytes().chunks(7) {
        events.push(text(std::str::from_utf8(chunk).unwrap()));
    }
    events.push(EventKind::PasteEnd);
    let (outcome, got, remaining) = read(events);
    assert_eq!(
        (outcome, got.as_str(), remaining),
        (Block::Done, whole.as_str(), 0)
    );
    // An unbracketed paste is typed text and Enter keys.
    let (outcome, got, _) = read(typed_lines(&whole));
    assert_eq!((outcome, got.as_str()), (Block::Done, whole.as_str()));
}

#[test]
fn a_paste_whose_last_line_has_no_terminator_is_ended_by_its_paste_end() {
    let unterminated = std::format!("{BEGIN}\nQUJD\n{END}");
    let (outcome, got, _) = read(pasted(&unterminated));
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, std::format!("{unterminated}\n"));
    // CRLF breaks are kept as pasted; the END line is still recognized.
    let crlf = std::format!("{BEGIN}\r\nQUJD\r\n{END}\r\n");
    let (outcome, got, _) = read(pasted(&crlf));
    assert_eq!((outcome, got.as_str()), (Block::Done, crlf.as_str()));
}

#[test]
fn the_reader_waits_while_there_is_no_end_line() {
    let (outcome, _, _) = read(typed_lines(&std::format!("{BEGIN}\nQUJD\n")));
    assert_eq!(
        outcome,
        Block::Lost,
        "an exhausted script is a lost session, never Done"
    );
    // An END marker that is only a prefix of a line does not end the block.
    let (outcome, _, _) = read(typed_lines(&std::format!("{BEGIN}\n{END}x\n")));
    assert_eq!(outcome, Block::Lost);
}

#[test]
fn escape_interrupt_and_end_of_input_cancel() {
    for stop in [key(ESCAPE), EventKind::Interrupt, EventKind::Eof] {
        let (outcome, _, remaining) = read(std::vec![text("abc"), stop, text("more")]);
        assert_eq!(outcome, Block::Cancelled);
        assert_eq!(remaining, 1);
    }
    // Escape with a modifier is not a cancel.
    let (outcome, _, _) = read(std::vec![EventKind::Key {
        code: ESCAPE,
        modifiers: 1,
        pressed: 1
    }]);
    assert_eq!(outcome, Block::Lost);
    let (outcome, _, _) = read(std::vec![EventKind::Lost(1)]);
    assert_eq!(outcome, Block::Lost);
}

#[test]
fn backspace_edits_typed_text_but_not_a_paste() {
    let (outcome, got, _) = read(std::vec![
        text("ab"),
        key(BACKSPACE),
        text("c"),
        key(ENTER),
        text("x"),
        key(ENTER),
        key(BACKSPACE),
        key(BACKSPACE),
        text(END),
        key(ENTER),
    ]);
    // "ab"<BS>"c"<ENTER> -> "ac\n"; "x"<ENTER> -> "x\n"; two backspaces remove the newline and
    // the x, reopening that line.
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, std::format!("ac\n{END}\n"));
    let (outcome, got, _) = read(std::vec![
        EventKind::PasteBegin,
        text("ab"),
        key(BACKSPACE),
        EventKind::PasteEnd,
        text(END),
        key(ENTER),
    ]);
    assert_eq!(outcome, Block::Done);
    assert_eq!(
        got,
        std::format!("ab\n{END}\n"),
        "the paste end closes the line, a backspace inside the paste is ignored"
    );
}

#[test]
fn an_overlong_input_stops_at_the_cap_and_the_rest_of_the_paste_is_dropped() {
    let mut events = std::vec![EventKind::PasteBegin];
    // 2 x 60,000 bytes exceed the 98,304 cap in the second event.
    events.push(text(&"A".repeat(60_000)));
    events.push(text(&"B".repeat(60_000)));
    events.push(text(&"C".repeat(10)));
    events.push(EventKind::PasteEnd);
    events.push(text("after the paste"));
    let (outcome, got, remaining) = read(events);
    assert_eq!(outcome, Block::TooLong);
    assert!(got.len() <= TEXT_MAX);
    assert_eq!(
        remaining, 1,
        "everything up to the paste end was consumed, nothing after it"
    );
    // Typed input over the cap is refused too.
    let mut events = Vec::new();
    for _ in 0..(TEXT_MAX / 1000 + 2) {
        events.push(text(&"A".repeat(1000)));
    }
    let (outcome, _, _) = read(events);
    assert_eq!(outcome, Block::TooLong);
}

#[test]
fn a_cancel_inside_a_paste_drops_the_rest_of_it() {
    let (outcome, _, remaining) = read(std::vec![
        EventKind::PasteBegin,
        text("abc"),
        key(ESCAPE),
        text("def"),
        text("ghi"),
        EventKind::PasteEnd,
        text("typed afterwards"),
    ]);
    assert_eq!(outcome, Block::Cancelled);
    assert_eq!(remaining, 1);
}

#[test]
fn the_end_marker_inside_a_paste_with_trailing_junk_still_ends_reading() {
    // Everything up to the paste end is read and dropped; the decoder then refuses the junk.
    let junk = std::format!("{BEGIN}\nQUJD\n{END}\njunk\n");
    let (outcome, got, remaining) = read(pasted(&junk));
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, junk);
    assert_eq!(remaining, 0);
    let events = std::vec![
        EventKind::PasteBegin,
        text(&std::format!("{BEGIN}\nQUJD\n{END}\n")),
        text("junk"),
        EventKind::PasteEnd,
        key(ENTER),
    ];
    let (outcome, got, remaining) = read(events);
    assert_eq!(outcome, Block::Done);
    assert_eq!(got, std::format!("{BEGIN}\nQUJD\n{END}\n"));
    assert_eq!(remaining, 1, "the junk was drained up to the paste end");
}

#[test]
fn the_word_reader_takes_typed_text_and_enter() {
    let mut script = Script::new(std::vec![text("del"), text("ete"), key(ENTER)]);
    assert_eq!(read_word(&mut script), Word::Given("delete".into()));
    let mut script = Script::new(std::vec![
        text("delx"),
        key(BACKSPACE),
        text("ete"),
        key(ENTER)
    ]);
    assert_eq!(read_word(&mut script), Word::Given("delete".into()));
}

#[test]
fn the_word_reader_ignores_a_paste_cancels_and_bounds_its_input() {
    let mut script = Script::new(std::vec![
        EventKind::PasteBegin,
        text("delete"),
        EventKind::PasteEnd,
        key(ENTER)
    ]);
    assert_eq!(
        read_word(&mut script),
        Word::Given(String::new()),
        "a pasted word is not a typed word"
    );
    for stop in [key(ESCAPE), EventKind::Interrupt, EventKind::Eof] {
        let mut script = Script::new(std::vec![text("del"), stop]);
        assert_eq!(read_word(&mut script), Word::Cancelled);
    }
    let mut script = Script::new(std::vec![text("a"), EventKind::Lost(1)]);
    assert_eq!(read_word(&mut script), Word::Lost);
    let mut script = Script::new(std::vec![text("a")]);
    assert_eq!(read_word(&mut script), Word::Lost);
    let mut script = Script::new(std::vec![
        text(&"delete".repeat(10)),
        text("delete"),
        key(ENTER)
    ]);
    match read_word(&mut script) {
        Word::Given(word) => assert_ne!(word, "delete"),
        other => panic!("{other:?}"),
    }
}
