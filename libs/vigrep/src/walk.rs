//! A serial, depth-first walker over a [`ViSearchTree`].
//!
//! This module is written for Cellos; it replaces upstream ignore's
//! `walk.rs` (walkdir, threads, `std::fs`) and drives the ported precedence
//! logic of upstream's `dir.rs` ([`crate::ignore`]) and the ported searcher.
//!
//! # Semantics
//!
//! * Roots are processed in the order given. A root equal to, or below, an
//!   earlier root is skipped, and an entry whose path equals an earlier root
//!   is skipped while walking a later root: the first occurrence wins (paths
//!   are canonical, so this is the path-based dedup of DV7-3).
//! * Each directory is listed completely (page by page), its handle is
//!   closed, and its children are visited in ascending byte order of their
//!   names, depth first. No directory handle is held between calls. A file
//!   handle is held only by a [`FileSearch`] between its steps; every other
//!   return, including cancellation, releases every handle.
//! * Explicit roots bypass hidden, ignore and glob filtering; a directory
//!   root is always entered. Below a root, ignore files `.rgignore`,
//!   `.ignore` and `.gitignore` are read in every entered directory, from
//!   the root downward, with upstream's precedence (see [`crate::ignore`]).
//!   Command globs ([`crate::ignore::overrides::Override`]) take precedence
//!   over ignore files and hidden filtering. Hidden means a dot-prefixed
//!   name. Ignored directories are pruned, so a later negation cannot revive
//!   their descendants.
//! * Children of kind [`EntryKind::Mount`] are skipped and reported
//!   (DV7-7); children of kind [`EntryKind::Other`] are a per-entry error
//!   (DV7-5). Nothing is followed: there is no link kind.
//! * A listing reported as changed while it was read is an error for that
//!   directory. A file whose [`FileState::modified`] is set after the
//!   binary preflight or after the search pass is an error (DV7-4).
//! * In [`BinaryMode::Skip`] (the default), a file is read once completely
//!   to look for a NUL byte; a file with a NUL anywhere is skipped before
//!   any search output (stable binary preflight). [`BinaryMode::Search`]
//!   (`-a`) searches every file once, NUL bytes included.
//! * [`WalkLimits`] bounds depth, visited entries, entries per directory,
//!   pending children (packed bytes and entries), path length and ignore
//!   files (DV7-6, D76-4 and the `rg` contract limits). Exceeding a limit is
//!   an error, never a silent truncation; the visited-entry limit ends the
//!   walk.
//! * [`Walker::begin_file`] and [`FileSearch::step`] search one file in
//!   segments: a sink may pause after any row and the next step resumes
//!   exactly where one uninterrupted search would continue (see
//!   [`ResumableSearch`]). [`Walker::search_file`] is the unpaused form.
//! * The cancel callback is polled once per entry, per listing page and per
//!   read chunk. When it returns true the walk stops after releasing every
//!   handle.
//!
//! Errors from one entry never stop the walk (except the visited-entry
//! limit); the caller decides how to report them.

use alloc::collections::VecDeque;
use core::{convert::Infallible, fmt};

use memchr::{memchr, memchr_iter};

use crate::{
    ignore::{self, Ignore, IgnoreBuilder, overrides::Override},
    io,
    matcher::{Match, Matcher},
    prelude::*,
    searcher::{
        BinaryDetection, Searcher, Sink, SinkContext, SinkContextKind,
        SinkError, SinkFinish, SinkMatch,
    },
};

#[cfg(test)]
mod tests;

/// The kind of a directory entry, as reported by a [`ViSearchTree`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory on the same mount.
    Dir,
    /// The root of another mount. Never entered below an explicit root.
    Mount,
    /// Anything else. Reported as an error for that entry.
    Other,
}

/// One directory entry: a kind and a single path component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    /// The kind of the entry.
    pub kind: EntryKind,
    /// The entry's name: one non-empty path component without `/`, never
    /// `.` or `..`.
    pub name: Vec<u8>,
}

/// The status of one listing page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageInfo {
    /// This was the final page of the listing.
    pub complete: bool,
    /// The directory changed while the listing was read, so the listing
    /// may be inconsistent.
    pub changed: bool,
}

/// The state of an open file.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileState {
    /// The current size of the file in bytes.
    pub size: u64,
    /// The file was modified (content or binding) after it was opened.
    pub modified: bool,
}

/// A handle to release with [`ViSearchTree::close`].
#[derive(Debug)]
pub enum Handle<D, F> {
    /// An open directory.
    Dir(D),
    /// An open file.
    File(F),
}

/// A tree of directories and files the walker can search.
///
/// Paths are canonical byte paths: `/`-separated, no empty, `.` or `..`
/// components and no trailing `/` except for the root `/`. Every method
/// enforces the implementation's own authority checks; the walker never
/// assumes access. Errors use [`io::ErrorKind`]; [`ViSearchTree::open_dir`]
/// must return [`io::ErrorKind::NotADirectory`] when `path` names a file.
pub trait ViSearchTree {
    /// An open directory.
    type Dir;
    /// An open file.
    type File;

    /// Open the directory at `path` for listing.
    fn open_dir(&mut self, path: &[u8]) -> Result<Self::Dir, io::Error>;

    /// Append the next page of `dir`'s entries to `entries`.
    fn next_page(
        &mut self,
        dir: &mut Self::Dir,
        entries: &mut Vec<DirEntry>,
    ) -> Result<PageInfo, io::Error>;

    /// Open the regular file at `path` for reading.
    fn open_file(&mut self, path: &[u8]) -> Result<Self::File, io::Error>;

    /// Read bytes of `file` starting at `offset` into a prefix of `buf` and
    /// return their count; `0` means end of file.
    fn read_chunk(
        &mut self,
        file: &mut Self::File,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error>;

    /// The current state of `file`.
    fn file_state(
        &mut self,
        file: &mut Self::File,
    ) -> Result<FileState, io::Error>;

    /// Release a handle.
    fn close(&mut self, handle: Handle<Self::Dir, Self::File>);
}

/// How files containing a NUL byte are treated.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BinaryMode {
    /// Read the whole file first; skip it if it contains a NUL byte.
    #[default]
    Skip,
    /// Search every file once, treating NUL bytes as content (`-a`).
    Search,
}

/// Resource bounds of a walk. Exceeding any of them is an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalkLimits {
    /// Maximum directory depth below each root.
    pub max_depth: usize,
    /// Maximum number of entries visited in the whole walk; exceeding it
    /// ends the walk.
    pub max_entries: u64,
    /// Maximum number of entries in one directory.
    pub max_dir_entries: usize,
    /// Maximum bytes of pending children (packed: name plus a 2-byte
    /// header) retained across the whole directory stack.
    pub max_pending_bytes: usize,
    /// Maximum number of pending children retained across the whole
    /// directory stack.
    pub max_pending_entries: usize,
    /// Maximum length of a path in bytes.
    pub max_path_len: usize,
    /// Maximum size of one ignore file in bytes.
    pub max_ignore_file_bytes: usize,
    /// Maximum number of rules in one ignore file.
    pub max_ignore_rules_per_file: usize,
    /// Maximum total bytes of ignore files retained across the stack.
    pub max_ignore_bytes_total: usize,
    /// Maximum total number of ignore rules retained across the stack. A
    /// compiled rule costs far more memory than its source bytes (up to about
    /// 10 KB for a glob that needs its own regex), so this bounds the ignore
    /// matchers where [`WalkLimits::max_ignore_bytes_total`] cannot.
    pub max_ignore_rules_total: usize,
    /// Size of the read scratch used for binary preflight and ignore files.
    pub scratch_bytes: usize,
    /// Bytes of recently searched file content a [`FileSearch`] keeps so a
    /// resumed step does not re-read them from the tree.
    pub resume_window_bytes: usize,
}

impl Default for WalkLimits {
    fn default() -> WalkLimits {
        WalkLimits {
            max_depth: 64,
            max_entries: 65_536,
            max_dir_entries: 4_096,
            max_pending_bytes: 1 << 20,
            max_pending_entries: 65_536,
            max_path_len: 1_024,
            max_ignore_file_bytes: 65_536,
            max_ignore_rules_per_file: 1_024,
            max_ignore_bytes_total: 256 * 1024,
            max_ignore_rules_total: 65_536,
            scratch_bytes: 16 * 1024,
            resume_window_bytes: 32 * 1024,
        }
    }
}

/// Why an entry failed.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum WalkErrorKind {
    /// The tree reported an error.
    Io(io::Error),
    /// An ignore file could not be fully applied (malformed rule or
    /// non-UTF-8 line); its valid rules are in effect.
    Ignore(ignore::Error),
    /// An entry of kind [`EntryKind::Other`] (DV7-5).
    OtherKind,
    /// A listing returned a name that is not a single path component.
    InvalidName,
    /// The directory changed while it was listed.
    ListingChanged,
    /// The file was modified while it was read (DV7-4).
    FileModified,
    /// [`WalkLimits::max_depth`] exceeded; the directory was not entered.
    DepthLimit,
    /// [`WalkLimits::max_entries`] exceeded; the walk ended.
    EntryLimit,
    /// [`WalkLimits::max_dir_entries`] exceeded; the directory was skipped.
    DirEntryLimit,
    /// [`WalkLimits::max_pending_bytes`] or
    /// [`WalkLimits::max_pending_entries`] exceeded; the directory was
    /// skipped.
    PendingNamesLimit,
    /// [`WalkLimits::max_path_len`] exceeded.
    PathTooLong,
    /// [`WalkLimits::max_ignore_file_bytes`] exceeded; the directory was
    /// skipped.
    IgnoreFileTooLarge,
    /// [`WalkLimits::max_ignore_rules_per_file`] exceeded; the directory was
    /// skipped.
    IgnoreRulesLimit,
    /// [`WalkLimits::max_ignore_bytes_total`] or
    /// [`WalkLimits::max_ignore_rules_total`] exceeded; the directory was
    /// skipped.
    IgnoreBytesLimit,
}

/// A per-entry walk error.
#[derive(Clone, Debug)]
pub struct WalkError {
    /// The path of the entry (or ignore file) that failed.
    pub path: Vec<u8>,
    /// Why it failed.
    pub kind: WalkErrorKind,
}

impl WalkError {
    fn new(path: &[u8], kind: WalkErrorKind) -> WalkError {
        WalkError { path: path.to_vec(), kind }
    }
}

impl fmt::Display for WalkErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WalkErrorKind::Io(err) => err.fmt(f),
            WalkErrorKind::Ignore(err) => err.fmt(f),
            WalkErrorKind::OtherKind => {
                f.write_str("unsupported entry kind (not a file or directory)")
            }
            WalkErrorKind::InvalidName => f.write_str("invalid entry name"),
            WalkErrorKind::ListingChanged => {
                f.write_str("directory changed while it was listed")
            }
            WalkErrorKind::FileModified => {
                f.write_str("file changed while it was read")
            }
            WalkErrorKind::DepthLimit => f.write_str("directory depth limit"),
            WalkErrorKind::EntryLimit => f.write_str("visited entry limit"),
            WalkErrorKind::DirEntryLimit => {
                f.write_str("directory entry limit")
            }
            WalkErrorKind::PendingNamesLimit => {
                f.write_str("pending entry limit")
            }
            WalkErrorKind::PathTooLong => f.write_str("path length limit"),
            WalkErrorKind::IgnoreFileTooLarge => {
                f.write_str("ignore file size limit")
            }
            WalkErrorKind::IgnoreRulesLimit => {
                f.write_str("ignore file rule limit")
            }
            WalkErrorKind::IgnoreBytesLimit => {
                f.write_str("retained ignore rule limit")
            }
        }
    }
}

impl fmt::Display for WalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", bstr::BStr::new(&self.path), self.kind)
    }
}

impl core::error::Error for WalkError {}

/// One step of a walk.
#[derive(Clone, Debug)]
pub enum WalkEvent {
    /// A file admitted for search; pass `path` to [`Walker::search_file`]
    /// or [`Walker::begin_file`].
    File {
        /// The canonical path of the file.
        path: Vec<u8>,
        /// The file was a root operand (filters were not applied).
        explicit: bool,
    },
    /// A child of kind [`EntryKind::Mount`] was skipped (DV7-7).
    MountSkipped {
        /// The path of the mount root.
        path: Vec<u8>,
    },
    /// An entry failed; the walk continues unless it ended.
    Error(WalkError),
    /// The cancel callback returned true. Every handle was released and the
    /// walk ended.
    Cancelled,
}

/// The result of [`Walker::search_file`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileOutcome {
    /// The file was searched completely.
    Searched,
    /// The binary preflight found a NUL byte; nothing was searched.
    Binary,
}

/// Why [`Walker::search_file`], [`Walker::begin_file`] or
/// [`FileSearch::step`] failed.
#[derive(Debug)]
pub enum SearchFileError<E> {
    /// The tree or the file failed; see [`WalkError`].
    Walk(WalkError),
    /// The sink (or the searcher, e.g. its heap limit) returned an error.
    Sink(E),
    /// The cancel callback returned true. The file handle was released and
    /// the walk ended.
    Cancelled,
}

impl<E: fmt::Display> fmt::Display for SearchFileError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchFileError::Walk(err) => err.fmt(f),
            SearchFileError::Sink(err) => err.fmt(f),
            SearchFileError::Cancelled => f.write_str("cancelled"),
        }
    }
}

/// The result of one [`FileSearch::step`] or [`ResumableSearch::step`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    /// The sink paused after accepting a row; call `step` again to resume.
    Paused,
    /// The search reached the end of the input (or the sink stopped it from
    /// `begin`, `context_break` or `binary_data`).
    Finished,
}

/// The result of [`Walker::begin_file`].
#[derive(Debug)]
pub enum Begun<F> {
    /// The binary preflight found a NUL byte; the handle was released and
    /// nothing is searched.
    Binary,
    /// The file is open and ready for [`FileSearch::step`].
    Search(FileSearch<F>),
}

/// Counters of a walk.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WalkStats {
    /// Entries visited below roots.
    pub entries: u64,
    /// Files searched completely (a [`Step::Finished`] that passed the final
    /// state check).
    pub files_scanned: u64,
    /// Files skipped by the binary preflight.
    pub files_binary_skipped: u64,
    /// Mount children skipped.
    pub mounts_skipped: u64,
    /// Roots skipped because an earlier root covers them.
    pub roots_deduplicated: u64,
    /// Bytes read from searched files, including preflight scans and
    /// re-reads of resumed steps (resume window hits are not reads).
    pub bytes_read: u64,
    /// Bytes read from ignore files.
    pub ignore_bytes_read: u64,
    /// Errors reported by [`Walker::next`], [`Walker::search_file`],
    /// [`Walker::begin_file`] and [`FileSearch::step`].
    pub errors: u64,
}

/// Builds a [`Walker`].
#[derive(Clone, Debug)]
pub struct WalkBuilder {
    roots: Vec<Vec<u8>>,
    hidden: bool,
    ignore_files: bool,
    overrides: Override,
    binary: BinaryMode,
    limits: WalkLimits,
}

impl WalkBuilder {
    /// A walk over `roots`, in order.
    pub fn new<I, P>(roots: I) -> WalkBuilder
    where
        I: IntoIterator<Item = P>,
        P: AsRef<[u8]>,
    {
        WalkBuilder {
            roots: roots.into_iter().map(|r| r.as_ref().to_vec()).collect(),
            hidden: false,
            ignore_files: true,
            overrides: Override::empty(),
            binary: BinaryMode::Skip,
            limits: WalkLimits::default(),
        }
    }

    /// Include hidden (dot-prefixed) entries (`--hidden`). Default: false.
    pub fn hidden(&mut self, yes: bool) -> &mut WalkBuilder {
        self.hidden = yes;
        self
    }

    /// Read `.rgignore`, `.ignore` and `.gitignore` files. Default: true;
    /// false is `--no-ignore`.
    pub fn ignore_files(&mut self, yes: bool) -> &mut WalkBuilder {
        self.ignore_files = yes;
        self
    }

    /// Command globs (`-g`), built relative to the directory the globs are
    /// interpreted from. Default: none.
    pub fn overrides(&mut self, overrides: Override) -> &mut WalkBuilder {
        self.overrides = overrides;
        self
    }

    /// Binary file policy. Default: [`BinaryMode::Skip`].
    pub fn binary(&mut self, mode: BinaryMode) -> &mut WalkBuilder {
        self.binary = mode;
        self
    }

    /// Resource bounds. Default: [`WalkLimits::default`].
    pub fn limits(&mut self, limits: WalkLimits) -> &mut WalkBuilder {
        self.limits = limits;
        self
    }

    /// Build the walker.
    pub fn build(&self) -> Walker {
        let mut ig = IgnoreBuilder::new();
        ig.hidden(!self.hidden)
            .ignore(self.ignore_files)
            .git_ignore(self.ignore_files)
            .overrides(self.overrides.clone());
        if self.ignore_files {
            ig.add_custom_ignore_filename(".rgignore");
        }
        let mut roots = self.roots.clone();
        roots.reverse();
        Walker {
            roots,
            done_roots: Vec::new(),
            stack: Vec::new(),
            ig_root: ig.build(),
            binary: self.binary,
            limits: self.limits,
            stats: WalkStats::default(),
            pending_bytes: 0,
            pending_entries: 0,
            ignore_bytes: 0,
            ignore_rules: 0,
            finished: false,
            scratch: Vec::new(),
        }
    }
}

/// One entered directory.
struct Frame {
    dir: Vec<u8>,
    depth: usize,
    ig: Ignore,
    /// Pending children, packed in descending name order so they pop from
    /// the end in ascending order. Each child is its name followed by a
    /// 2-byte header (see [`pack_header`]).
    children: Vec<u8>,
    /// Bytes of `children` counted in `Walker::pending_bytes`.
    pending_bytes: usize,
    /// Children counted in `Walker::pending_entries`.
    pending_entries: usize,
    /// Bytes of this directory's ignore files counted in
    /// `Walker::ignore_bytes`.
    ignore_bytes: usize,
    /// Rules of this directory's ignore files counted in
    /// `Walker::ignore_rules`.
    ignore_rules: usize,
}

/// A serial depth-first walk; see the module documentation.
pub struct Walker {
    /// Roots not yet started, in reverse order.
    roots: Vec<Vec<u8>>,
    /// Roots already started.
    done_roots: Vec<Vec<u8>>,
    stack: Vec<Frame>,
    ig_root: Ignore,
    binary: BinaryMode,
    limits: WalkLimits,
    stats: WalkStats,
    pending_bytes: usize,
    pending_entries: usize,
    ignore_bytes: usize,
    ignore_rules: usize,
    finished: bool,
    scratch: Vec<u8>,
}

impl fmt::Debug for Walker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Walker")
            .field("depth", &self.stack.len())
            .field("stats", &self.stats)
            .field("finished", &self.finished)
            .finish()
    }
}

impl Walker {
    /// The counters so far.
    pub fn stats(&self) -> WalkStats {
        self.stats
    }

    /// The configured limits.
    pub fn limits(&self) -> &WalkLimits {
        &self.limits
    }

    /// Advance the walk to the next event, or `None` when it has ended.
    pub fn next<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Option<WalkEvent> {
        loop {
            if self.finished {
                return None;
            }
            if cancel() {
                return Some(self.cancelled());
            }
            let Some(frame) = self.stack.last_mut() else {
                let root = self.roots.pop()?;
                if let Some(event) = self.start_root(tree, root, cancel) {
                    return Some(self.count(event));
                }
                continue;
            };
            let Some((kind, start)) = last_packed(&frame.children) else {
                self.pop_frame();
                continue;
            };
            let parent_depth = frame.depth;
            let name = &frame.children[start..frame.children.len() - 2];
            let path = ignore_join(&frame.dir, name);
            let packed = frame.children.len() - start;
            frame.children.truncate(start);
            frame.pending_bytes -= packed;
            frame.pending_entries -= 1;
            self.pending_bytes -= packed;
            self.pending_entries -= 1;
            self.stats.entries += 1;
            if self.stats.entries > self.limits.max_entries {
                self.finish_walk();
                let err = WalkError::new(&path, WalkErrorKind::EntryLimit);
                return Some(self.count(WalkEvent::Error(err)));
            }
            if path.len() > self.limits.max_path_len {
                let err = WalkError::new(&path, WalkErrorKind::PathTooLong);
                return Some(self.count(WalkEvent::Error(err)));
            }
            if self.done_roots.contains(&path) {
                continue;
            }
            let is_dir = match kind {
                EntryKind::Mount => {
                    self.stats.mounts_skipped += 1;
                    return Some(WalkEvent::MountSkipped { path });
                }
                EntryKind::Other => {
                    let err = WalkError::new(&path, WalkErrorKind::OtherKind);
                    return Some(self.count(WalkEvent::Error(err)));
                }
                EntryKind::File => false,
                EntryKind::Dir => true,
            };
            let frame = self.stack.last().unwrap();
            let m = frame.ig.matched_dir_entry(&path, is_dir);
            if m.is_ignore() {
                log::debug!("ignoring {}: {:?}", bstr::BStr::new(&path), m);
                continue;
            }
            if !is_dir {
                return Some(WalkEvent::File { path, explicit: false });
            }
            if parent_depth + 1 > self.limits.max_depth {
                let err = WalkError::new(&path, WalkErrorKind::DepthLimit);
                return Some(self.count(WalkEvent::Error(err)));
            }
            let ig = frame.ig.clone();
            if let Some(event) =
                self.enter_dir(tree, path, parent_depth + 1, &ig, cancel)
            {
                return Some(self.count(event));
            }
        }
    }

    /// Search the file at `path` with `searcher`/`matcher`, reporting to
    /// `sink`.
    ///
    /// This is [`Walker::begin_file`] followed by [`FileSearch::step`] until
    /// [`Step::Finished`]. A sink returning `Ok(false)` from `matched` or
    /// `context` pauses (it never stops), so the file is still searched to
    /// its end. In [`BinaryMode::Skip`] the file is first read completely;
    /// if it contains a NUL byte, `Ok(FileOutcome::Binary)` is returned and
    /// `sink` sees nothing. The searcher's binary detection is set by this
    /// call. The file's state is checked after the preflight and after the
    /// search; a modification is [`WalkErrorKind::FileModified`] (rows
    /// already given to `sink` by the search pass then belong to a failed
    /// file). The file handle is always released before returning.
    pub fn search_file<T, M, S>(
        &mut self,
        tree: &mut T,
        path: &[u8],
        searcher: &mut Searcher,
        matcher: M,
        mut sink: S,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<FileOutcome, SearchFileError<S::Error>>
    where
        T: ViSearchTree,
        M: Matcher,
        S: Sink,
    {
        let mut file = match self.begin_file(tree, path, cancel) {
            Ok(Begun::Binary) => return Ok(FileOutcome::Binary),
            Ok(Begun::Search(file)) => file,
            Err(SearchFileError::Walk(err)) => {
                return Err(SearchFileError::Walk(err));
            }
            Err(SearchFileError::Cancelled) => {
                return Err(SearchFileError::Cancelled);
            }
            Err(SearchFileError::Sink(never)) => match never {},
        };
        loop {
            let step =
                file.step(self, tree, searcher, &matcher, &mut sink, cancel)?;
            if step == Step::Finished {
                return Ok(FileOutcome::Searched);
            }
        }
    }

    /// Open the file at `path` for a segmented search.
    ///
    /// In [`BinaryMode::Skip`] the file is first read completely; if it
    /// contains a NUL byte the handle is released and [`Begun::Binary`] is
    /// returned. The file's state is then checked; a modification is
    /// [`WalkErrorKind::FileModified`]. On success the returned
    /// [`FileSearch`] holds the file handle until [`FileSearch::step`]
    /// returns anything but [`Step::Paused`], or [`FileSearch::abandon`] is
    /// called. On error the handle is released.
    pub fn begin_file<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        path: &[u8],
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Begun<T::File>, SearchFileError<Infallible>> {
        let result = self.begin_file_inner(tree, path, cancel);
        match result {
            Ok(Begun::Search(_)) => {}
            Ok(Begun::Binary) => self.stats.files_binary_skipped += 1,
            Err(SearchFileError::Cancelled) => self.finish_walk(),
            Err(_) => self.stats.errors += 1,
        }
        result
    }

    fn begin_file_inner<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        path: &[u8],
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Begun<T::File>, SearchFileError<Infallible>> {
        let walk_err =
            |kind| SearchFileError::Walk(WalkError::new(path, kind));
        if cancel() {
            return Err(SearchFileError::Cancelled);
        }
        let file = tree
            .open_file(path)
            .map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
        let mut file = OpenFile { tree, file: Some(file) };
        if self.binary == BinaryMode::Skip {
            self.scratch.resize(self.limits.scratch_bytes.max(1), 0);
            let mut offset = 0u64;
            loop {
                if cancel() {
                    return Err(SearchFileError::Cancelled);
                }
                let n = file
                    .read_chunk(offset, &mut self.scratch)
                    .map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
                if n == 0 {
                    break;
                }
                self.stats.bytes_read += n as u64;
                if memchr(0, &self.scratch[..n]).is_some() {
                    return Ok(Begun::Binary);
                }
                offset += n as u64;
            }
            let state =
                file.state().map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
            if state.modified {
                return Err(walk_err(WalkErrorKind::FileModified));
            }
        }
        Ok(Begun::Search(FileSearch {
            path: path.to_vec(),
            file: file.file.take(),
            search: ResumableSearch::new(self.limits.resume_window_bytes),
        }))
    }

    fn count(&mut self, event: WalkEvent) -> WalkEvent {
        if let WalkEvent::Error(_) = event {
            self.stats.errors += 1;
        }
        event
    }

    fn cancelled(&mut self) -> WalkEvent {
        self.finish_walk();
        WalkEvent::Cancelled
    }

    /// End the walk: drop all pending state.
    fn finish_walk(&mut self) {
        self.finished = true;
        while !self.stack.is_empty() {
            self.pop_frame();
        }
        self.roots.clear();
    }

    fn pop_frame(&mut self) {
        let frame = self.stack.pop().unwrap();
        self.pending_bytes -= frame.pending_bytes;
        self.pending_entries -= frame.pending_entries;
        self.ignore_bytes -= frame.ignore_bytes;
        self.ignore_rules -= frame.ignore_rules;
    }

    /// Start the walk of one root; returns an event to report, if any.
    fn start_root<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        root: Vec<u8>,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Option<WalkEvent> {
        if root.len() > self.limits.max_path_len {
            let err = WalkError::new(&root, WalkErrorKind::PathTooLong);
            return Some(WalkEvent::Error(err));
        }
        if self.done_roots.iter().any(|r| is_same_or_below(&root, r)) {
            self.stats.roots_deduplicated += 1;
            return None;
        }
        self.done_roots.push(root.clone());
        let ig = self.ig_root.clone();
        match self.list_dir(tree, &root, cancel) {
            Ok(children) => {
                self.push_frame(tree, root, 0, &ig, children, cancel)
            }
            Err(ListError::NotADirectory) => {
                Some(WalkEvent::File { path: root, explicit: true })
            }
            Err(ListError::Cancelled) => Some(self.cancelled()),
            Err(ListError::Walk(err)) => Some(WalkEvent::Error(err)),
        }
    }

    /// List and enter the directory `path` at `depth` below its root.
    fn enter_dir<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        path: Vec<u8>,
        depth: usize,
        parent: &Ignore,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Option<WalkEvent> {
        match self.list_dir(tree, &path, cancel) {
            Ok(children) => {
                self.push_frame(tree, path, depth, parent, children, cancel)
            }
            Err(ListError::NotADirectory) => {
                let err = io::Error::from(io::ErrorKind::NotADirectory);
                Some(WalkEvent::Error(WalkError::new(
                    &path,
                    WalkErrorKind::Io(err),
                )))
            }
            Err(ListError::Cancelled) => Some(self.cancelled()),
            Err(ListError::Walk(err)) => Some(WalkEvent::Error(err)),
        }
    }

    /// Read the directory's ignore files, build its matcher and push it.
    fn push_frame<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        dir: Vec<u8>,
        depth: usize,
        parent: &Ignore,
        children: Packed,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Option<WalkEvent> {
        let names = parent.ignore_file_names();
        let mut contents: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut ignore_bytes = 0usize;
        let mut ignore_rules = 0usize;
        for (kind, name) in packed_children(&children.bytes) {
            if kind != EntryKind::File || !names.contains(&name) {
                continue;
            }
            let path = ignore_join(&dir, name);
            match self.read_ignore_file(tree, &path, cancel) {
                Ok((bytes, rules)) => {
                    ignore_bytes += bytes.len();
                    ignore_rules += rules;
                    if self.ignore_bytes + ignore_bytes
                        > self.limits.max_ignore_bytes_total
                        || self.ignore_rules + ignore_rules
                            > self.limits.max_ignore_rules_total
                    {
                        let kind = WalkErrorKind::IgnoreBytesLimit;
                        return Some(WalkEvent::Error(WalkError::new(
                            &path, kind,
                        )));
                    }
                    contents.push((name.to_vec(), bytes));
                }
                Err(ListError::Cancelled) => return Some(self.cancelled()),
                Err(ListError::NotADirectory) => unreachable!(),
                Err(ListError::Walk(err)) => {
                    return Some(WalkEvent::Error(err));
                }
            }
        }
        let files: Vec<(&[u8], &[u8])> = contents
            .iter()
            .map(|(n, c)| (n.as_slice(), c.as_slice()))
            .collect();
        let (ig, err) = parent.add_child(&dir, &files);
        self.pending_bytes += children.bytes.len();
        self.pending_entries += children.entries;
        self.ignore_bytes += ignore_bytes;
        self.ignore_rules += ignore_rules;
        let event = err.map(|err| {
            WalkEvent::Error(WalkError::new(&dir, WalkErrorKind::Ignore(err)))
        });
        self.stack.push(Frame {
            dir,
            depth,
            ig,
            pending_bytes: children.bytes.len(),
            pending_entries: children.entries,
            children: children.bytes,
            ignore_bytes,
            ignore_rules,
        });
        event
    }

    /// List `path` completely and pack its children in descending name
    /// order; the directory handle is closed before returning. One listing
    /// page of [`DirEntry`] values is held at a time.
    fn list_dir<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        path: &[u8],
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Packed, ListError> {
        let walk_err = |kind| ListError::Walk(WalkError::new(path, kind));
        let mut dir = match tree.open_dir(path) {
            Ok(dir) => dir,
            Err(err) if err.kind() == io::ErrorKind::NotADirectory => {
                return Err(ListError::NotADirectory);
            }
            Err(err) => return Err(walk_err(WalkErrorKind::Io(err))),
        };
        let mut page = Vec::new();
        let mut packed = Vec::new();
        // End offset (after the header) of each packed child.
        let mut ends: Vec<u32> = Vec::new();
        let mut listed = 0usize;
        let mut invalid = false;
        let result = loop {
            if cancel() {
                break Err(ListError::Cancelled);
            }
            page.clear();
            let info = match tree.next_page(&mut dir, &mut page) {
                Ok(info) => info,
                Err(err) => break Err(walk_err(WalkErrorKind::Io(err))),
            };
            if info.changed {
                break Err(walk_err(WalkErrorKind::ListingChanged));
            }
            listed += page.len();
            if listed > self.limits.max_dir_entries {
                break Err(walk_err(WalkErrorKind::DirEntryLimit));
            }
            for entry in page.drain(..) {
                if !is_component(&entry.name)
                    || entry.name.len() > MAX_PACKED_NAME
                {
                    invalid = true;
                    continue;
                }
                packed.extend_from_slice(&entry.name);
                packed.extend_from_slice(&pack_header(
                    entry.kind,
                    entry.name.len(),
                ));
                let Ok(end) = u32::try_from(packed.len()) else {
                    break;
                };
                ends.push(end);
            }
            if self.pending_bytes + packed.len()
                > self.limits.max_pending_bytes
                || self.pending_entries + ends.len()
                    > self.limits.max_pending_entries
                || u32::try_from(packed.len()).is_err()
            {
                break Err(walk_err(WalkErrorKind::PendingNamesLimit));
            }
            if info.complete {
                break Ok(());
            }
        };
        tree.close(Handle::Dir(dir));
        result?;
        drop(page);
        if invalid {
            return Err(walk_err(WalkErrorKind::InvalidName));
        }
        let name_of = |end: u32| {
            let end = end as usize;
            let (_, start) = last_packed(&packed[..end]).unwrap();
            &packed[start..end - 2]
        };
        ends.sort_unstable_by(|&a, &b| name_of(b).cmp(name_of(a)));
        let mut bytes = Vec::with_capacity(packed.len());
        for &end in &ends {
            let (_, start) = last_packed(&packed[..end as usize]).unwrap();
            bytes.extend_from_slice(&packed[start..end as usize]);
        }
        Ok(Packed { bytes, entries: ends.len() })
    }

    /// Read one ignore file completely, within the per-file limits; returns
    /// its contents and its rule count.
    fn read_ignore_file<T: ViSearchTree>(
        &mut self,
        tree: &mut T,
        path: &[u8],
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(Vec<u8>, usize), ListError> {
        let walk_err = |kind| ListError::Walk(WalkError::new(path, kind));
        let file = tree
            .open_file(path)
            .map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
        let mut file = OpenFile { tree, file: Some(file) };
        let max = self.limits.max_ignore_file_bytes;
        let mut contents = Vec::new();
        loop {
            if cancel() {
                return Err(ListError::Cancelled);
            }
            let start = contents.len();
            let want = core::cmp::min(
                self.limits.scratch_bytes.max(1),
                max + 1 - start,
            );
            contents.resize(start + want, 0);
            let n = file
                .read_chunk(start as u64, &mut contents[start..])
                .map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
            contents.truncate(start + n);
            self.stats.ignore_bytes_read += n as u64;
            if contents.len() > max {
                return Err(walk_err(WalkErrorKind::IgnoreFileTooLarge));
            }
            if n == 0 {
                break;
            }
        }
        let state =
            file.state().map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
        if state.modified {
            return Err(walk_err(WalkErrorKind::FileModified));
        }
        let rules = count_rules(&contents);
        if rules > self.limits.max_ignore_rules_per_file {
            return Err(walk_err(WalkErrorKind::IgnoreRulesLimit));
        }
        Ok((contents, rules))
    }
}

enum ListError {
    NotADirectory,
    Cancelled,
    Walk(WalkError),
}

/// An open file that is closed when dropped.
struct OpenFile<'t, T: ViSearchTree> {
    tree: &'t mut T,
    file: Option<T::File>,
}

impl<T: ViSearchTree> OpenFile<'_, T> {
    fn read_chunk(
        &mut self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error> {
        let file = self.file.as_mut().unwrap();
        self.tree.read_chunk(file, offset, buf)
    }

    fn state(&mut self) -> Result<FileState, io::Error> {
        let file = self.file.as_mut().unwrap();
        self.tree.file_state(file)
    }
}

impl<T: ViSearchTree> Drop for OpenFile<'_, T> {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            self.tree.close(Handle::File(file));
        }
    }
}

/// A directory's children, packed in descending name order (see
/// [`pack_header`]).
struct Packed {
    bytes: Vec<u8>,
    entries: usize,
}

/// The longest name a packed child can hold (14 bits); longer names are
/// [`WalkErrorKind::InvalidName`].
const MAX_PACKED_NAME: usize = 0x3FFF;

/// The 2-byte header stored after a packed child's name: the kind in the
/// top 2 bits and the name length in the low 14 bits, big-endian. Storing
/// it after the name lets children pop from the end of the buffer.
fn pack_header(kind: EntryKind, len: usize) -> [u8; 2] {
    debug_assert!(len <= MAX_PACKED_NAME);
    let code: u16 = match kind {
        EntryKind::File => 0,
        EntryKind::Dir => 1,
        EntryKind::Mount => 2,
        EntryKind::Other => 3,
    };
    ((code << 14) | len as u16).to_be_bytes()
}

/// The kind and the name's start offset of the last child packed in
/// `bytes`.
fn last_packed(bytes: &[u8]) -> Option<(EntryKind, usize)> {
    let &[.., hi, lo] = bytes else {
        return None;
    };
    let header = u16::from_be_bytes([hi, lo]);
    let kind = match header >> 14 {
        0 => EntryKind::File,
        1 => EntryKind::Dir,
        2 => EntryKind::Mount,
        _ => EntryKind::Other,
    };
    let len = usize::from(header & 0x3FFF);
    Some((kind, bytes.len() - 2 - len))
}

/// The children packed in `bytes`, last first (ascending name order for a
/// frame).
fn packed_children(
    mut bytes: &[u8],
) -> impl Iterator<Item = (EntryKind, &[u8])> {
    core::iter::from_fn(move || {
        let (kind, start) = last_packed(bytes)?;
        let name = &bytes[start..bytes.len() - 2];
        bytes = &bytes[..start];
        Some((kind, name))
    })
}

/// One file opened by [`Walker::begin_file`], searched in segments by
/// [`FileSearch::step`].
///
/// The file handle is held from `begin_file` until `step` returns anything
/// but [`Step::Paused`], or until [`FileSearch::abandon`]. Closing needs the
/// tree, so dropping a paused `FileSearch` without `abandon` leaks its
/// handle. Every step must use the same walker, tree, searcher
/// configuration and matcher; the resume rule is that of
/// [`ResumableSearch`] over the tree file (rewindable, with a window of
/// [`WalkLimits::resume_window_bytes`]).
pub struct FileSearch<F> {
    path: Vec<u8>,
    file: Option<F>,
    search: ResumableSearch,
}

impl<F> fmt::Debug for FileSearch<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileSearch")
            .field("path", &bstr::BStr::new(&self.path))
            .field("open", &self.file.is_some())
            .field("lines_scanned", &self.lines_scanned())
            .finish()
    }
}

impl<F> FileSearch<F> {
    /// Run one searcher segment, reporting rows to `sink`.
    ///
    /// A sink returning `Ok(false)` from `matched` or `context` means "row
    /// accepted, pause now": the step returns [`Step::Paused`] and keeps
    /// the handle; the next step resumes exactly after that row. The
    /// searcher's binary detection is set from the walker's
    /// [`BinaryMode`] on every step. On [`Step::Finished`] the file's state
    /// is checked (a modification, or a NUL the preflight did not see, is
    /// [`WalkErrorKind::FileModified`]), the handle is released and
    /// [`WalkStats::files_scanned`] counts the file. On error the handle is
    /// released and [`WalkStats::errors`] counts it, except
    /// [`SearchFileError::Cancelled`], which ends the walk. A step after the
    /// handle was released does nothing and returns [`Step::Finished`].
    pub fn step<T, M, S>(
        &mut self,
        walker: &mut Walker,
        tree: &mut T,
        searcher: &mut Searcher,
        matcher: M,
        sink: &mut S,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Step, SearchFileError<S::Error>>
    where
        T: ViSearchTree<File = F>,
        M: Matcher,
        S: Sink,
    {
        if self.file.is_none() {
            return Ok(Step::Finished);
        }
        let result =
            self.step_inner(walker, tree, searcher, matcher, sink, cancel);
        match result {
            Ok(Step::Paused) => return result,
            Ok(Step::Finished) => walker.stats.files_scanned += 1,
            Err(SearchFileError::Cancelled) => walker.finish_walk(),
            Err(_) => walker.stats.errors += 1,
        }
        self.release(tree);
        result
    }

    fn step_inner<T, M, S>(
        &mut self,
        walker: &mut Walker,
        tree: &mut T,
        searcher: &mut Searcher,
        matcher: M,
        sink: &mut S,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Step, SearchFileError<S::Error>>
    where
        T: ViSearchTree<File = F>,
        M: Matcher,
        S: Sink,
    {
        let file = self.file.as_mut().unwrap();
        searcher.set_binary_detection(match walker.binary {
            // The preflight saw no NUL. A NUL in the search pass means the
            // file changed between the passes.
            BinaryMode::Skip => BinaryDetection::quit(0),
            BinaryMode::Search => BinaryDetection::none(),
        });
        let mut guard = GuardSink { inner: sink, binary: false };
        let before = self.search.bytes_read();
        let result = self.search.step(
            &mut TreeInput { tree: &mut *tree, file: &mut *file },
            searcher,
            matcher,
            &mut guard,
            cancel,
        );
        walker.stats.bytes_read += self.search.bytes_read() - before;
        let path = &self.path;
        let walk_err =
            |kind| SearchFileError::Walk(WalkError::new(path, kind));
        match result {
            Ok(Step::Paused) => return Ok(Step::Paused),
            Ok(Step::Finished) => {}
            Err(ResumeError::Cancelled) => {
                return Err(SearchFileError::Cancelled);
            }
            Err(ResumeError::Io(err)) => {
                return Err(walk_err(WalkErrorKind::Io(err)));
            }
            Err(ResumeError::Sink(err)) => {
                return Err(SearchFileError::Sink(err));
            }
            // A tree file is rewindable, so this cannot happen.
            Err(ResumeError::WindowEvicted) => {
                let err = ResumeError::<Infallible>::WindowEvicted;
                let err = io::Error::other(err);
                return Err(walk_err(WalkErrorKind::Io(err)));
            }
        }
        if guard.binary {
            return Err(walk_err(WalkErrorKind::FileModified));
        }
        let state = tree
            .file_state(file)
            .map_err(|e| walk_err(WalkErrorKind::Io(e)))?;
        if state.modified {
            return Err(walk_err(WalkErrorKind::FileModified));
        }
        Ok(Step::Finished)
    }

    /// Stop searching and release the file handle.
    pub fn abandon<T: ViSearchTree<File = F>>(mut self, tree: &mut T) {
        self.release(tree);
    }

    /// The canonical path of the file.
    pub fn path(&self) -> &[u8] {
        &self.path
    }

    /// See [`ResumableSearch::lines_scanned`].
    pub fn lines_scanned(&self) -> u64 {
        self.search.lines_scanned()
    }

    fn release<T: ViSearchTree<File = F>>(&mut self, tree: &mut T) {
        if let Some(file) = self.file.take() {
            tree.close(Handle::File(file));
        }
    }
}

/// A tree file as a [`ResumeInput`].
struct TreeInput<'a, T: ViSearchTree> {
    tree: &'a mut T,
    file: &'a mut T::File,
}

impl<T: ViSearchTree> ResumeInput for TreeInput<'_, T> {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error> {
        self.tree.read_chunk(self.file, offset, buf)
    }

    fn rewindable(&self) -> bool {
        true
    }
}

/// An input a [`ResumableSearch`] reads by absolute offset.
pub trait ResumeInput {
    /// Read bytes starting at `offset` into a prefix of `buf` and return
    /// their count; `0` means end of input.
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error>;

    /// Whether any offset may be read again. When false (a stream), offsets
    /// below the furthest offset read are never requested: a resume that
    /// needs bytes older than the window fails with
    /// [`ResumeError::WindowEvicted`].
    fn rewindable(&self) -> bool;
}

/// Why [`ResumableSearch::step`] failed.
#[derive(Debug)]
pub enum ResumeError<E> {
    /// The input failed.
    Io(io::Error),
    /// The sink (or the searcher, e.g. its heap limit) returned an error.
    Sink(E),
    /// The cancel callback returned true.
    Cancelled,
    /// The input is not rewindable and the resume point is older than the
    /// resume window.
    WindowEvicted,
}

impl<E: fmt::Display> fmt::Display for ResumeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResumeError::Io(err) => err.fmt(f),
            ResumeError::Sink(err) => err.fmt(f),
            ResumeError::Cancelled => f.write_str("cancelled"),
            ResumeError::WindowEvicted => {
                f.write_str("resume point is older than the resume window")
            }
        }
    }
}

/// A line-oriented search of one input that a sink can pause after any row
/// and that resumes exactly where one uninterrupted search would continue
/// (D76-1, P1 segmented restart). Cellos-authored; the upstream searcher is
/// unchanged.
///
/// Each [`ResumableSearch::step`] runs a fresh `search_reader` from a line
/// start. A sink returning `Ok(false)` from `matched` or `context` has
/// accepted that row and pauses the search; that searcher run is
/// discarded. The state kept between steps is E, the last emitted row,
/// and M, the last emitted match (line, start and end offsets, kind).
///
/// **Resume rule** (refines proposal §2, which restarts at
/// `min(M, E + 1 − B)`). The next segment starts at line L0 = M when the
/// after-context A > 0 and E is M or one of M's after-context rows
/// (E ≤ M + A), and at L0 = E + 1 otherwise; L0 is a line start whose
/// offset is known from an emitted row (M's start or E's end). Going back
/// B lines is unnecessary: before-context lines ≤ E of a later match were
/// already emitted and are suppressed anyway, and lines > E are all
/// ≥ E + 1; the proposal's E + 1 − B would need offsets of lines that need
/// not have been emitted (E can be a before-context line). The rule uses
/// row kinds instead of line arithmetic, so it also works without line
/// numbers; for E = M + A it re-scans M..E (≤ A + 1 lines) where
/// `E < M + A` would start at E + 1 (both are exact).
///
/// Why it is exact: a line x > E is a match, before-context, after-context
/// or nothing depending only on matches in [x − A, x + B] and on the most
/// recent match before x. Matches > E are seen by the new run, and no match
/// lies in (M, E]. If x is after-context of M then E ≤ M + A, so L0 = M and
/// the new run sees M. Line-oriented matching (multi-line search is
/// refused) makes a restart at a line start equivalent to upstream's own
/// buffer roll.
///
/// The new run's line numbers are shifted by L0 − 1 and its offsets by the
/// offset of L0; rows at or before E are suppressed (accepted with
/// `Ok(true)` and not forwarded). The sink's `begin` is called once, on the
/// first step; `finish` only when the searcher reaches the end of the
/// input (shifted byte count and binary offset); `binary_data` at most
/// once. The searcher's own `context_break` calls are suppressed and
/// synthesized: with any context configured, a break precedes an emitted
/// row that does not directly follow the last emitted row. Returning
/// `Ok(false)` from `begin`, `context_break` or `binary_data` stops the
/// search ([`Step::Finished`]).
///
/// Rows the sink sees are shifted copies: [`SinkMatch::buffer`] still
/// starts where this segment's searcher buffer starts, so compute
/// submatches with [`line_submatches`], which does not depend on it.
///
/// Pausing a multi-line search, or one with `max_matches` or
/// `stop_on_nonmatch`, is a sink error (those cannot restart at a line).
/// Every step must use the same searcher configuration and matcher.
///
/// Reads: the most recent `window_bytes` bytes handed to the searcher are
/// kept (bytes [P − len, P), P = the furthest offset read). A restart
/// inside the window is served from memory; outside it, a rewindable
/// input is read again and a stream fails with
/// [`ResumeError::WindowEvicted`]. For a stream the window must cover the
/// searcher's read-ahead (up to its heap limit) plus A + 1 lines.
pub struct ResumableSearch {
    window: Window,
    state: ResumeState,
}

impl fmt::Debug for ResumableSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumableSearch")
            .field("furthest", &self.window.furthest)
            .field("window", &self.window.bytes.len())
            .field("bytes_read", &self.window.bytes_read)
            .field("finished", &self.state.finished)
            .finish()
    }
}

/// Recently searched bytes and read counters of a [`ResumableSearch`].
struct Window {
    /// Bytes [furthest − len, furthest).
    bytes: VecDeque<u8>,
    cap: usize,
    furthest: u64,
    /// Line terminators in [0, furthest).
    lines: u64,
    last_byte: Option<u8>,
    eof: bool,
    bytes_read: u64,
    line_term: u8,
}

impl Window {
    fn start(&self) -> u64 {
        self.furthest - self.bytes.len() as u64
    }

    /// Record `data`, read at `furthest`.
    fn push(&mut self, data: &[u8]) {
        self.furthest += data.len() as u64;
        self.lines += memchr_iter(self.line_term, data).count() as u64;
        if let Some(&last) = data.last() {
            self.last_byte = Some(last);
        }
        if self.cap == 0 {
            return;
        }
        if self.bytes.capacity() == 0 {
            self.bytes.reserve_exact(self.cap);
        }
        if data.len() >= self.cap {
            self.bytes.clear();
            self.bytes.extend(&data[data.len() - self.cap..]);
        } else {
            let over =
                (self.bytes.len() + data.len()).saturating_sub(self.cap);
            self.bytes.drain(..over);
            self.bytes.extend(data);
        }
    }

    /// Copy window bytes starting at index `from` into `out`.
    fn copy_out(&self, from: usize, out: &mut [u8]) {
        let (a, b) = self.bytes.as_slices();
        let n = out.len();
        if from < a.len() {
            let k = n.min(a.len() - from);
            out[..k].copy_from_slice(&a[from..from + k]);
            out[k..].copy_from_slice(&b[..n - k]);
        } else {
            let from = from - a.len();
            out.copy_from_slice(&b[from..from + n]);
        }
    }
}

/// What a [`ResumableSearch`] remembers between steps.
#[derive(Default)]
struct ResumeState {
    begun: bool,
    finished: bool,
    /// E: the last emitted row.
    last: Option<Row>,
    /// M: the last emitted match.
    last_match: Option<Row>,
    /// The sink's answer to the first `binary_data` call.
    binary: Option<bool>,
}

impl ResumeState {
    /// The offset and line number (when known) of L0.
    fn resume_point(&self, after: usize) -> (u64, Option<u64>) {
        match (self.last, self.last_match) {
            (None, _) => (0, Some(1)),
            (Some(e), Some(m)) if after > 0 && e.kind != RowKind::Other => {
                (m.start, m.line)
            }
            (Some(e), _) => (e.end, e.line.map(|n| n + 1)),
        }
    }
}

/// One emitted row, in input coordinates.
#[derive(Clone, Copy, Debug)]
struct Row {
    line: Option<u64>,
    start: u64,
    end: u64,
    kind: RowKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowKind {
    Match,
    After,
    /// Before-context or passthru context.
    Other,
}

impl ResumableSearch {
    /// A search that keeps a resume window of `window_bytes` bytes.
    pub fn new(window_bytes: usize) -> ResumableSearch {
        ResumableSearch {
            window: Window {
                bytes: VecDeque::new(),
                cap: window_bytes,
                furthest: 0,
                lines: 0,
                last_byte: None,
                eof: false,
                bytes_read: 0,
                line_term: b'\n',
            },
            state: ResumeState::default(),
        }
    }

    /// Run one searcher segment over `input`, reporting rows to `sink`;
    /// see the type documentation. The cancel callback is polled once per
    /// read the searcher makes. After [`Step::Finished`] a step does nothing
    /// and returns `Finished` again.
    pub fn step<I, M, S>(
        &mut self,
        input: &mut I,
        searcher: &mut Searcher,
        matcher: M,
        sink: &mut S,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Step, ResumeError<S::Error>>
    where
        I: ResumeInput,
        M: Matcher,
        S: Sink,
    {
        if self.state.finished {
            return Ok(Step::Finished);
        }
        let (offset, line) = self.state.resume_point(searcher.after_context());
        if offset < self.window.start() && !input.rewindable() {
            return Err(ResumeError::WindowEvicted);
        }
        let resumable = !searcher.multi_line_with_matcher(&matcher)
            && searcher.max_matches().is_none()
            && !searcher.stop_on_nonmatch();
        self.window.line_term = searcher.line_terminator().as_byte();
        let mut reader = ResumeReader {
            input,
            window: &mut self.window,
            pos: offset,
            cancel,
            cancelled: false,
            error: None,
        };
        let mut rows = ResumeSink {
            inner: sink,
            skip_upto: self.state.last.map(|e| e.start),
            state: &mut self.state,
            shift_offset: offset,
            shift_line: line.map_or(0, |n| n - 1),
            context: searcher.before_context() > 0
                || searcher.after_context() > 0,
            paused: false,
        };
        let result = searcher.search_reader(matcher, &mut reader, &mut rows);
        let paused = rows.paused;
        if reader.cancelled {
            return Err(ResumeError::Cancelled);
        }
        if let Some(err) = reader.error.take() {
            return Err(ResumeError::Io(err));
        }
        result.map_err(ResumeError::Sink)?;
        if !paused {
            self.state.finished = true;
            return Ok(Step::Finished);
        }
        if !resumable {
            self.state.finished = true;
            return Err(ResumeError::Sink(S::Error::error_message(
                "a multi-line, max-matches or stop-on-nonmatch search \
                 cannot pause",
            )));
        }
        Ok(Step::Paused)
    }

    /// Line terminators in the bytes read for the search so far (bytes
    /// re-read or re-served by a resumed step are not counted again), plus
    /// one for a final unterminated line once the search has finished at
    /// the end of the input.
    pub fn lines_scanned(&self) -> u64 {
        let w = &self.window;
        let tail = self.state.finished
            && w.eof
            && w.last_byte.is_some_and(|b| b != w.line_term);
        w.lines + u64::from(tail)
    }

    /// Bytes actually read from the input (window hits excluded).
    pub fn bytes_read(&self) -> u64 {
        self.window.bytes_read
    }
}

/// Feeds the searcher from the window and the input.
struct ResumeReader<'a, I> {
    input: &'a mut I,
    window: &'a mut Window,
    pos: u64,
    cancel: &'a mut dyn FnMut() -> bool,
    cancelled: bool,
    error: Option<io::Error>,
}

impl<I: ResumeInput> ResumeReader<'_, I> {
    fn read_input(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        match self.input.read_at(self.pos, buf) {
            Ok(n) => {
                self.window.bytes_read += n as u64;
                Ok(n)
            }
            Err(err) => {
                self.error = Some(err.clone());
                Err(err)
            }
        }
    }
}

impl<I: ResumeInput> io::Read for ResumeReader<'_, I> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        if (self.cancel)() {
            self.cancelled = true;
            return Err(io::Error::other("cancelled"));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let start = self.window.start();
        let furthest = self.window.furthest;
        let n = if self.pos >= furthest {
            let n = self.read_input(buf)?;
            if n == 0 {
                self.window.eof = true;
            }
            self.window.push(&buf[..n]);
            n
        } else if self.pos >= start {
            let from = (self.pos - start) as usize;
            let n = buf.len().min((furthest - self.pos) as usize);
            self.window.copy_out(from, &mut buf[..n]);
            n
        } else {
            // Re-read up to the window, which then serves the rest.
            let n = (start - self.pos).min(buf.len() as u64) as usize;
            self.read_input(&mut buf[..n])?
        };
        self.pos += n as u64;
        Ok(n)
    }
}

/// Shifts, suppresses and pauses rows for [`ResumableSearch`].
struct ResumeSink<'a, S> {
    inner: &'a mut S,
    state: &'a mut ResumeState,
    /// Rows starting at or before this offset (E's start) were emitted by
    /// an earlier step.
    skip_upto: Option<u64>,
    shift_offset: u64,
    shift_line: u64,
    context: bool,
    paused: bool,
}

impl<S: Sink> ResumeSink<'_, S> {
    fn suppressed(&self, start: u64) -> bool {
        self.skip_upto.is_some_and(|e| start <= e)
    }

    /// Forward the context break that precedes a row at `start`, if any.
    fn break_before(
        &mut self,
        searcher: &Searcher,
        start: u64,
    ) -> Result<bool, S::Error> {
        match self.state.last {
            Some(e) if self.context && start > e.end => {
                self.inner.context_break(searcher)
            }
            _ => Ok(true),
        }
    }

    /// Record an emitted row; the sink's `false` pauses.
    fn accepted(&mut self, row: Row, keepgoing: bool) -> bool {
        self.state.last = Some(row);
        if row.kind == RowKind::Match {
            self.state.last_match = Some(row);
        }
        if !keepgoing {
            self.paused = true;
        }
        keepgoing
    }
}

impl<S: Sink> Sink for ResumeSink<'_, S> {
    type Error = S::Error;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, S::Error> {
        let start = mat.absolute_byte_offset + self.shift_offset;
        if self.suppressed(start) {
            return Ok(true);
        }
        if !self.break_before(searcher, start)? {
            return Ok(false);
        }
        let line = mat.line_number.map(|n| n + self.shift_line);
        let shifted = SinkMatch {
            line_term: mat.line_term,
            bytes: mat.bytes,
            absolute_byte_offset: start,
            line_number: line,
            buffer: mat.buffer,
            bytes_range_in_buffer: mat.bytes_range_in_buffer.clone(),
        };
        let keepgoing = self.inner.matched(searcher, &shifted)?;
        let end = start + mat.bytes.len() as u64;
        let row = Row { line, start, end, kind: RowKind::Match };
        Ok(self.accepted(row, keepgoing))
    }

    fn context(
        &mut self,
        searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, S::Error> {
        let start = ctx.absolute_byte_offset + self.shift_offset;
        if self.suppressed(start) {
            return Ok(true);
        }
        if !self.break_before(searcher, start)? {
            return Ok(false);
        }
        let line = ctx.line_number.map(|n| n + self.shift_line);
        let shifted = SinkContext {
            #[cfg(test)]
            line_term: ctx.line_term,
            bytes: ctx.bytes,
            kind: ctx.kind.clone(),
            absolute_byte_offset: start,
            line_number: line,
        };
        let keepgoing = self.inner.context(searcher, &shifted)?;
        let kind = match ctx.kind {
            SinkContextKind::After => RowKind::After,
            _ => RowKind::Other,
        };
        let end = start + ctx.bytes.len() as u64;
        Ok(self.accepted(Row { line, start, end, kind }, keepgoing))
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, S::Error> {
        Ok(true)
    }

    fn binary_data(
        &mut self,
        searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, S::Error> {
        if let Some(keepgoing) = self.state.binary {
            return Ok(keepgoing);
        }
        let offset = binary_byte_offset + self.shift_offset;
        let keepgoing = self.inner.binary_data(searcher, offset)?;
        self.state.binary = Some(keepgoing);
        Ok(keepgoing)
    }

    fn begin(&mut self, searcher: &Searcher) -> Result<bool, S::Error> {
        if self.state.begun {
            return Ok(true);
        }
        self.state.begun = true;
        self.inner.begin(searcher)
    }

    fn finish(
        &mut self,
        searcher: &Searcher,
        sink_finish: &SinkFinish,
    ) -> Result<(), S::Error> {
        if self.paused {
            return Ok(());
        }
        let shifted = SinkFinish {
            byte_count: sink_finish.byte_count + self.shift_offset,
            binary_byte_offset: sink_finish
                .binary_byte_offset
                .map(|o| o + self.shift_offset),
        };
        self.inner.finish(searcher, &shifted)
    }
}

/// Why [`line_submatches`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmatchError {
    /// The line has more than the allowed number of submatches.
    Limit,
    /// The matcher failed.
    Matcher(String),
}

impl fmt::Display for SubmatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubmatchError::Limit => {
                f.write_str("too many matches in one line")
            }
            SubmatchError::Matcher(msg) => f.write_str(msg),
        }
    }
}

impl core::error::Error for SubmatchError {}

/// The submatches of a single-line match as `(start, end)` offsets into
/// [`SinkMatch::bytes`], following rg's printer (`record_matches` /
/// `find_iter_at_in_context`): the matcher runs over the input up to the
/// end of the line (its terminator removed), starting at the line, so
/// look-behind sees the preceding bytes; a match starting at or after the
/// line end ends the iteration; and a final empty match at the end of the
/// searched bytes is dropped. More than `max` submatches is
/// [`SubmatchError::Limit`].
///
/// Cellos adaptation: "the input" is taken from the start of the input,
/// not from the start of the searcher's buffer, so the result does not
/// depend on where a buffer (or a resumed [`ResumableSearch`] segment)
/// begins. A line at the start of a buffer that does not begin the input
/// is searched behind its preceding line terminator (one line copy), so
/// `\A` matches only at the start of the input; and the final empty match
/// is dropped only when it lies at the end of the input searched so far
/// measured from offset 0. Both equal upstream whenever the buffer starts
/// at the start of the input (inputs within one buffer).
pub fn line_submatches<M: Matcher>(
    matcher: M,
    mat: &SinkMatch<'_>,
    max: usize,
) -> Result<Vec<(usize, usize)>, SubmatchError> {
    let buffer = mat.buffer;
    let range = mat.bytes_range_in_buffer.clone();
    let mut end = range.end;
    if mat.line_term.is_suffix(&buffer[range.clone()]) {
        end -= 1;
    }
    let buffer_start = mat.absolute_byte_offset - range.start as u64;
    let prefixed;
    let (haystack, at) = if range.start == 0 && buffer_start > 0 {
        let mut bytes = Vec::with_capacity(end + 1);
        bytes.push(mat.line_term.as_byte());
        bytes.extend_from_slice(&buffer[..end]);
        prefixed = bytes;
        (&prefixed[..], 1)
    } else {
        (&buffer[..end], range.start)
    };
    let line_end = at + (range.end - range.start);
    let input_end = buffer_start + buffer.len() as u64;
    let final_empty = |(s, e): (usize, usize)| s == e && s as u64 >= input_end;
    let mut subs = Vec::new();
    let mut extra = None;
    matcher
        .find_iter_at(haystack, at, |m: Match| {
            if m.start() >= line_end {
                return false;
            }
            let sub = (m.start() - at, m.end() - at);
            if subs.len() == max {
                extra = Some(sub);
                return false;
            }
            subs.push(sub);
            true
        })
        .map_err(|err| SubmatchError::Matcher(err.to_string()))?;
    match extra {
        // A final empty match is the last match and is dropped, so only it
        // may follow `max` submatches.
        Some(sub) if final_empty(sub) => return Ok(subs),
        Some(_) => return Err(SubmatchError::Limit),
        None => {}
    }
    if subs.last().is_some_and(|&sub| final_empty(sub)) {
        subs.pop();
    }
    Ok(subs)
}

/// Forwards to the caller's sink, recording (and not forwarding) binary
/// data reports.
struct GuardSink<S> {
    inner: S,
    binary: bool,
}

impl<S: Sink> Sink for GuardSink<S> {
    type Error = S::Error;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, S::Error> {
        self.inner.matched(searcher, mat)
    }

    fn context(
        &mut self,
        searcher: &Searcher,
        context: &SinkContext<'_>,
    ) -> Result<bool, S::Error> {
        self.inner.context(searcher, context)
    }

    fn context_break(
        &mut self,
        searcher: &Searcher,
    ) -> Result<bool, S::Error> {
        self.inner.context_break(searcher)
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, S::Error> {
        self.binary = true;
        Ok(false)
    }

    fn begin(&mut self, searcher: &Searcher) -> Result<bool, S::Error> {
        self.inner.begin(searcher)
    }

    fn finish(
        &mut self,
        searcher: &Searcher,
        sink_finish: &SinkFinish,
    ) -> Result<(), S::Error> {
        if sink_finish.binary_byte_offset().is_some() {
            self.binary = true;
        }
        self.inner.finish(searcher, sink_finish)
    }
}

/// `dir` joined with one child name.
fn ignore_join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    if out.last() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

/// `path` is `root` or a descendant of it.
fn is_same_or_below(path: &[u8], root: &[u8]) -> bool {
    match path.strip_prefix(root) {
        None => false,
        Some(rest) => {
            rest.is_empty() || rest[0] == b'/' || root.last() == Some(&b'/')
        }
    }
}

/// `name` is a single, non-special path component.
fn is_component(name: &[u8]) -> bool {
    !name.is_empty()
        && name != b"."
        && name != b".."
        && memchr(b'/', name).is_none()
}

/// The number of lines of an ignore file that become rules, following
/// `GitignoreBuilder::add_line`'s comment and blank-line handling.
fn count_rules(contents: &[u8]) -> usize {
    contents
        .split(|&b| b == b'\n')
        .filter(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.starts_with(b"#") {
                return false;
            }
            if line.ends_with(b"\\ ") {
                return true;
            }
            match core::str::from_utf8(line) {
                Ok(line) => !line.trim_end().is_empty(),
                Err(_) => true,
            }
        })
        .count()
}
