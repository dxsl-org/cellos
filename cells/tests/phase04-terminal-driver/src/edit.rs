//! Phase 08 S8-5 (`phase-08-s0-proposal.md` §3.4): `edit` on the target through a real
//! frontend host. Host `t` runs the editor; the main session `a` writes the concurrent
//! changes and reads the stored bytes back. Every check judges stored bytes, the drawn
//! status/message rows and `$?`, never exact wording beyond the stable fragments the
//! editor prints for each outcome. Files live on the CellosFS `/srv` of the fixture disk
//! (D03) and, for the refusal, on the FAT32 `/mnt/sd`.

use super::*;

const ED: &str = "/srv/ed";
const ROWS: u16 = 24;
const COLS: u16 = 80;
const EDIT_MS: u64 = 60_000;
const BACKSPACE: u32 = 0x110003;
const TAB: u32 = 0x110004;
const RIGHT: u32 = 0x110013;
const END: u32 = 0x110015;

const EDIT_MARKERS: [&str; 10] = [
    "edit-refusals", "edit-save-reopen-exact", "edit-conflict-keeps-buffer",
    "edit-saveas-overwrite-race", "edit-fat-save-refused", "edit-paste-one-group",
    "edit-dirty-exit-no-default", "edit-resize", "edit-quit-restores", "edit-find-select-redo",
];

fn ctrl(t: &mut Term, c: char) -> Result<(), String> {
    t.host.submit_key(c as u32, CTRL, 1).map_err(|s| format!("ctrl-{c} {s:?}"))
}

fn row(s: &Screen, y: usize) -> String {
    s.text(y, 0, s.size().0)
}

fn status_row(s: &Screen) -> String {
    row(s, s.size().1 - 2)
}

fn bottom_row(s: &Screen) -> String {
    row(s, s.size().1 - 1)
}

/// Wait for the editor's status row to name `name`.
fn opened(t: &mut Term, name: &str) -> Result<(), String> {
    wait_screen(t, EDIT_MS, "edit status row", |s| s.alternate().then(|| status_row(s)).filter(|r| r.contains(name)).map(drop))
}

fn bottom_has(t: &mut Term, what: &str) -> Result<(), String> {
    wait_screen(t, EDIT_MS, what, |s| bottom_row(s).contains(what).then_some(()))
}

fn dirty_mark(t: &mut Term, dirty: bool) -> Result<(), String> {
    wait_screen(t, EDIT_MS, "dirty mark", |s| (status_row(s).contains(" * ") == dirty).then_some(()))
}

/// Wait for the status row to report the cursor at `line`, `col`.
fn at(t: &mut Term, line: usize, col: usize) -> Result<(), String> {
    let want = format!("Ln {line}, Col {col}");
    wait_screen(t, EDIT_MS, &want, |s| status_row(s).contains(&want).then_some(()))
}

/// Answer a Ctrl-F / Ctrl-G prompt.
fn prompt(t: &mut Term, key: char, answer: &str) -> Result<(), String> {
    ctrl(t, key)?;
    type_keys(t, answer)?;
    press(t, ENTER)
}

/// `cat` of `path` on `a`, cut to the size `ls --records` reports: exact stored bytes,
/// whatever the prompt layout after them. (Cellbox `wc` reads only RamFS paths.)
fn stored(a: &mut Term, path: &str) -> Result<Vec<u8>, String> {
    let (dir, name) = path.rsplit_once('/').ok_or_else(|| String::from("path"))?;
    let query = format!("ls --records {dir} | where name '==' '\"{name}\"' | select size | to jsonl");
    let rows = a.run(&query, 30_000)?;
    let len: usize = rows.iter().find_map(|l| l.strip_prefix("{\"size\":")?.strip_suffix('}')?.parse().ok())
        .ok_or_else(|| format!("size {rows:?}"))?;
    let cmd = format!("cat {path}");
    a.run(&cmd, 30_000)?;
    let body = a.out.strip_prefix(cmd.as_bytes()).and_then(|r| r.strip_prefix(b"\n"))
        .ok_or_else(|| String::from("cat echo"))?;
    body.get(..len).map(<[u8]>::to_vec).ok_or_else(|| format!("cat short of {len}"))
}

fn last_status(t: &mut Term, var: &str) -> Result<String, String> {
    let lines = t.run(&format!("echo {var}=$?"), 30_000)?;
    Ok(lines.join("|"))
}

/// Leave the editor with Ctrl-Q (and `d` when a dirty prompt appears), back at the prompt.
fn quit(t: &mut Term, discard: bool) -> Result<(), String> {
    t.pump();
    t.out.clear();
    t.err.clear();
    ctrl(t, 'q')?;
    if discard {
        bottom_has(t, "(d)iscard")?;
        type_keys(t, "d")?;
    }
    t.wait_prompt(EDIT_MS)
}

pub(crate) fn edit_checks(a: &mut Term) {
    let setup = [
        format!("mkdir {ED}"),
        format!("echo -e 'one\\r\\ntwo\\nlast' | /bin/p03-filter cat > {ED}/m.txt"),
        format!("echo -en 'base' | /bin/p03-filter cat > {ED}/c.txt"),
        format!("echo -e 'alpha\\nbeta\\nalpha' | /bin/p03-filter cat > {ED}/f.txt"),
        format!("echo -en 'dst' | /bin/p03-filter cat > {ED}/d.txt"),
        String::from("echo -en 'fat' | /bin/p03-filter cat > /mnt/sd/edfat.txt"),
    ];
    let failed: Vec<String> = setup.iter().filter(|cmd| a.run(cmd, 30_000).is_err()).cloned().collect();
    if !failed.is_empty() {
        return skip(&EDIT_MARKERS, &format!("setup {failed:?}"));
    }
    edit_refusals(a);
    let mut t = match open_shell(ROWS, COLS) {
        Ok(term) => term,
        Err(why) => return skip(&EDIT_MARKERS[1..], &format!("host t {why}")),
    };
    edit_save_reopen(a, &mut t);
    edit_conflict(a, &mut t);
    edit_saveas_race(a, &mut t);
    edit_fat_refused(a, &mut t);
    edit_paste(a, &mut t);
    edit_find_select_redo(a, &mut t);
    edit_dirty_exit(&mut t);
    edit_resize(&mut t);
    let _ = t.host.close();
}

/// Usage errors exit 2; a directory exits 1; a pipeline is refused before the editor
/// runs; none of them draws a screen.
fn edit_refusals(a: &mut Term) {
    let enters = a.screen.enters;
    let usage = a.run("edit one two", 30_000).and_then(|_| last_status(a, "stu"));
    let dir = a.run(&format!("edit {ED}"), 30_000).and_then(|_| last_status(a, "std"));
    let pipe = a.run(&format!("edit {ED}/m.txt | cat"), 30_000).and_then(|_| last_status(a, "stp"));
    check(
        "edit-refusals",
        usage.as_deref() == Ok("stu=2") && dir.as_deref() == Ok("std=1")
            && pipe.as_ref().is_ok_and(|s| s != "stp=0") && a.screen.enters == enters,
        &format!("usage={usage:?} dir={dir:?} pipe={pipe:?} enters={}", a.screen.enters - enters),
    );
}

/// Edits over CRLF/LF lines, a multibyte Backspace, a tab and Enter; Ctrl-S; quit. The
/// stored bytes keep every original terminator and the final newline, and a reopen shows a
/// clean buffer.
fn edit_save_reopen(a: &mut Term, t: &mut Term) {
    let path = format!("{ED}/m.txt");
    let run = (|| {
        t.send(&format!("edit {path}"))?;
        opened(t, "m.txt")?;
        press(t, END)?;
        type_keys(t, "1")?;
        press(t, DOWN)?;
        type_keys(t, "é")?;
        press(t, BACKSPACE)?;
        type_keys(t, "2")?;
        press(t, ENTER)?;
        press(t, TAB)?;
        type_keys(t, "new")?;
        dirty_mark(t, true)?;
        ctrl(t, 's')?;
        bottom_has(t, "saved")?;
        dirty_mark(t, false)?;
        quit(t, false)?;
        let st = last_status(t, "sts")?;
        t.send(&format!("edit {path}"))?;
        opened(t, "m.txt")?;
        dirty_mark(t, false)?;
        quit(t, false)?;
        Ok::<String, String>(st)
    })();
    let bytes = stored(a, &path);
    check(
        "edit-save-reopen-exact",
        run.as_deref() == Ok("sts=0") && bytes.as_deref() == Ok(b"one1\r\ntwo2\n\tnew\nlast\n".as_slice()),
        &format!("run={run:?} bytes={:?}", bytes.map(|b| String::from_utf8_lossy(&b).into_owned())),
    );
}

/// Find, find-next with wrap, go-to-line, selection with Ctrl-C copy and Ctrl-X cut,
/// paste, undo and redo through the real terminal; the saved bytes show each effect.
fn edit_find_select_redo(a: &mut Term, t: &mut Term) {
    let path = format!("{ED}/f.txt");
    let run = (|| {
        t.send(&format!("edit {path}"))?;
        opened(t, "f.txt")?;
        prompt(t, 'f', "alpha")?;
        at(t, 3, 1)?;
        ctrl(t, 'n')?;
        at(t, 1, 1)?;
        bottom_has(t, "wrapped")?;
        prompt(t, 'g', "2")?;
        at(t, 2, 1)?;
        ctrl(t, ' ')?;
        press(t, END)?;
        at(t, 2, 5)?;
        ctrl(t, 'c')?;
        press(t, ESCAPE)?;
        dirty_mark(t, false)?;
        prompt(t, 'g', "3")?;
        press(t, END)?;
        at(t, 3, 6)?;
        ctrl(t, 'v')?;
        at(t, 3, 10)?;
        prompt(t, 'g', "1")?;
        ctrl(t, ' ')?;
        press(t, END)?;
        ctrl(t, 'x')?;
        at(t, 1, 1)?;
        ctrl(t, 'z')?;
        at(t, 1, 6)?;
        ctrl(t, 'y')?;
        at(t, 1, 1)?;
        ctrl(t, 's')?;
        bottom_has(t, "saved")?;
        quit(t, false)
    })();
    let bytes = stored(a, &path);
    check(
        "edit-find-select-redo",
        run.is_ok() && bytes.as_deref() == Ok(b"\nbeta\nalphabeta\n".as_slice()),
        &format!("run={run:?} bytes={bytes:?}"),
    );
    if run.is_err() {
        let _ = quit(t, true);
    }
}

/// The file changes on `a` after `t` opened it: Ctrl-S refuses, the other writer's bytes
/// stay, `t` keeps its edit and stays dirty.
fn edit_conflict(a: &mut Term, t: &mut Term) {
    let path = format!("{ED}/c.txt");
    let run = (|| {
        t.send(&format!("edit {path}"))?;
        opened(t, "c.txt")?;
        type_keys(t, "X")?;
        dirty_mark(t, true)?;
        a.run(&format!("echo -en 'other' | /bin/p03-filter cat > {path}"), 30_000)?;
        ctrl(t, 's')?;
        bottom_has(t, "changed since it was read")?;
        dirty_mark(t, true)
    })();
    let bytes = stored(a, &path);
    let left = quit(t, true);
    check(
        "edit-conflict-keeps-buffer",
        run.is_ok() && left.is_ok() && bytes.as_deref() == Ok(b"other".as_slice()),
        &format!("run={run:?} quit={left:?} bytes={bytes:?}"),
    );
}

/// Save As onto an existing file asks first; the file then changes on `a`; `y` refuses
/// with the other bytes intact. Save As to an absent name creates it and becomes the
/// buffer's file.
fn edit_saveas_race(a: &mut Term, t: &mut Term) {
    let dst = format!("{ED}/d.txt");
    let fresh = format!("{ED}/new.txt");
    let run = (|| {
        t.send("edit")?;
        opened(t, "[unnamed]")?;
        type_keys(t, "mine")?;
        ctrl(t, 'o')?;
        bottom_has(t, "Save as:")?;
        type_keys(t, &dst)?;
        press(t, ENTER)?;
        bottom_has(t, "overwrite? (y/n)")?;
        a.run(&format!("echo -en 'raced' | /bin/p03-filter cat > {dst}"), 30_000)?;
        type_keys(t, "y")?;
        bottom_has(t, "changed since it was read")?;
        ctrl(t, 'o')?;
        bottom_has(t, "Save as:")?;
        type_keys(t, &fresh)?;
        press(t, ENTER)?;
        bottom_has(t, "saved")?;
        opened(t, "new.txt")?;
        quit(t, false)
    })();
    let raced = stored(a, &dst);
    let created = stored(a, &fresh);
    check(
        "edit-saveas-overwrite-race",
        run.is_ok() && raced.as_deref() == Ok(b"raced".as_slice()) && created.as_deref() == Ok(b"mine".as_slice()),
        &format!("run={run:?} raced={raced:?} created={created:?}"),
    );
}

/// FAT32 has no conditional publication: the file opens, Ctrl-S is refused, nothing is
/// written.
fn edit_fat_refused(a: &mut Term, t: &mut Term) {
    let path = "/mnt/sd/edfat.txt";
    let run = (|| {
        t.send(&format!("edit {path}"))?;
        opened(t, "edfat.txt")?;
        type_keys(t, "Z")?;
        ctrl(t, 's')?;
        bottom_has(t, "save refused")?;
        dirty_mark(t, true)?;
        quit(t, true)
    })();
    let bytes = stored(a, path);
    check(
        "edit-fat-save-refused",
        run.is_ok() && bytes.as_deref() == Ok(b"fat".as_slice()),
        &format!("run={run:?} bytes={bytes:?}"),
    );
}

/// A bracketed paste with newlines is one edit group: one Ctrl-Z returns to the clean
/// empty buffer, so Ctrl-Q exits at once and the absent file is never created.
fn edit_paste(a: &mut Term, t: &mut Term) {
    let path = format!("{ED}/p.txt");
    let run = (|| {
        t.send(&format!("edit {path}"))?;
        opened(t, "p.txt")?;
        t.host.submit_paste("a\nb\r\nc").map_err(|s| format!("paste {s:?}"))?;
        dirty_mark(t, true)?;
        ctrl(t, 'z')?;
        dirty_mark(t, false)?;
        quit(t, false)
    })();
    let absent = a.run(&format!("ls {path}"), 30_000).and_then(|_| last_status(a, "stl"));
    check(
        "edit-paste-one-group",
        run.is_ok() && absent.as_ref().is_ok_and(|s| s != "stl=0"),
        &format!("run={run:?} ls={absent:?}"),
    );
}

/// A dirty Ctrl-Q waits for s/d/c: Enter is not an answer, `c` returns to the buffer,
/// `d` exits 0. The exit restores the terminal modes.
fn edit_dirty_exit(t: &mut Term) {
    let run = (|| {
        t.send("edit")?;
        opened(t, "[unnamed]")?;
        type_keys(t, "q")?;
        ctrl(t, 'q')?;
        bottom_has(t, "(d)iscard")?;
        press(t, ENTER)?;
        drain_for(t, 1_000);
        let asking = bottom_row(&t.screen).contains("(d)iscard");
        type_keys(t, "c")?;
        bottom_has(t, "^S save")?;
        t.pump();
        t.out.clear();
        ctrl(t, 'q')?;
        bottom_has(t, "(d)iscard")?;
        type_keys(t, "d")?;
        t.wait_prompt(EDIT_MS)?;
        Ok::<bool, String>(asking)
    })();
    let (restored, at) = in_order(&t.out, &TUI_RESTORE);
    let model = !t.screen.alternate() && !t.screen.cursor_hidden() && !t.screen.bracketed_paste();
    let st = last_status(t, "stx");
    check(
        "edit-dirty-exit-no-default",
        run == Ok(true) && st.as_deref() == Ok("stx=0"),
        &format!("run={run:?} st={st:?}"),
    );
    check("edit-quit-restores", restored && model, &format!("restore={at:?} {}", t.screen.summary()));
}

/// A resize repaints the status row at the new last-but-one row; the buffer survives.
fn edit_resize(t: &mut Term) {
    let run = (|| {
        t.send("edit")?;
        opened(t, "[unnamed]")?;
        type_keys(t, "keep")?;
        press(t, RIGHT)?;
        t.host.resize(30, 100).map_err(|s| format!("resize {s:?}"))?;
        wait_screen(t, EDIT_MS, "status at row 28", |s| (s.size() == (100, 30) && row(s, 28).contains("Ln 1, Col 5")).then_some(()))?;
        t.host.resize(ROWS, COLS).map_err(|s| format!("resize {s:?}"))?;
        wait_screen(t, EDIT_MS, "status at row 22", |s| (s.size() == (80, 24) && row(s, 22).contains("Ln 1, Col 5")).then_some(()))?;
        quit(t, true)
    })();
    check("edit-resize", run.is_ok(), &format!("run={run:?}"));
}
