//! Save-safety and key-flow regressions for `edit`, against a recording store.

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use api::ipc::{Observation, SaveExpect};
use ostd::ViError;
use terminal_sdk::codec::{KeyState, KeySymbol, Modifiers};
use text_engine::editor::EditBuffer;
use vitui::event::{Event, Key};
use vitui::ratatui::core::buffer::Buffer;
use vitui::ratatui::core::layout::Rect;
use vitui::theme::{ColorMode, Theme};

use super::editor::{Editor, Mode, SaveResult, Step, Store, SESSION_LOST};

const OPENED: SaveExpect = SaveExpect::Present { inode: 7, generation: 3 };
const SAVED: SaveExpect = SaveExpect::Present { inode: 9, generation: 1 };

#[derive(Default)]
struct Recorder {
    observations: Vec<(String, Result<Observation, ViError>)>,
    results: VecDeque<SaveResult>,
    saves: Vec<(String, Vec<u8>, SaveExpect)>,
    observed: usize,
}

impl Store for Recorder {
    fn observe(&mut self, path: &str) -> Result<Observation, ViError> {
        self.observed += 1;
        self.observations.iter().find(|(p, _)| p == path).map_or(Ok(Observation::Absent), |(_, o)| *o)
    }
    fn save(&mut self, path: &str, bytes: &[u8], expect: SaveExpect) -> SaveResult {
        self.saves.push((path.to_string(), bytes.to_vec(), expect));
        self.results.pop_front().expect("a scripted result")
    }
}

fn editor(text: &str, path: Option<&str>, binding: Option<SaveExpect>) -> Editor {
    Editor::new(EditBuffer::from_bytes(text.as_bytes()).unwrap(), path.map(String::from), binding, String::from("/srv"))
}

fn ctrl(c: char) -> Event {
    Event::Key(Key { symbol: KeySymbol::Character(c), modifiers: Modifiers::CONTROL, state: KeyState::Press })
}

fn key(symbol: KeySymbol) -> Event {
    Event::Key(Key { symbol, modifiers: Modifiers::NONE, state: KeyState::Press })
}

fn text(s: &str) -> Event {
    Event::Text(s.to_string())
}

fn feed(ed: &mut Editor, store: &mut Recorder, events: &[Event]) -> Step {
    let mut last = Step::Continue;
    for event in events {
        last = ed.handle(event, store);
    }
    last
}

#[test]
fn save_is_bound_to_the_opening_observation_and_a_conflict_keeps_everything() {
    let mut store = Recorder { results: [SaveResult::Conflict].into(), ..Default::default() };
    let mut ed = editor("a\r\nb", Some("/srv/cfg"), Some(OPENED));
    feed(&mut ed, &mut store, &[key(KeySymbol::End), text("!"), ctrl('s')]);
    assert_eq!(store.saves, [("/srv/cfg".to_string(), b"a!\r\nb".to_vec(), OPENED)]);
    assert!(ed.buf.is_dirty());
    assert_eq!(ed.buf.to_bytes(), b"a!\r\nb");
    assert_eq!(store.observed, 0, "never re-observed to make the save succeed");
    // The next save is still bound to the same observation: no silent re-binding.
    store.results.push_back(SaveResult::Committed(SAVED));
    feed(&mut ed, &mut store, &[ctrl('s')]);
    assert_eq!(store.saves[1].2, OPENED);
    assert!(!ed.buf.is_dirty());
    // After a commit, later saves bind to the committed state.
    store.results.push_back(SaveResult::Committed(SaveExpect::Present { inode: 9, generation: 2 }));
    feed(&mut ed, &mut store, &[text("x"), ctrl('s')]);
    assert_eq!(store.saves[2].2, SAVED);
}

#[test]
fn unknown_or_refused_outcomes_never_mark_the_buffer_saved() {
    for result in [SaveResult::Indeterminate, SaveResult::NotPublished("quota exceeded"), SaveResult::Failed(ViError::IO)] {
        let mut store = Recorder { results: [result].into(), ..Default::default() };
        let mut ed = editor("x", Some("/srv/f"), Some(OPENED));
        feed(&mut ed, &mut store, &[text("y"), ctrl('s')]);
        assert!(ed.buf.is_dirty(), "{result:?}");
        assert_eq!(ed.path(), Some("/srv/f"));
        assert_eq!(ed.buf.to_bytes(), b"yx");
        assert!(ed.buf.undo(), "history survives a failed save");
    }
}

#[test]
fn save_as_onto_an_existing_file_needs_yes_and_commits_against_what_was_asked_about() {
    let present = Observation::Present { inode: 4, generation: 8, size: 3, regular: true };
    let mut store = Recorder { observations: vec![("/srv/dst".into(), Ok(present))], ..Default::default() };
    let mut ed = editor("new", None, None);
    feed(&mut ed, &mut store, &[ctrl('o'), text("dst"), key(KeySymbol::Enter)]);
    assert!(matches!(ed.mode, Mode::Overwrite { .. }));
    // Enter and Escape mean no; nothing is written.
    feed(&mut ed, &mut store, &[key(KeySymbol::Enter)]);
    assert_eq!(ed.mode, Mode::Edit);
    assert!(store.saves.is_empty());
    // Yes publishes with the expectation observed before the question.
    store.results.push_back(SaveResult::Conflict);
    feed(&mut ed, &mut store, &[ctrl('o'), text("dst"), key(KeySymbol::Enter), text("y")]);
    assert_eq!(store.saves, [("/srv/dst".to_string(), b"new".to_vec(), SaveExpect::Present { inode: 4, generation: 8 })]);
    assert_eq!(ed.path(), None, "a failed Save As keeps the old association");
    // An absent destination is created only if still absent.
    store.results.push_back(SaveResult::Committed(SAVED));
    feed(&mut ed, &mut store, &[ctrl('o'), text("fresh.toml"), key(KeySymbol::Enter)]);
    assert_eq!(store.saves[1], ("/srv/fresh.toml".to_string(), b"new".to_vec(), SaveExpect::Absent));
    assert_eq!(ed.path(), Some("/srv/fresh.toml"));
    assert!(!ed.buf.is_dirty());
}

#[test]
fn dirty_exit_has_no_default_and_exits_only_after_a_committed_save() {
    let mut store = Recorder { results: [SaveResult::Conflict, SaveResult::Committed(SAVED)].into(), ..Default::default() };
    let mut ed = editor("", Some("/srv/f"), Some(OPENED));
    assert_eq!(feed(&mut ed, &mut store, &[text("a"), ctrl('q'), key(KeySymbol::Enter)]), Step::Continue);
    assert_eq!(ed.mode, Mode::DirtyExit, "Enter is not an answer");
    assert_eq!(feed(&mut ed, &mut store, &[text("c")]), Step::Continue);
    assert_eq!(ed.mode, Mode::Edit);
    assert_eq!(feed(&mut ed, &mut store, &[ctrl('q'), text("s")]), Step::Continue, "a conflicting save does not exit");
    assert!(ed.buf.is_dirty());
    assert_eq!(feed(&mut ed, &mut store, &[ctrl('q'), text("S")]), Step::Exit(0));
    let mut ed = editor("", None, None);
    assert_eq!(feed(&mut ed, &mut store, &[text("a"), ctrl('q'), text("d")]), Step::Exit(0));
    assert_eq!(feed(&mut editor("", None, None), &mut store, &[ctrl('q')]), Step::Exit(0), "clean exits at once");
}

#[test]
fn a_mount_without_conditional_save_refuses_before_any_storage_call() {
    let mut store = Recorder::default();
    let mut ed = editor("fat", Some("/mnt/sd/x.txt"), None);
    feed(&mut ed, &mut store, &[text("!"), ctrl('s')]);
    assert!(store.saves.is_empty());
    assert!(ed.message.as_deref().is_some_and(|m| m.starts_with("save refused")));
    assert!(ed.buf.is_dirty());
    // Save As to such a mount is refused by observation, also before staging.
    let mut store = Recorder { observations: vec![("/mnt/sd/y".into(), Err(ViError::NotSupported))], ..Default::default() };
    feed(&mut ed, &mut store, &[ctrl('o'), text("/mnt/sd/y"), key(KeySymbol::Enter)]);
    assert!(store.saves.is_empty());
}

#[test]
fn ctrl_c_copies_and_end_of_input_ends_the_session() {
    let mut store = Recorder::default();
    let mut ed = editor("abc", None, None);
    feed(&mut ed, &mut store, &[ctrl(' '), key(KeySymbol::Right), key(KeySymbol::Right), Event::Interrupt]);
    assert_eq!(ed.buf.clipboard(), "ab");
    // Escape drops the selection; otherwise the paste would replace it.
    feed(&mut ed, &mut store, &[key(KeySymbol::Escape), key(KeySymbol::End), ctrl('v')]);
    assert_eq!(ed.buf.to_bytes(), b"abcab");
    assert_eq!(feed(&mut ed, &mut store, &[Event::Eof]), Step::Exit(SESSION_LOST));
}

#[test]
fn unnamed_save_opens_save_as_and_prompts_are_cancelled_by_escape() {
    let mut store = Recorder::default();
    let mut ed = editor("", None, None);
    feed(&mut ed, &mut store, &[ctrl('s')]);
    assert!(matches!(ed.mode, Mode::Prompt(..)));
    feed(&mut ed, &mut store, &[text("x"), key(KeySymbol::Escape)]);
    assert_eq!(ed.mode, Mode::Edit);
    assert!(store.saves.is_empty());
    feed(&mut ed, &mut store, &[ctrl('g'), text("9"), key(KeySymbol::Enter)]);
    assert!(ed.message.as_deref().is_some_and(|m| m.contains("1..1")));
}

#[test]
fn rendering_escapes_controls_and_expands_tabs_without_changing_bytes() {
    let mut ed = editor("a\tb\u{1b}[31m\u{9b}", None, None);
    let area = Rect::new(0, 0, 30, 4);
    let mut out = Buffer::empty(area);
    let cursor = super::render::draw(&mut ed, &Theme::new(ColorMode::Ansi16), area, &mut out);
    let row: String = (0..30).map(|x| out[(x, 0)].symbol().to_string()).collect();
    assert!(row.starts_with("a       b^[[31m<9B>"), "{row:?}");
    assert_eq!(cursor.map(|p| (p.x, p.y)), Some((0, 0)));
    assert_eq!(ed.buf.to_bytes(), "a\tb\u{1b}[31m\u{9b}".as_bytes());
}
