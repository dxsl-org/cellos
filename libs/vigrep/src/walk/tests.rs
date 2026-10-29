//! Walker tests over an in-memory [`ViSearchTree`] that counts handles and
//! reads.

use alloc::collections::{BTreeMap, BTreeSet};
use std::{cell::Cell, rc::Rc};

use super::*;
use crate::{
    ignore::overrides::OverrideBuilder,
    regex::RegexMatcher,
    searcher::{SearcherBuilder, sinks},
};

#[derive(Clone, Debug)]
struct Node {
    kind: EntryKind,
    contents: Vec<u8>,
}

struct FakeDir {
    path: Vec<u8>,
    pos: usize,
}

struct FakeFile {
    path: Vec<u8>,
    reads: usize,
}

/// An in-memory tree. Paths are absolute; a directory's children are the
/// nodes whose parent path is the directory.
#[derive(Default)]
struct FakeTree {
    nodes: BTreeMap<Vec<u8>, Node>,
    /// Listing a path below a key lists the value's subtree instead (a fake
    /// symlink cycle when the value is an ancestor).
    aliases: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Entries per listing page.
    page_size: usize,
    /// Maximum bytes per `read_chunk`.
    chunk_max: usize,
    denied: BTreeSet<Vec<u8>>,
    changed_dirs: BTreeSet<Vec<u8>>,
    /// `file_state` reports `modified` once the file saw this many reads.
    modified_after_reads: BTreeMap<Vec<u8>, usize>,
    /// After a file handle saw this many reads, the file's contents become
    /// the given value (a provider that changes a file without reporting
    /// `modified`).
    mutate_after_reads: BTreeMap<Vec<u8>, (usize, Vec<u8>)>,
    /// Set by a test's cancel callback once it returns true.
    cancelled: Rc<Cell<bool>>,
    /// Tree operations (other than `close`) after `cancelled` was set.
    ops_after_cancel: usize,
    open_dirs: usize,
    open_files: usize,
    peak_dirs: usize,
    peak_files: usize,
    reads: usize,
    listed: Vec<Vec<u8>>,
}

fn parent_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) => b"/",
        Some(i) => &path[..i],
        None => b"",
    }
}

fn name_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

impl FakeTree {
    fn new() -> FakeTree {
        FakeTree { page_size: 3, chunk_max: 4000, ..FakeTree::default() }
    }

    fn add(&mut self, path: &str, kind: EntryKind, contents: &[u8]) {
        let path = path.as_bytes().to_vec();
        let mut p = parent_of(&path).to_vec();
        while !p.is_empty() && !self.nodes.contains_key(&p) {
            let next = parent_of(&p).to_vec();
            self.nodes.insert(
                p.clone(),
                Node { kind: EntryKind::Dir, contents: vec![] },
            );
            if p == b"/" {
                break;
            }
            p = next;
        }
        self.nodes.insert(path, Node { kind, contents: contents.to_vec() });
    }

    fn file(&mut self, path: &str, contents: &str) -> &mut FakeTree {
        self.add(path, EntryKind::File, contents.as_bytes());
        self
    }

    fn bytes(&mut self, path: &str, contents: &[u8]) -> &mut FakeTree {
        self.add(path, EntryKind::File, contents);
        self
    }

    fn dir(&mut self, path: &str) -> &mut FakeTree {
        self.add(path, EntryKind::Dir, b"");
        self
    }

    fn kind(&mut self, path: &str, kind: EntryKind) -> &mut FakeTree {
        self.add(path, kind, b"");
        self
    }

    fn resolve(&self, path: &[u8]) -> Vec<u8> {
        let mut path = path.to_vec();
        loop {
            let hit = self.aliases.iter().find(|(k, _)| {
                path.starts_with(k)
                    && (path.len() == k.len() || path[k.len()] == b'/')
            });
            match hit {
                None => return path,
                Some((k, v)) => {
                    let mut p = v.clone();
                    p.extend_from_slice(&path[k.len()..]);
                    path = p;
                }
            }
        }
    }

    fn assert_released(&self) {
        assert_eq!(self.open_dirs, 0, "directory handles leaked");
        assert_eq!(self.open_files, 0, "file handles leaked");
    }

    fn note_op(&mut self) {
        if self.cancelled.get() {
            self.ops_after_cancel += 1;
        }
    }
}

impl ViSearchTree for FakeTree {
    type Dir = FakeDir;
    type File = FakeFile;

    fn open_dir(&mut self, path: &[u8]) -> Result<FakeDir, io::Error> {
        self.note_op();
        let real = self.resolve(path);
        if self.denied.contains(&real) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match self.nodes.get(&real) {
            None => Err(io::ErrorKind::NotFound.into()),
            Some(n) if n.kind == EntryKind::File => {
                Err(io::ErrorKind::NotADirectory.into())
            }
            Some(_) => {
                self.open_dirs += 1;
                self.peak_dirs = self.peak_dirs.max(self.open_dirs);
                self.listed.push(path.to_vec());
                Ok(FakeDir { path: real, pos: 0 })
            }
        }
    }

    fn next_page(
        &mut self,
        dir: &mut FakeDir,
        entries: &mut Vec<DirEntry>,
    ) -> Result<PageInfo, io::Error> {
        self.note_op();
        let children: Vec<DirEntry> = self
            .nodes
            .iter()
            .filter(|(p, _)| {
                p.as_slice() != b"/" && parent_of(p) == dir.path.as_slice()
            })
            .map(|(p, n)| DirEntry { kind: n.kind, name: name_of(p).to_vec() })
            // Reverse so that the walker cannot rely on listing order.
            .rev()
            .collect();
        let end = (dir.pos + self.page_size).min(children.len());
        entries.extend_from_slice(&children[dir.pos..end]);
        dir.pos = end;
        Ok(PageInfo {
            complete: end == children.len(),
            changed: self.changed_dirs.contains(&dir.path),
        })
    }

    fn open_file(&mut self, path: &[u8]) -> Result<FakeFile, io::Error> {
        self.note_op();
        let real = self.resolve(path);
        if self.denied.contains(&real) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match self.nodes.get(&real) {
            Some(n) if n.kind == EntryKind::File => {}
            Some(_) => return Err(io::Error::other("not a file")),
            None => return Err(io::ErrorKind::NotFound.into()),
        }
        self.open_files += 1;
        self.peak_files = self.peak_files.max(self.open_files);
        Ok(FakeFile { path: real, reads: 0 })
    }

    fn read_chunk(
        &mut self,
        file: &mut FakeFile,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error> {
        self.note_op();
        if let Some((n, c)) = self.mutate_after_reads.get(&file.path) {
            if file.reads == *n {
                let c = c.clone();
                self.nodes.get_mut(&file.path).unwrap().contents = c;
            }
        }
        self.reads += 1;
        file.reads += 1;
        let contents = &self.nodes[&file.path].contents;
        let start = (offset as usize).min(contents.len());
        let n = buf.len().min(self.chunk_max).min(contents.len() - start);
        buf[..n].copy_from_slice(&contents[start..start + n]);
        Ok(n)
    }

    fn file_state(
        &mut self,
        file: &mut FakeFile,
    ) -> Result<FileState, io::Error> {
        self.note_op();
        let modified = self
            .modified_after_reads
            .get(&file.path)
            .is_some_and(|&n| file.reads >= n);
        let size = self.nodes[&file.path].contents.len() as u64;
        Ok(FileState { size, modified })
    }

    fn close(&mut self, handle: Handle<FakeDir, FakeFile>) {
        match handle {
            Handle::Dir(_) => self.open_dirs -= 1,
            Handle::File(_) => self.open_files -= 1,
        }
    }
}

fn s(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn describe(event: &WalkEvent) -> String {
    match event {
        WalkEvent::File { path, explicit: false } => format!("F {}", s(path)),
        WalkEvent::File { path, explicit: true } => format!("X {}", s(path)),
        WalkEvent::MountSkipped { path } => format!("M {}", s(path)),
        WalkEvent::Error(err) => {
            let kind = match &err.kind {
                WalkErrorKind::Io(e) => format!("Io({:?})", e.kind()),
                WalkErrorKind::Ignore(_) => "Ignore".to_string(),
                other => {
                    let dbg = format!("{other:?}");
                    dbg.split('(').next().unwrap().to_string()
                }
            };
            format!("E {} {}", s(&err.path), kind)
        }
        WalkEvent::Cancelled => "C".to_string(),
    }
}

/// Walk to the end without searching; return the event descriptions.
fn events(tree: &mut FakeTree, builder: &WalkBuilder) -> Vec<String> {
    let mut walker = builder.build();
    let mut out = vec![];
    while let Some(ev) = walker.next(tree, &mut || false) {
        out.push(describe(&ev));
    }
    tree.assert_released();
    out
}

fn files(list: &[String]) -> Vec<String> {
    list.iter()
        .filter(|e| e.starts_with("F ") || e.starts_with("X "))
        .cloned()
        .collect()
}

fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn searcher() -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .buffer_capacity(16 * 1024)
        .heap_limit(Some(16 * 1024 + 65_536))
        .build()
}

/// Walk and search every file for `pattern`; return `path:line:text`
/// matches plus outcomes and errors.
fn search_all(
    tree: &mut FakeTree,
    builder: &WalkBuilder,
    pattern: &str,
) -> (Vec<String>, Walker) {
    let matcher = RegexMatcher::new_line_matcher(pattern).unwrap();
    let mut srch = searcher();
    let mut walker = builder.build();
    let mut out = vec![];
    while let Some(ev) = walker.next(tree, &mut || false) {
        let WalkEvent::File { path, .. } = ev else {
            out.push(describe(&ev));
            continue;
        };
        let mut lines = vec![];
        let sink = sinks::Bytes(|n, line: &[u8]| {
            lines.push(format!("{}:{}:{}", s(&path), n, s(line).trim_end()));
            Ok(true)
        });
        let result = walker.search_file(
            tree,
            &path,
            &mut srch,
            &matcher,
            sink,
            &mut || false,
        );
        out.extend(lines);
        match result {
            Ok(FileOutcome::Searched) => {}
            Ok(FileOutcome::Binary) => out.push(format!("B {}", s(&path))),
            Err(SearchFileError::Walk(err)) => {
                out.push(describe(&WalkEvent::Error(err)))
            }
            Err(SearchFileError::Sink(err)) => {
                out.push(format!("S {} {}", s(&path), err))
            }
            Err(SearchFileError::Cancelled) => out.push("C".to_string()),
        }
    }
    tree.assert_released();
    (out, walker)
}

#[test]
fn children_in_byte_order_depth_first() {
    let mut t = FakeTree::new();
    t.file("/r/b.txt", "")
        .file("/r/a/z.txt", "")
        .file("/r/a/y.txt", "")
        .file("/r/B.txt", "")
        .file("/r/\u{e9}.txt", "")
        .file("/r/a.txt", "")
        .file("/r/a-b/c.txt", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(
        ev,
        strs(&[
            "F /r/B.txt",
            "F /r/a/y.txt",
            "F /r/a/z.txt",
            "F /r/a-b/c.txt",
            "F /r/a.txt",
            "F /r/b.txt",
            "F /r/\u{e9}.txt",
        ])
    );
    // One directory handle at a time, never held across directories.
    assert_eq!(t.peak_dirs, 1);
}

#[test]
fn hidden_entries_excluded_unless_requested() {
    let mut t = FakeTree::new();
    t.file("/r/.h", "").file("/r/.d/x", "").file("/r/v", "");
    assert_eq!(events(&mut t, &WalkBuilder::new(["/r"])), strs(&["F /r/v"]));
    // The pruned hidden directory is never listed.
    assert!(!t.listed.iter().any(|p| p == b"/r/.d"));
    let ev = events(&mut t, WalkBuilder::new(["/r"]).hidden(true));
    assert_eq!(ev, strs(&["F /r/.d/x", "F /r/.h", "F /r/v"]));
}

#[test]
fn explicit_roots_bypass_filters() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "ign/\n*.log\n")
        .file("/r/.h", "")
        .file("/r/a.log", "")
        .file("/r/ign/x", "")
        .file("/r/.hd/y", "");
    let ev = events(
        &mut t,
        &WalkBuilder::new(["/r/.h", "/r/a.log", "/r/ign", "/r/.hd"]),
    );
    assert_eq!(
        ev,
        strs(&["X /r/.h", "X /r/a.log", "F /r/ign/x", "F /r/.hd/y"])
    );
}

#[test]
fn rgignore_over_ignore_over_gitignore() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "a\nb\nc\n")
        .file("/r/.ignore", "!a\nb\n")
        .file("/r/.rgignore", "!b\n")
        .file("/r/a", "")
        .file("/r/b", "")
        .file("/r/c", "")
        .file("/r/d", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["F /r/a", "F /r/b", "F /r/d"]));
    // --no-ignore disables all three files.
    let ev = events(&mut t, WalkBuilder::new(["/r"]).ignore_files(false));
    assert_eq!(ev, strs(&["F /r/a", "F /r/b", "F /r/c", "F /r/d"]));
}

#[test]
fn deeper_rules_override_ancestor_rules() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "*.log\n")
        .file("/r/sub/.gitignore", "!keep.log\n")
        .file("/r/sub/keep.log", "")
        .file("/r/sub/drop.log", "")
        .file("/r/top.log", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["F /r/sub/keep.log"]));
}

#[test]
fn last_matching_rule_wins_within_a_file() {
    let mut t = FakeTree::new();
    t.file("/r/.ignore", "*.txt\n!a.txt\na.txt\n!b.txt\n")
        .file("/r/a.txt", "")
        .file("/r/b.txt", "")
        .file("/r/c.txt", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["F /r/b.txt"]));
}

#[test]
fn negation_cannot_revive_pruned_directory() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "build/\n!build/keep.txt\n")
        .file("/r/build/keep.txt", "")
        .file("/r/src.txt", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["F /r/src.txt"]));
    assert!(!t.listed.iter().any(|p| p == b"/r/build"));
}

#[test]
fn command_globs_have_highest_precedence() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "*.rs\n")
        .file("/r/a.rs", "")
        .file("/r/b.txt", "")
        .file("/r/.h.rs", "")
        .file("/r/sub/c.rs", "")
        .file("/r/sub/d.md", "");
    // Positive globs restrict files, and override ignore and hidden.
    let mut ov = OverrideBuilder::new("/");
    ov.add("*.rs").unwrap();
    let ev = events(
        &mut t,
        WalkBuilder::new(["/r"]).overrides(ov.build().unwrap()),
    );
    assert_eq!(ev, strs(&["F /r/.h.rs", "F /r/a.rs", "F /r/sub/c.rs"]));
    // A negated glob excludes; later matching globs win.
    let mut ov = OverrideBuilder::new("/");
    ov.add("!*.md").unwrap().add("!*.txt").unwrap().add("b.txt").unwrap();
    let ev = events(
        &mut t,
        WalkBuilder::new(["/r"]).overrides(ov.build().unwrap()),
    );
    assert_eq!(ev, strs(&["F /r/b.txt"]));
}

#[test]
fn mounts_are_skipped_and_reported() {
    let mut t = FakeTree::new();
    t.kind("/r/mnt", EntryKind::Mount).file("/r/mnt/x", "").file("/r/y", "");
    let mut walker = WalkBuilder::new(["/r"]).build();
    let mut ev = vec![];
    while let Some(e) = walker.next(&mut t, &mut || false) {
        ev.push(describe(&e));
    }
    assert_eq!(ev, strs(&["M /r/mnt", "F /r/y"]));
    assert_eq!(walker.stats().mounts_skipped, 1);
    assert_eq!(walker.stats().errors, 0);
    // An explicit root on another mount is walked.
    t.nodes.get_mut(b"/r/mnt".as_slice()).unwrap().kind = EntryKind::Dir;
    let ev = events(&mut t, &WalkBuilder::new(["/r/mnt"]));
    assert_eq!(ev, strs(&["F /r/mnt/x"]));
}

#[test]
fn other_kind_is_a_per_entry_error() {
    let mut t = FakeTree::new();
    t.kind("/r/dev", EntryKind::Other).file("/r/z", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["E /r/dev OtherKind", "F /r/z"]));
}

#[test]
fn overlapping_roots_first_occurrence_wins() {
    let mut t = FakeTree::new();
    t.file("/r/a", "").file("/r/sub/b", "").file("/q/c", "");
    let mut walker = WalkBuilder::new(["/r", "/r/sub", "/r", "/q"]).build();
    let mut ev = vec![];
    while let Some(e) = walker.next(&mut t, &mut || false) {
        ev.push(describe(&e));
    }
    assert_eq!(ev, strs(&["F /r/a", "F /r/sub/b", "F /q/c"]));
    assert_eq!(walker.stats().roots_deduplicated, 2);
    // A later root containing an earlier one does not revisit it.
    let ev = events(&mut t, &WalkBuilder::new(["/r/sub", "/r"]));
    assert_eq!(ev, strs(&["F /r/sub/b", "F /r/a"]));
    let ev = events(&mut t, &WalkBuilder::new(["/", "/r"]));
    assert_eq!(ev, strs(&["F /q/c", "F /r/a", "F /r/sub/b"]));
}

/// Re-expression of upstream's multi-root ordering tests
/// (`multi_root_*_order_independent`, issues #3320/#3376/#3419): each root's
/// rules come only from its own subtree, in any root order. Unlike
/// upstream (which also reads ancestors of each root), an ignore file above
/// a root does not apply.
#[test]
fn roots_use_only_their_own_subtree_rules() {
    let mut t = FakeTree::new();
    t.file("/w/.rgignore", "beta/**/*.svg\nalpha/a.txt\n")
        .file("/w/alpha/a.txt", "")
        .file("/w/beta/.rgignore", "*.png\n")
        .file("/w/beta/x.svg", "")
        .file("/w/beta/y.png", "");
    for roots in [["/w/alpha", "/w/beta"], ["/w/beta", "/w/alpha"]] {
        let ev = files(&events(&mut t, &WalkBuilder::new(roots)));
        let mut sorted = ev.clone();
        sorted.sort();
        assert_eq!(sorted, strs(&["F /w/alpha/a.txt", "F /w/beta/x.svg"]));
    }
    // Walking the common parent applies its rules.
    let ev = events(&mut t, &WalkBuilder::new(["/w"]));
    assert_eq!(ev, Vec::<String>::new());
}

#[test]
fn depth_limit() {
    let mut t = FakeTree::new();
    t.file("/r/1/2/3/f", "").file("/r/1/g", "");
    let limits = WalkLimits { max_depth: 2, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/1/2/3 DepthLimit", "F /r/1/g"]));
}

#[test]
fn path_length_limit() {
    let mut t = FakeTree::new();
    t.file("/r/short", "").file("/r/a-much-longer-name", "");
    let limits = WalkLimits { max_path_len: 10, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(
        ev,
        strs(&["E /r/a-much-longer-name PathTooLong", "F /r/short"])
    );
}

#[test]
fn visited_entry_limit_ends_the_walk() {
    let mut t = FakeTree::new();
    t.file("/r/a", "").file("/r/b", "").file("/r/c", "").file("/q/d", "");
    let limits = WalkLimits { max_entries: 2, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r", "/q"]).limits(limits));
    assert_eq!(ev, strs(&["F /r/a", "F /r/b", "E /r/c EntryLimit"]));
}

#[test]
fn per_directory_entry_limit() {
    let mut t = FakeTree::new();
    t.file("/r/big/1", "")
        .file("/r/big/2", "")
        .file("/r/big/3", "")
        .file("/r/big/4", "")
        .file("/r/ok/1", "");
    let limits = WalkLimits { max_dir_entries: 3, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/big DirEntryLimit", "F /r/ok/1"]));
}

#[test]
fn pending_name_bytes_limit() {
    let mut t = FakeTree::new();
    t.file("/r/aaaa/bbbbbbbb", "").file("/r/cccc", "");
    // Packed children cost name + 2 header bytes: "aaaa" + "cccc" = 12
    // bytes pending at the root; visiting "aaaa" leaves 6, and listing it
    // adds 10.
    let limits = WalkLimits { max_pending_bytes: 15, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/aaaa PendingNamesLimit", "F /r/cccc"]));
    let limits = WalkLimits { max_pending_bytes: 16, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["F /r/aaaa/bbbbbbbb", "F /r/cccc"]));
}

#[test]
fn pending_entries_limit() {
    let mut t = FakeTree::new();
    t.file("/r/aaaa/x", "").file("/r/aaaa/y", "").file("/r/cccc", "");
    // Two children pending at the root; visiting "aaaa" leaves 1, and
    // listing it adds 2.
    let limits =
        WalkLimits { max_pending_entries: 2, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/aaaa PendingNamesLimit", "F /r/cccc"]));
    let limits =
        WalkLimits { max_pending_entries: 3, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["F /r/aaaa/x", "F /r/aaaa/y", "F /r/cccc"]));
}

#[test]
fn invalid_listing_names_are_errors() {
    let mut t = FakeTree::new();
    t.file("/r/ok", "");
    t.nodes.insert(
        b"/r/..".to_vec(),
        Node { kind: EntryKind::File, contents: vec![] },
    );
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["E /r InvalidName"]));
}

#[test]
fn changed_listing_is_an_error() {
    let mut t = FakeTree::new();
    t.file("/r/d/x", "").file("/r/e", "");
    t.changed_dirs.insert(b"/r/d".to_vec());
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["E /r/d ListingChanged", "F /r/e"]));
}

#[test]
fn denied_and_missing_entries_continue_the_walk() {
    let mut t = FakeTree::new();
    t.file("/r/secret/x", "").file("/r/z", "");
    t.denied.insert(b"/r/secret".to_vec());
    let ev = events(&mut t, &WalkBuilder::new(["/nope", "/r"]));
    assert_eq!(
        ev,
        strs(&[
            "E /nope Io(NotFound)",
            "E /r/secret Io(PermissionDenied)",
            "F /r/z",
        ])
    );
}

#[test]
fn ignore_file_failures_skip_the_directory() {
    let mut t = FakeTree::new();
    t.file("/r/d/.gitignore", "x\n").file("/r/d/y", "").file("/r/z", "");
    t.denied.insert(b"/r/d/.gitignore".to_vec());
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(
        ev,
        strs(&["E /r/d/.gitignore Io(PermissionDenied)", "F /r/z"])
    );

    let mut t = FakeTree::new();
    t.file("/r/d/.ignore", &"x\n".repeat(10)).file("/r/d/y", "");
    let limits =
        WalkLimits { max_ignore_rules_per_file: 9, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/d/.ignore IgnoreRulesLimit"]));

    let limits = WalkLimits {
        max_ignore_file_bytes: 19,
        scratch_bytes: 4,
        ..WalkLimits::default()
    };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/d/.ignore IgnoreFileTooLarge"]));

    let mut t = FakeTree::new();
    t.file("/r/.ignore", "a\n")
        .file("/r/d/.ignore", "bb\n")
        .file("/r/d/y", "");
    let limits =
        WalkLimits { max_ignore_bytes_total: 4, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/d/.ignore IgnoreBytesLimit"]));

    // Rules retained across the stack: the root's 2 rules fit a limit of 3,
    // the child's 2 more do not; a sibling tree under a fresh frame fits again.
    let mut t = FakeTree::new();
    t.file("/r/.ignore", "a\n# comment\n\nb\n")
        .file("/r/d/.ignore", "c\nd\n")
        .file("/r/d/y", "")
        .file("/r/e/.ignore", "e\n")
        .file("/r/e/z", "");
    let limits =
        WalkLimits { max_ignore_rules_total: 3, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r/d/.ignore IgnoreBytesLimit", "F /r/e/z"]));
    let limits =
        WalkLimits { max_ignore_rules_total: 4, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["F /r/d/y", "F /r/e/z"]));

    let mut t = FakeTree::new();
    t.file("/r/.ignore", "a\n").file("/r/y", "");
    t.modified_after_reads.insert(b"/r/.ignore".to_vec(), 1);
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["E /r/.ignore FileModified"]));
}

#[test]
fn malformed_rules_are_reported_and_valid_rules_apply() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "{bad\nx\n").file("/r/x", "").file("/r/y", "");
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    assert_eq!(ev, strs(&["E /r Ignore", "F /r/y"]));
}

#[test]
fn late_nul_is_found_by_the_preflight() {
    let mut t = FakeTree::new();
    let mut late = "hit\n".repeat(10_000).into_bytes();
    late.extend_from_slice(b"hit\0\n");
    t.bytes("/r/late.bin", &late).file("/r/text", "hit\n");
    let (out, walker) = search_all(&mut t, &WalkBuilder::new(["/r"]), "hit");
    assert_eq!(out, strs(&["B /r/late.bin", "/r/text:1:hit"]));
    assert_eq!(walker.stats().files_binary_skipped, 1);
    assert_eq!(walker.stats().files_scanned, 1);
    assert_eq!(walker.stats().bytes_read, late.len() as u64 + 2 * 4);

    // -a: searched once, NUL as content.
    let (out, walker) = search_all(
        &mut t,
        WalkBuilder::new(["/r/late.bin"]).binary(BinaryMode::Search),
        "\\x00",
    );
    assert_eq!(out, strs(&["/r/late.bin:10001:hit\0"]));
    assert_eq!(walker.stats().bytes_read, late.len() as u64);
}

#[test]
fn modification_fails_the_file() {
    // Modified during the preflight: nothing is emitted.
    let mut t = FakeTree::new();
    t.file("/r/a", "hit\nhit\n");
    t.modified_after_reads.insert(b"/r/a".to_vec(), 1);
    let (out, walker) = search_all(&mut t, &WalkBuilder::new(["/r"]), "hit");
    assert_eq!(out, strs(&["E /r/a FileModified"]));
    assert_eq!(walker.stats().errors, 1);

    // Modified during the only pass of -a: rows were emitted, then error.
    let (out, _) = search_all(
        &mut t,
        WalkBuilder::new(["/r"]).binary(BinaryMode::Search),
        "hit",
    );
    assert_eq!(
        out,
        strs(&["/r/a:1:hit", "/r/a:2:hit", "E /r/a FileModified"])
    );

    // Changed between the passes without a `modified` report: the NUL the
    // preflight did not see stops the search pass before the buffer that
    // holds it is searched and fails the file, never a silent stop.
    let mut t = FakeTree::new();
    t.file("/r/a", "hit\nhit\n");
    t.mutate_after_reads.insert(b"/r/a".to_vec(), (2, b"hit\n\0\n".to_vec()));
    let (out, _) = search_all(&mut t, &WalkBuilder::new(["/r/a"]), "hit");
    assert_eq!(out, strs(&["E /r/a FileModified"]));
}

#[test]
fn multi_chunk_file_bounded_buffer() {
    let mut t = FakeTree::new();
    t.chunk_max = 4000;
    let mut big = String::new();
    for i in 0..30_000u32 {
        if i % 7_919 == 0 {
            big.push_str(&format!("needle {i}\n"));
        } else {
            big.push_str("haystack line\n");
        }
    }
    assert!(big.len() > 64 * 1024 * 6);
    t.file("/r/big", &big);
    let (out, walker) =
        search_all(&mut t, &WalkBuilder::new(["/r"]), "needle");
    assert_eq!(
        out,
        strs(&[
            "/r/big:1:needle 0",
            "/r/big:7920:needle 7919",
            "/r/big:15839:needle 15838",
            "/r/big:23758:needle 23757",
        ])
    );
    assert_eq!(walker.stats().bytes_read, 2 * big.len() as u64);
    assert!(t.reads as u64 >= 2 * big.len() as u64 / 4000);
}

#[test]
fn long_line_over_heap_limit_is_a_sink_error() {
    let mut t = FakeTree::new();
    let line = "x".repeat(16 * 1024 + 65_536 + 1);
    t.file("/r/long", &line);
    let (out, _) = search_all(&mut t, &WalkBuilder::new(["/r"]), "x");
    assert_eq!(out.len(), 1);
    assert!(out[0].starts_with("S /r/long "), "{out:?}");
}

#[test]
fn cancellation_stops_reads_and_releases_handles() {
    let mut t = FakeTree::new();
    t.chunk_max = 10;
    t.file("/r/a", &"hit\n".repeat(100)).file("/r/b", "hit\n");
    let matcher = RegexMatcher::new_line_matcher("hit").unwrap();
    let mut srch = searcher();
    let mut walker = WalkBuilder::new(["/r"]).build();
    let flag = t.cancelled.clone();
    let polls = Cell::new(0usize);
    let mut cancel = || {
        polls.set(polls.get() + 1);
        let stop = polls.get() > 12;
        flag.set(flag.get() || stop);
        stop
    };
    let Some(WalkEvent::File { path, .. }) = walker.next(&mut t, &mut cancel)
    else {
        panic!("expected a file");
    };
    let reads_before = t.reads;
    let result = walker.search_file(
        &mut t,
        &path,
        &mut srch,
        &matcher,
        sinks::Bytes(|_, _| Ok(true)),
        &mut cancel,
    );
    assert!(matches!(result, Err(SearchFileError::Cancelled)));
    // Some reads happened, none after the callback asked to stop.
    let reads = t.reads - reads_before;
    assert!(reads > 0 && reads < 400 / 10, "{reads} reads");
    assert_eq!(t.ops_after_cancel, 0);
    t.assert_released();
    // The walk is over.
    assert!(walker.next(&mut t, &mut || false).is_none());

    // Cancel during listing: the directory handle is released.
    let mut t = FakeTree::new();
    t.page_size = 1;
    t.file("/r/a", "").file("/r/b", "").file("/r/c", "");
    let mut walker = WalkBuilder::new(["/r"]).build();
    let flag = t.cancelled.clone();
    let polls = Cell::new(0usize);
    let ev = walker.next(&mut t, &mut || {
        polls.set(polls.get() + 1);
        let stop = polls.get() > 2;
        flag.set(flag.get() || stop);
        stop
    });
    assert!(matches!(ev, Some(WalkEvent::Cancelled)));
    assert_eq!(t.ops_after_cancel, 0);
    assert!(t.listed.len() == 1);
    t.assert_released();
    assert!(walker.next(&mut t, &mut || false).is_none());
}

#[test]
fn every_handle_is_released() {
    let mut t = FakeTree::new();
    t.file("/r/.gitignore", "*.o\n")
        .file("/r/a.o", "")
        .file("/r/d/.ignore", "x\n")
        .file("/r/d/y", "hit\n")
        .bytes("/r/bin", b"\0")
        .file("/r/e/f/g", "hit\nhit\n");
    t.modified_after_reads.insert(b"/r/e/f/g".to_vec(), 1);
    let (out, _) = search_all(&mut t, &WalkBuilder::new(["/r"]), "hit");
    assert_eq!(
        out,
        strs(&["B /r/bin", "/r/d/y:1:hit", "E /r/e/f/g FileModified"])
    );
    t.assert_released();
    assert_eq!(t.peak_dirs, 1);
    assert_eq!(t.peak_files, 1);
}

#[test]
fn symlink_cycle_fake_terminates_at_a_bound() {
    let mut t = FakeTree::new();
    t.file("/r/f", "").dir("/r/loop");
    // Listing /r/loop lists /r again: an infinite tree.
    t.aliases.insert(b"/r/loop".to_vec(), b"/r".to_vec());
    let ev = events(&mut t, &WalkBuilder::new(["/r"]));
    let depth = WalkLimits::default().max_depth;
    assert_eq!(ev.len(), depth + 2);
    assert_eq!(ev[0], "F /r/f");
    let deepest = format!("/r{}", "/loop".repeat(depth + 1));
    assert_eq!(ev[depth + 1], format!("E {deepest} DepthLimit"));
}

#[test]
fn packed_children_keep_byte_order_and_kinds() {
    let mut t = FakeTree::new();
    t.page_size = 2;
    let long = "n".repeat(MAX_PACKED_NAME);
    t.file("/r/b", "")
        .file("/r/a", "")
        .file("/r/aa", "")
        .file("/r/a-", "")
        .file("/r/A", "")
        .file("/r/\u{e9}", "")
        .file(&format!("/r/{long}"), "")
        .kind("/r/mnt", EntryKind::Mount)
        .kind("/r/dev", EntryKind::Other)
        .file("/r/d/x", "");
    let limits = WalkLimits { max_path_len: 0x5000, ..WalkLimits::default() };
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    let long_file = format!("F /r/{long}");
    assert_eq!(
        ev,
        strs(&[
            "F /r/A",
            "F /r/a",
            "F /r/a-",
            "F /r/aa",
            "F /r/b",
            "F /r/d/x",
            "E /r/dev OtherKind",
            "M /r/mnt",
            &long_file,
            "F /r/\u{e9}",
        ])
    );
    // A name longer than the 14-bit length field is an invalid name.
    t.file(&format!("/r/{long}x"), "");
    let ev = events(&mut t, WalkBuilder::new(["/r"]).limits(limits));
    assert_eq!(ev, strs(&["E /r InvalidName"]));
}

// ------------------------------------------------------ segmented search

#[derive(Clone, Copy, Debug)]
enum Pause {
    Never,
    /// After every n-th row.
    Every(usize),
    /// After the row with this index only.
    At(usize),
}

/// Records every sink call; pauses after the rows `pause` selects.
struct Rows<'m> {
    matcher: &'m RegexMatcher,
    out: Vec<String>,
    rows: usize,
    pause: Pause,
}

impl<'m> Rows<'m> {
    fn new(matcher: &'m RegexMatcher, pause: Pause) -> Rows<'m> {
        Rows { matcher, out: vec![], rows: 0, pause }
    }

    fn row(&mut self, desc: String) -> bool {
        self.out.push(desc);
        let i = self.rows;
        self.rows += 1;
        !match self.pause {
            Pause::Never => false,
            Pause::Every(n) => (i + 1).is_multiple_of(n),
            Pause::At(k) => i == k,
        }
    }
}

impl Sink for Rows<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &Searcher,
        m: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let subs = line_submatches(self.matcher, m, 4_096).unwrap();
        let desc = format!(
            "M {}@{} {:?} {subs:?}",
            m.line_number().unwrap(),
            m.absolute_byte_offset(),
            s(m.bytes()),
        );
        Ok(self.row(desc))
    }

    fn context(
        &mut self,
        _: &Searcher,
        c: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        let desc = format!(
            "{:?} {}@{} {:?}",
            c.kind(),
            c.line_number().unwrap(),
            c.absolute_byte_offset(),
            s(c.bytes()),
        );
        Ok(self.row(desc))
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, io::Error> {
        self.out.push("--".to_string());
        Ok(true)
    }

    fn begin(&mut self, _: &Searcher) -> Result<bool, io::Error> {
        self.out.push("begin".to_string());
        Ok(true)
    }

    fn finish(
        &mut self,
        _: &Searcher,
        f: &SinkFinish,
    ) -> Result<(), io::Error> {
        let desc =
            format!("finish {} {:?}", f.byte_count(), f.binary_byte_offset());
        self.out.push(desc);
        Ok(())
    }
}

fn ctx_searcher(before: usize, after: usize) -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .before_context(before)
        .after_context(after)
        .buffer_capacity(16 * 1024)
        .heap_limit(Some(16 * 1024 + 65_536))
        .build()
}

struct Run {
    out: Vec<String>,
    pauses: usize,
    lines: u64,
    stats: WalkStats,
}

/// Walk to the first file of `builder` and search it with
/// `begin_file` + `step`, pausing per `pause`.
fn run_file(
    t: &mut FakeTree,
    builder: &WalkBuilder,
    pattern: &str,
    (before, after): (usize, usize),
    pause: Pause,
) -> Run {
    let matcher = RegexMatcher::new_line_matcher(pattern).unwrap();
    let mut srch = ctx_searcher(before, after);
    let mut walker = builder.build();
    let Some(WalkEvent::File { path, .. }) = walker.next(t, &mut || false)
    else {
        panic!("expected a file");
    };
    let begun = walker.begin_file(t, &path, &mut || false).unwrap();
    let Begun::Search(mut file) = begun else { panic!("binary") };
    assert_eq!(file.path(), path.as_slice());
    let mut sink = Rows::new(&matcher, pause);
    let mut pauses = 0;
    let mut never = || false;
    loop {
        let step = file
            .step(&mut walker, t, &mut srch, &matcher, &mut sink, &mut never)
            .unwrap();
        if step == Step::Finished {
            break;
        }
        pauses += 1;
    }
    t.assert_released();
    Run {
        out: sink.out,
        pauses,
        lines: file.lines_scanned(),
        stats: walker.stats(),
    }
}

fn row_count(out: &[String]) -> usize {
    out.iter()
        .filter(|r| r.as_str() != "--" && r.as_str() != "begin")
        .filter(|r| !r.starts_with("finish"))
        .count()
}

/// Pausing after any single row, and after every 1, 2 or 3 rows, gives
/// exactly the sink calls of the uninterrupted search (rows with line
/// numbers, offsets, bytes and submatches, context breaks, one `begin`, one
/// `finish`). Returns the uninterrupted calls.
fn assert_pause_invariant(
    t: &mut FakeTree,
    root: &str,
    pattern: &str,
    context: (usize, usize),
) -> Vec<String> {
    let builder = WalkBuilder::new([root]);
    let whole = run_file(t, &builder, pattern, context, Pause::Never);
    assert_eq!(whole.pauses, 0);
    let rows = row_count(&whole.out);
    assert!(rows > 0, "{pattern}: no rows");
    let mut schedules: Vec<Pause> = (0..rows).map(Pause::At).collect();
    schedules.extend([Pause::Every(1), Pause::Every(2), Pause::Every(3)]);
    for pause in schedules {
        let run = run_file(t, &builder, pattern, context, pause);
        let what = format!("{pattern} {context:?} {pause:?}");
        assert_eq!(run.out, whole.out, "{what}");
        assert_eq!(run.lines, whole.lines, "{what}");
        let want = match pause {
            Pause::Never => 0,
            Pause::Every(n) => rows / n,
            Pause::At(_) => 1,
        };
        assert_eq!(run.pauses, want, "{what}");
        assert_eq!(run.stats.files_scanned, 1, "{what}");
        assert_eq!(run.stats.errors, 0, "{what}");
    }
    whole.out
}

#[test]
fn pause_right_before_eof() {
    let mut t = FakeTree::new();
    t.file("/f", "x\nbeta 1\ny\nbeta 2");
    let out = assert_pause_invariant(&mut t, "/f", "beta", (0, 0));
    assert_eq!(
        out,
        strs(&[
            "begin",
            "M 2@2 \"beta 1\\n\" [(0, 4)]",
            "M 4@11 \"beta 2\" [(0, 4)]",
            "finish 17 None",
        ])
    );
    for contents in ["x\nbeta 1\ny\nbeta 2\n", "beta", "beta\n\n\n"] {
        let mut t = FakeTree::new();
        t.file("/f", contents);
        let out = assert_pause_invariant(&mut t, "/f", "beta", (0, 0));
        let finish = format!("finish {} None", contents.len());
        assert_eq!(out.last(), Some(&finish));
    }
}

#[test]
fn pause_on_after_context_rows() {
    let mut t = FakeTree::new();
    t.file("/f", "l1\nbeta a\nl3\nl4\nl5\nl6\nbeta b\nl8\nbeta c\nl10\nl11\n");
    for context in [(0, 1), (0, 2), (0, 3)] {
        let out = assert_pause_invariant(&mut t, "/f", "beta", context);
        assert!(out.iter().any(|r| r.starts_with("After ")), "{out:?}");
        assert!(out.contains(&"--".to_string()), "{out:?}");
    }
}

#[test]
fn pause_with_before_context_zero_and_twenty() {
    let mut contents = String::new();
    for i in 1..=80 {
        if [5, 9, 40, 70, 71, 80].contains(&i) {
            contents.push_str(&format!("beta {i}\n"));
        } else {
            contents.push_str(&format!("l{i}\n"));
        }
    }
    let mut t = FakeTree::new();
    t.file("/f", &contents);
    for context in [(0, 0), (0, 3), (20, 0), (20, 3), (1, 1), (2, 25)] {
        let out = assert_pause_invariant(&mut t, "/f", "beta", context);
        if context.0 == 20 {
            let first = out.iter().any(|r| r.starts_with("Before 20@"));
            assert!(first, "{out:?}");
        }
    }
}

/// `\A` disables the fast line path, so matching is per line; submatches
/// see `\A` only at the start of the input. Pausing does not change either.
#[test]
fn haystack_anchor_pause_matches_uninterrupted() {
    let mut t = FakeTree::new();
    t.file("/f", "beta 1\nbeta 2\nx beta\nbeta 3\ny\nz\nbeta 4\n");
    for context in [(0, 0), (1, 1), (0, 2), (2, 0)] {
        let out = assert_pause_invariant(&mut t, "/f", r"\Abeta", context);
        let matched: Vec<&String> =
            out.iter().filter(|r| r.starts_with("M ")).collect();
        assert_eq!(
            matched,
            [
                "M 1@0 \"beta 1\\n\" [(0, 4)]",
                "M 2@7 \"beta 2\\n\" []",
                "M 4@21 \"beta 3\\n\" []",
                "M 7@32 \"beta 4\\n\" []",
            ]
        );
    }
}

#[test]
fn big_file_pauses_are_served_by_the_window() {
    let mut big = String::new();
    for i in 0..30_000u32 {
        if i % 997 == 0 {
            big.push_str(&format!("needle {i}\n"));
        } else {
            big.push_str("haystack line\n");
        }
    }
    let mut t = FakeTree::new();
    t.file("/f", &big);
    let b = WalkBuilder::new(["/f"]);
    let whole = run_file(&mut t, &b, "needle", (1, 1), Pause::Never);
    let paused = run_file(&mut t, &b, "needle", (1, 1), Pause::Every(1));
    assert_eq!(paused.out, whole.out);
    assert_eq!(paused.pauses, row_count(&whole.out));
    assert_eq!(paused.lines, 30_000);
    // Preflight plus one search pass: every restart hit the window.
    assert_eq!(paused.stats.bytes_read, 2 * big.len() as u64);
    // Without a window every restart re-reads from the tree.
    let limits =
        WalkLimits { resume_window_bytes: 0, ..WalkLimits::default() };
    let mut b = WalkBuilder::new(["/f"]);
    b.limits(limits);
    let reread = run_file(&mut t, &b, "needle", (1, 1), Pause::Every(1));
    assert_eq!(reread.out, whole.out);
    assert_eq!(reread.lines, 30_000);
    assert!(reread.stats.bytes_read > 2 * big.len() as u64);
}

#[test]
fn modification_between_steps_fails_the_file() {
    let mut t = FakeTree::new();
    t.file("/f", "beta 1\nbeta 2\nbeta 3\n");
    let matcher = RegexMatcher::new_line_matcher("beta").unwrap();
    let mut srch = ctx_searcher(0, 0);
    let mut walker = WalkBuilder::new(["/f"]).build();
    let begun = walker.begin_file(&mut t, b"/f", &mut || false).unwrap();
    let Begun::Search(mut file) = begun else { panic!("binary") };
    let mut sink = Rows::new(&matcher, Pause::Every(1));
    let mut step = |file: &mut FileSearch<FakeFile>,
                    walker: &mut Walker,
                    t: &mut FakeTree| {
        file.step(walker, t, &mut srch, &matcher, &mut sink, &mut || false)
    };
    assert_eq!(step(&mut file, &mut walker, &mut t).unwrap(), Step::Paused);
    t.modified_after_reads.insert(b"/f".to_vec(), 0);
    let err = loop {
        match step(&mut file, &mut walker, &mut t) {
            Ok(Step::Paused) => {}
            Ok(Step::Finished) => panic!("the modification was not detected"),
            Err(err) => break err,
        }
    };
    let SearchFileError::Walk(err) = err else { panic!("{err}") };
    assert!(matches!(err.kind, WalkErrorKind::FileModified), "{err}");
    assert_eq!(err.path, b"/f");
    assert_eq!(walker.stats().errors, 1);
    assert_eq!(walker.stats().files_scanned, 0);
    t.assert_released();
    // A step after the handle was released does nothing.
    let reads = t.reads;
    assert_eq!(step(&mut file, &mut walker, &mut t).unwrap(), Step::Finished);
    assert_eq!(t.reads, reads);
    assert_eq!(walker.stats().files_scanned, 0);
}

#[test]
fn abandon_releases_the_handle() {
    let mut t = FakeTree::new();
    t.file("/f", "beta 1\nbeta 2\n").bytes("/b", b"beta\0\n");
    let matcher = RegexMatcher::new_line_matcher("beta").unwrap();
    let mut srch = ctx_searcher(0, 0);
    let mut walker = WalkBuilder::new(["/f"]).build();
    let begun = walker.begin_file(&mut t, b"/f", &mut || false).unwrap();
    let Begun::Search(mut file) = begun else { panic!("binary") };
    assert_eq!(t.open_files, 1);
    let mut sink = Rows::new(&matcher, Pause::Every(1));
    let mut never = || false;
    let step = file
        .step(&mut walker, &mut t, &mut srch, &matcher, &mut sink, &mut never)
        .unwrap();
    assert_eq!(step, Step::Paused);
    assert_eq!(t.open_files, 1);
    file.abandon(&mut t);
    t.assert_released();
    assert_eq!(walker.stats().files_scanned, 0);
    assert_eq!(walker.stats().errors, 0);
    // A binary file is released by `begin_file`.
    let begun = walker.begin_file(&mut t, b"/b", &mut || false).unwrap();
    assert!(matches!(begun, Begun::Binary));
    t.assert_released();
    assert_eq!(walker.stats().files_binary_skipped, 1);
}

/// An in-memory [`ResumeInput`] that counts reads.
struct MemInput {
    data: Vec<u8>,
    rewindable: bool,
    furthest: u64,
    /// Reads that returned bytes.
    data_reads: usize,
    /// Reads below the furthest offset read.
    rereads: usize,
}

impl MemInput {
    fn new(data: &str, rewindable: bool) -> MemInput {
        MemInput {
            data: data.as_bytes().to_vec(),
            rewindable,
            furthest: 0,
            data_reads: 0,
            rereads: 0,
        }
    }
}

impl ResumeInput for MemInput {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error> {
        if offset < self.furthest {
            assert!(self.rewindable, "a stream was rewound");
            self.rereads += 1;
        }
        let start = (offset as usize).min(self.data.len());
        let n = buf.len().min(4000).min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.furthest = self.furthest.max(offset + n as u64);
        if n > 0 {
            self.data_reads += 1;
        }
        Ok(n)
    }

    fn rewindable(&self) -> bool {
        self.rewindable
    }
}

struct ResumeRun {
    out: Vec<String>,
    pauses: usize,
    lines: u64,
    bytes_read: u64,
}

fn resume_run(
    input: &mut MemInput,
    window: usize,
    pattern: &str,
    context: (usize, usize),
    pause: Pause,
) -> Result<ResumeRun, ResumeError<io::Error>> {
    let matcher = RegexMatcher::new_line_matcher(pattern).unwrap();
    let mut srch = ctx_searcher(context.0, context.1);
    let mut search = ResumableSearch::new(window);
    let mut sink = Rows::new(&matcher, pause);
    let mut pauses = 0;
    while search.step(input, &mut srch, &matcher, &mut sink, &mut || false)?
        == Step::Paused
    {
        pauses += 1;
    }
    Ok(ResumeRun {
        out: sink.out,
        pauses,
        lines: search.lines_scanned(),
        bytes_read: search.bytes_read(),
    })
}

const RESUME_DATA: &str = "beta 1\nx\nbeta 2\ny\nz\nbeta 3\nw\nbeta 4";

#[test]
fn resume_window_hit_serves_from_memory() {
    let mut whole_input = MemInput::new(RESUME_DATA, true);
    let whole =
        resume_run(&mut whole_input, 1024, "beta", (1, 1), Pause::Never)
            .unwrap();
    // A stream is fine: every restart lies inside the window.
    let mut input = MemInput::new(RESUME_DATA, false);
    let run =
        resume_run(&mut input, 1024, "beta", (1, 1), Pause::Every(1)).unwrap();
    assert_eq!(run.out, whole.out);
    assert_eq!(run.pauses, row_count(&whole.out));
    assert_eq!(input.rereads, 0);
    assert_eq!(input.data_reads, whole_input.data_reads);
    assert_eq!(run.bytes_read, RESUME_DATA.len() as u64);
    assert_eq!(run.lines, 8);
}

#[test]
fn resume_window_miss_rereads_a_rewindable_input() {
    let mut whole_input = MemInput::new(RESUME_DATA, true);
    let whole =
        resume_run(&mut whole_input, 4, "beta", (1, 1), Pause::Never).unwrap();
    for window in [0, 4] {
        let mut input = MemInput::new(RESUME_DATA, true);
        let run =
            resume_run(&mut input, window, "beta", (1, 1), Pause::Every(1))
                .unwrap();
        assert_eq!(run.out, whole.out, "window {window}");
        assert!(input.rereads > 0, "window {window}");
        assert!(run.bytes_read > RESUME_DATA.len() as u64, "window {window}");
        assert_eq!(run.lines, 8, "window {window}");
    }
}

#[test]
fn resume_window_miss_on_a_stream_is_an_error() {
    let mut input = MemInput::new(RESUME_DATA, false);
    let err = resume_run(&mut input, 4, "beta", (1, 1), Pause::Every(1));
    assert!(matches!(err, Err(ResumeError::WindowEvicted)));
    assert_eq!(input.rereads, 0);
}

#[test]
fn lines_scanned_counts_each_line_once() {
    let cases = [
        ("a\nbeta\nc\nbeta", 4),
        ("a\nbeta\n", 2),
        ("", 0),
        ("beta", 1),
        ("\n\nbeta\n\n", 4),
    ];
    for (data, lines) in cases {
        for window in [0, 4, 1024] {
            for pause in [Pause::Never, Pause::Every(1)] {
                let mut input = MemInput::new(data, true);
                let run =
                    resume_run(&mut input, window, "beta", (1, 0), pause)
                        .unwrap();
                assert_eq!(run.lines, lines, "{data:?} {window} {pause:?}");
            }
        }
    }
    // A final unterminated line counts once the search has finished.
    let matcher = RegexMatcher::new_line_matcher("beta").unwrap();
    let mut srch = ctx_searcher(0, 0);
    let mut search = ResumableSearch::new(1024);
    let mut input = MemInput::new("beta\nx\nbeta", true);
    let mut sink = Rows::new(&matcher, Pause::Every(1));
    let mut step = |search: &mut ResumableSearch| {
        search.step(&mut input, &mut srch, &matcher, &mut sink, &mut || false)
    };
    assert_eq!(step(&mut search).unwrap(), Step::Paused);
    assert_eq!(search.lines_scanned(), 2);
    assert_eq!(step(&mut search).unwrap(), Step::Paused);
    assert_eq!(step(&mut search).unwrap(), Step::Finished);
    assert_eq!(search.lines_scanned(), 3);
    assert_eq!(step(&mut search).unwrap(), Step::Finished);
    assert_eq!(search.lines_scanned(), 3);
}

/// Calls `line_submatches` for every matched line.
struct Subs<'m> {
    matcher: &'m RegexMatcher,
    max: usize,
    out: Vec<Result<Vec<(usize, usize)>, SubmatchError>>,
}

impl Sink for Subs<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &Searcher,
        m: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        self.out.push(line_submatches(self.matcher, m, self.max));
        Ok(true)
    }
}

fn submatches(
    pattern: &str,
    haystack: &str,
    max: usize,
) -> Vec<Result<Vec<(usize, usize)>, SubmatchError>> {
    let matcher = RegexMatcher::new_line_matcher(pattern).unwrap();
    let mut sink = Subs { matcher: &matcher, max, out: vec![] };
    searcher().search_slice(&matcher, haystack.as_bytes(), &mut sink).unwrap();
    sink.out
}

#[test]
fn line_submatches_cap() {
    let line = |n: usize| format!("{}\n", "a".repeat(n));
    let got = submatches("a", &line(4_096), 4_096);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].as_ref().unwrap().len(), 4_096);
    let got = submatches("a", &line(4_097), 4_096);
    assert_eq!(got, [Err(SubmatchError::Limit)]);
    assert_eq!(submatches("a", "xaxa\n", 1), [Err(SubmatchError::Limit)]);
    // The final empty match at the end of the input is dropped, so it does
    // not count against the cap.
    assert_eq!(submatches("", "ab", 2), [Ok(vec![(0, 0), (1, 1)])]);
    assert_eq!(submatches("", "ab", 1), [Err(SubmatchError::Limit)]);
}
