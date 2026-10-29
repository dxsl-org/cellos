// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/ignore/src/dir.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

// This module provides a data structure, `Ignore`, that connects "directory
// traversal" with "ignore matchers." Specifically, it knows about gitignore
// semantics and precedence, and is organized based on directory hierarchy.
// Namely, every matcher logically corresponds to ignore rules from a single
// directory, and points to the matcher for its corresponding parent directory.
// In this sense, `Ignore` is a *persistent* data structure.
//
// This design was specifically chosen to make it possible to use this data
// structure in a parallel directory iterator.
//
// My initial intention was to expose this module as part of this crate's
// public API, but I think the data structure's public API is too complicated
// with non-obvious failure modes. Alas, such things haven't been documented
// well.
//
// vigrep: only the precedence logic is kept, re-expressed for the serial
// walker in `crate::walk`:
//
// * The caller lists a directory and passes the bytes of the ignore files it
//   found there to `Ignore::add_child` (upstream's `add_child_with_entries`
//   probed and opened them itself). Nothing here touches a filesystem.
// * Rules are discovered from the walk root downward only. Upstream's
//   `add_parents` (ancestor-above-root discovery and its cross-root cache),
//   global gitignore, `.git/info/exclude`, explicit `--ignore-file` matchers,
//   file types and repository (`.git`/`.jj`) detection are not ported, so
//   `.gitignore` rules apply as with upstream's `require_git(false)` and are
//   not stopped by a nested repository.
// * Every error from loading an ignore file is reported. Upstream drops I/O
//   errors here (`maybe_push_ignore_io`), which made a non-UTF-8 line end
//   rule loading silently.
// * The precedence is upstream's: overrides (command globs) first; then, per
//   category, the deepest directory's match wins, and the categories combine
//   as custom ignore files (`.rgignore`) > `.ignore` > `.gitignore`; hidden
//   entries are ignored only when nothing else matched.

#[allow(unused_imports)]
use crate::prelude::*;

use alloc::sync::Arc;

use crate::ignore::{
    gitignore::{self, Gitignore, GitignoreBuilder},
    overrides::{self, Override},
    pathutil::{is_hidden, join, strip_prefix},
    {Error, Match, PartialErrorBuilder},
};

/// IgnoreMatch represents information about where a match came from when using
/// the `Ignore` matcher.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct IgnoreMatch<'a>(IgnoreMatchInner<'a>);

/// IgnoreMatchInner describes precisely where the match information came from.
/// This is private to allow expansion to more matchers in the future.
#[derive(Clone, Debug)]
#[allow(dead_code)]
enum IgnoreMatchInner<'a> {
    Override(overrides::Glob<'a>),
    Gitignore(&'a gitignore::Glob),
    Hidden,
}

impl<'a> IgnoreMatch<'a> {
    fn overrides(x: overrides::Glob<'a>) -> IgnoreMatch<'a> {
        IgnoreMatch(IgnoreMatchInner::Override(x))
    }

    fn gitignore(x: &'a gitignore::Glob) -> IgnoreMatch<'a> {
        IgnoreMatch(IgnoreMatchInner::Gitignore(x))
    }

    fn hidden() -> IgnoreMatch<'static> {
        IgnoreMatch(IgnoreMatchInner::Hidden)
    }
}

/// Options for the ignore matcher, shared between the matcher itself and the
/// builder.
#[derive(Clone, Copy, Debug)]
struct IgnoreOptions {
    /// Whether to ignore hidden file paths or not.
    hidden: bool,
    /// Whether to read .ignore files.
    ignore: bool,
    /// Whether to read .gitignore files.
    git_ignore: bool,
    /// Whether to ignore files case insensitively
    ignore_case_insensitive: bool,
}

/// Ignore is a matcher useful for recursively walking one or more directories.
#[derive(Clone, Debug)]
pub(crate) struct Ignore {
    inner: Arc<IgnoreInner>,
}

#[derive(Clone, Debug)]
struct IgnoreInner {
    /// An override matcher (default is empty).
    overrides: Arc<Override>,
    /// The parent directory to match next.
    ///
    /// If this is the root directory or there are otherwise no more
    /// directories to match, then `parent` is `None`.
    parent: Option<Arc<IgnoreInner>>,
    /// Ignore files used in addition to `.ignore`
    custom_ignore_filenames: Arc<Vec<Vec<u8>>>,
    /// The matcher for custom ignore files
    custom_ignore_matcher: Gitignore,
    /// The matcher for .ignore files.
    ignore_matcher: Gitignore,
    /// The matcher for .gitignore files.
    git_ignore_matcher: Gitignore,
    /// Ignore config.
    opts: IgnoreOptions,
}

impl Ignore {
    /// The names of the ignore files this matcher reads in each directory, in
    /// no particular order. The walker reads the ones present in a listing
    /// and hands their contents to [`Ignore::add_child`].
    pub(crate) fn ignore_file_names(&self) -> Vec<&[u8]> {
        let mut names: Vec<&[u8]> = self
            .inner
            .custom_ignore_filenames
            .iter()
            .map(|n| n.as_slice())
            .collect();
        if self.inner.opts.ignore {
            names.push(b".ignore");
        }
        if self.inner.opts.git_ignore {
            names.push(b".gitignore");
        }
        names
    }

    /// Create a new `Ignore` matcher for the given child directory.
    ///
    /// `files` holds the `(file name, contents)` of the ignore files found
    /// in `dir`; entries whose name is not one of
    /// [`Ignore::ignore_file_names`] are not used.
    ///
    /// Since building the matcher may require reading from multiple
    /// files, it's possible that this method partially succeeds. Therefore,
    /// a matcher is always returned (which may match nothing) and an error is
    /// returned if it exists.
    pub(crate) fn add_child<P: AsRef<[u8]>>(
        &self,
        dir: P,
        files: &[(&[u8], &[u8])],
    ) -> (Ignore, Option<Error>) {
        let (ig, err) =
            self.add_child_path_with_found_ignore_files(dir.as_ref(), files);
        (Ignore { inner: Arc::new(ig) }, err)
    }

    fn add_child_path_with_found_ignore_files(
        &self,
        dir: &[u8],
        files: &[(&[u8], &[u8])],
    ) -> (IgnoreInner, Option<Error>) {
        let mut errs = PartialErrorBuilder::default();
        let custom_ig_matcher =
            if self.inner.custom_ignore_filenames.is_empty() {
                Gitignore::empty()
            } else {
                let custom_ignore_names: Vec<&[u8]> = self
                    .inner
                    .custom_ignore_filenames
                    .iter()
                    .map(|n| n.as_slice())
                    .collect();
                let (m, err) = create_gitignore(
                    dir,
                    files,
                    &custom_ignore_names,
                    self.inner.opts.ignore_case_insensitive,
                );
                errs.maybe_push(err);
                m
            };
        let ig_matcher = if !self.inner.opts.ignore {
            Gitignore::empty()
        } else {
            let (m, err) = create_gitignore(
                dir,
                files,
                &[b".ignore"],
                self.inner.opts.ignore_case_insensitive,
            );
            errs.maybe_push(err);
            m
        };
        let gi_matcher = if !self.inner.opts.git_ignore {
            Gitignore::empty()
        } else {
            let (m, err) = create_gitignore(
                dir,
                files,
                &[b".gitignore"],
                self.inner.opts.ignore_case_insensitive,
            );
            errs.maybe_push(err);
            m
        };
        let ig = IgnoreInner {
            overrides: self.inner.overrides.clone(),
            parent: Some(self.inner.clone()),
            custom_ignore_filenames: self
                .inner
                .custom_ignore_filenames
                .clone(),
            custom_ignore_matcher: custom_ig_matcher,
            ignore_matcher: ig_matcher,
            git_ignore_matcher: gi_matcher,
            opts: self.inner.opts,
        };
        (ig, errs.into_error_option())
    }

    /// Returns true if at least one type of ignore rule should be matched.
    fn has_any_ignore_rules(&self) -> bool {
        let opts = self.inner.opts;
        let has_custom_ignore_files =
            !self.inner.custom_ignore_filenames.is_empty();

        opts.ignore || opts.git_ignore || has_custom_ignore_files
    }

    /// Like `matched`, but for a walk entry: hidden entries are ignored when
    /// nothing else matched.
    ///
    /// vigrep: takes the entry's path and kind instead of a walk `DirEntry`.
    pub(crate) fn matched_dir_entry<'a>(
        &'a self,
        path: &[u8],
        is_dir: bool,
    ) -> Match<IgnoreMatch<'a>> {
        let m = self.matched(path, is_dir);
        if m.is_none() && self.inner.opts.hidden && is_hidden(path) {
            return Match::Ignore(IgnoreMatch::hidden());
        }
        m
    }

    /// Returns a match indicating whether the given file path should be
    /// ignored or not.
    ///
    /// The match contains information about its origin.
    fn matched<'a, P: AsRef<[u8]>>(
        &'a self,
        path: P,
        is_dir: bool,
    ) -> Match<IgnoreMatch<'a>> {
        // We need to be careful with our path. If it has a leading ./, then
        // strip it because it causes nothing but trouble.
        let mut path = path.as_ref();
        if let Some(p) = strip_prefix("./", path) {
            path = p;
        }
        // Match against the override patterns. If an override matches
        // regardless of whether it's whitelist/ignore, then we quit and
        // return that result immediately. Overrides have the highest
        // precedence.
        if !self.inner.overrides.is_empty() {
            let mat = self
                .inner
                .overrides
                .matched(path, is_dir)
                .map(IgnoreMatch::overrides);
            if !mat.is_none() {
                return mat;
            }
        }
        let mut whitelisted = Match::None;
        if self.has_any_ignore_rules() {
            let mat = self.matched_ignore(path, is_dir);
            if mat.is_ignore() {
                return mat;
            } else if mat.is_whitelist() {
                whitelisted = mat;
            }
        }
        whitelisted
    }

    /// Performs matching only on the ignore files for this directory and
    /// all parent directories.
    fn matched_ignore<'a>(
        &'a self,
        path: &[u8],
        is_dir: bool,
    ) -> Match<IgnoreMatch<'a>> {
        let (mut m_custom_ignore, mut m_ignore, mut m_gi) =
            (Match::None, Match::None, Match::None);
        for ig in self.parents() {
            if m_custom_ignore.is_none() {
                m_custom_ignore = ig
                    .inner
                    .custom_ignore_matcher
                    .matched(path, is_dir)
                    .map(IgnoreMatch::gitignore);
            }
            if m_ignore.is_none() {
                m_ignore = ig
                    .inner
                    .ignore_matcher
                    .matched(path, is_dir)
                    .map(IgnoreMatch::gitignore);
            }
            if m_gi.is_none() {
                m_gi = ig
                    .inner
                    .git_ignore_matcher
                    .matched(path, is_dir)
                    .map(IgnoreMatch::gitignore);
            }
        }

        m_custom_ignore.or(m_ignore).or(m_gi)
    }

    /// Returns an iterator over parent ignore matchers, including this one.
    pub(crate) fn parents(&self) -> Parents<'_> {
        Parents(Some(IgnoreRef { inner: &self.inner }))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct IgnoreRef<'a> {
    inner: &'a IgnoreInner,
}

/// An iterator over all parents of an ignore matcher, including itself.
pub(crate) struct Parents<'a>(Option<IgnoreRef<'a>>);

impl<'a> Iterator for Parents<'a> {
    type Item = IgnoreRef<'a>;

    fn next(&mut self) -> Option<IgnoreRef<'a>> {
        match self.0.take() {
            None => None,
            Some(ig) => {
                self.0 = ig
                    .inner
                    .parent
                    .as_deref()
                    .map(|inner| IgnoreRef { inner });
                Some(ig)
            }
        }
    }
}

/// A builder for creating an Ignore matcher.
#[derive(Clone, Debug)]
pub(crate) struct IgnoreBuilder {
    /// An override matcher (default is empty).
    overrides: Arc<Override>,
    /// Ignore files in addition to .ignore.
    custom_ignore_filenames: Vec<Vec<u8>>,
    /// Ignore config.
    opts: IgnoreOptions,
}

impl IgnoreBuilder {
    /// Create a new builder for an `Ignore` matcher.
    pub(crate) fn new() -> IgnoreBuilder {
        IgnoreBuilder {
            overrides: Arc::new(Override::empty()),
            custom_ignore_filenames: vec![],
            opts: IgnoreOptions {
                hidden: true,
                ignore: true,
                git_ignore: true,
                ignore_case_insensitive: false,
            },
        }
    }

    /// Builds a new `Ignore` matcher.
    ///
    /// The matcher returned won't match anything until ignore rules from
    /// directories are added to it.
    pub(crate) fn build(&self) -> Ignore {
        Ignore {
            inner: Arc::new(IgnoreInner {
                overrides: self.overrides.clone(),
                parent: None,
                custom_ignore_filenames: Arc::new(
                    self.custom_ignore_filenames.clone(),
                ),
                custom_ignore_matcher: Gitignore::empty(),
                ignore_matcher: Gitignore::empty(),
                git_ignore_matcher: Gitignore::empty(),
                opts: self.opts,
            }),
        }
    }

    /// Add an override matcher.
    ///
    /// By default, no override matcher is used.
    ///
    /// This overrides any previous setting.
    pub(crate) fn overrides(
        &mut self,
        overrides: Override,
    ) -> &mut IgnoreBuilder {
        self.overrides = Arc::new(overrides);
        self
    }

    /// Add a custom ignore file name
    ///
    /// These ignore files have higher precedence than all other ignore files.
    ///
    /// When specifying multiple names, earlier names have lower precedence than
    /// later names.
    pub(crate) fn add_custom_ignore_filename<S: AsRef<[u8]>>(
        &mut self,
        file_name: S,
    ) -> &mut IgnoreBuilder {
        self.custom_ignore_filenames.push(file_name.as_ref().to_vec());
        self
    }

    /// Enables ignoring hidden files.
    ///
    /// This is enabled by default.
    pub(crate) fn hidden(&mut self, yes: bool) -> &mut IgnoreBuilder {
        self.opts.hidden = yes;
        self
    }

    /// Enables reading `.ignore` files.
    ///
    /// `.ignore` files have the same semantics as `gitignore` files and are
    /// supported by search tools such as ripgrep and The Silver Searcher.
    ///
    /// This is enabled by default.
    pub(crate) fn ignore(&mut self, yes: bool) -> &mut IgnoreBuilder {
        self.opts.ignore = yes;
        self
    }

    /// Enables reading `.gitignore` files.
    ///
    /// `.gitignore` files have match semantics as described in the `gitignore`
    /// man page.
    ///
    /// This is enabled by default.
    pub(crate) fn git_ignore(&mut self, yes: bool) -> &mut IgnoreBuilder {
        self.opts.git_ignore = yes;
        self
    }

    /// Process ignore files case insensitively
    ///
    /// This is disabled by default.
    #[allow(dead_code)]
    pub(crate) fn ignore_case_insensitive(
        &mut self,
        yes: bool,
    ) -> &mut IgnoreBuilder {
        self.opts.ignore_case_insensitive = yes;
        self
    }
}

/// Creates a new gitignore matcher for the directory given.
///
/// The matcher is meant to match files below `dir`.
/// Ignore globs are extracted from each of the named files found in `files`
/// in the order of `names` (earlier names have lower precedence than later
/// names).
///
/// vigrep: `files` holds the contents the caller read; every error is
/// reported (upstream drops I/O errors here).
pub(crate) fn create_gitignore(
    dir: &[u8],
    files: &[(&[u8], &[u8])],
    names: &[&[u8]],
    case_insensitive: bool,
) -> (Gitignore, Option<Error>) {
    let mut builder = GitignoreBuilder::new(dir);
    let mut errs = PartialErrorBuilder::default();
    builder.case_insensitive(case_insensitive).unwrap();
    for name in names {
        for &(file_name, contents) in files {
            if file_name == *name {
                errs.maybe_push(builder.add_bytes(join(dir, name), contents));
            }
        }
    }
    let gi = match builder.build() {
        Ok(gi) => gi,
        Err(err) => {
            errs.push(err);
            GitignoreBuilder::new(dir).build().unwrap()
        }
    };
    (gi, errs.into_error_option())
}

// vigrep: upstream's tests write ignore files into a temporary directory and
// let `add_child` read them; here the same contents are passed to
// `add_child` directly. Tests of dropped behaviour (explicit ignore files,
// `.git/info/exclude`, `.jj`/`.git` detection, ancestor-above-root parents
// and their cross-root cache, linked worktrees) are not ported; see
// PROVENANCE. Two tests assert the intentional difference that `.gitignore`
// applies without (and across) a repository.
#[cfg(test)]
mod tests {
    use crate::prelude::*;

    use crate::ignore::{Error, dir::IgnoreBuilder};

    const TD: &str = "/tmp/rust-ignore/0";

    fn files<'a>(list: &[(&'a str, &'a str)]) -> Vec<(&'a [u8], &'a [u8])> {
        list.iter().map(|&(n, c)| (n.as_bytes(), c.as_bytes())).collect()
    }

    fn partial(err: Error) -> Vec<Error> {
        match err {
            Error::Partial(errs) => errs,
            _ => panic!("expected partial error but got {:?}", err),
        }
    }

    #[test]
    fn gitignore() {
        let (ig, err) = IgnoreBuilder::new().build().add_child(
            TD,
            &files(&[(".git", ""), (".gitignore", "foo\n!bar")]),
        );
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_whitelist());
        assert!(ig.matched("baz", false).is_none());
    }

    // vigrep: upstream asserts that `.gitignore` is not applied without a
    // `.git` directory (`require_git` defaults to true). vigrep has no
    // repository detection: `.gitignore` always applies.
    #[test]
    fn gitignore_no_git() {
        let (ig, err) = IgnoreBuilder::new()
            .build()
            .add_child(TD, &files(&[(".gitignore", "foo\n!bar")]));
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_whitelist());
        assert!(ig.matched("baz", false).is_none());
    }

    #[test]
    fn gitignore_allowed_no_git() {
        let (ig, err) = IgnoreBuilder::new()
            .build()
            .add_child(TD, &files(&[(".gitignore", "foo\n!bar")]));
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_whitelist());
        assert!(ig.matched("baz", false).is_none());
    }

    #[test]
    fn ignore() {
        let (ig, err) = IgnoreBuilder::new()
            .build()
            .add_child(TD, &files(&[(".ignore", "foo\n!bar")]));
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_whitelist());
        assert!(ig.matched("baz", false).is_none());
    }

    #[test]
    fn custom_ignore() {
        let custom_ignore = ".customignore";
        let (ig, err) = IgnoreBuilder::new()
            .add_custom_ignore_filename(custom_ignore)
            .build()
            .add_child(TD, &files(&[(custom_ignore, "foo\n!bar")]));
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_whitelist());
        assert!(ig.matched("baz", false).is_none());
    }

    // Tests that a custom ignore file will override an .ignore.
    #[test]
    fn custom_ignore_over_ignore() {
        let custom_ignore = ".customignore";
        let (ig, err) = IgnoreBuilder::new()
            .add_custom_ignore_filename(custom_ignore)
            .build()
            .add_child(
                TD,
                &files(&[(".ignore", "foo"), (custom_ignore, "!foo")]),
            );
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_whitelist());
    }

    // Tests that earlier custom ignore files have lower precedence than later.
    #[test]
    fn custom_ignore_precedence() {
        let custom_ignore1 = ".customignore1";
        let custom_ignore2 = ".customignore2";
        let (ig, err) = IgnoreBuilder::new()
            .add_custom_ignore_filename(custom_ignore1)
            .add_custom_ignore_filename(custom_ignore2)
            .build()
            .add_child(
                TD,
                &files(&[(custom_ignore1, "foo"), (custom_ignore2, "!foo")]),
            );
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_whitelist());
    }

    // Tests that an .ignore will override a .gitignore.
    #[test]
    fn ignore_over_gitignore() {
        let (ig, err) = IgnoreBuilder::new().build().add_child(
            TD,
            &files(&[(".gitignore", "foo"), (".ignore", "!foo")]),
        );
        assert!(err.is_none());
        assert!(ig.matched("foo", false).is_whitelist());
    }

    #[test]
    fn errored() {
        let (_, err) = IgnoreBuilder::new()
            .build()
            .add_child(TD, &files(&[(".gitignore", "{foo")]));
        assert!(err.is_some());
    }

    #[test]
    fn errored_both() {
        let (_, err) = IgnoreBuilder::new().build().add_child(
            TD,
            &files(&[(".gitignore", "{foo"), (".ignore", "{bar")]),
        );
        assert_eq!(2, partial(err.expect("an error")).len());
    }

    #[test]
    fn errored_partial() {
        let (ig, err) = IgnoreBuilder::new().build().add_child(
            TD,
            &files(&[(".git", ""), (".gitignore", "{foo\nbar")]),
        );
        assert!(err.is_some());
        assert!(ig.matched("bar", false).is_ignore());
    }

    #[test]
    fn errored_partial_and_ignore() {
        let (ig, err) = IgnoreBuilder::new().build().add_child(
            TD,
            &files(&[(".gitignore", "{foo\nbar"), (".ignore", "!bar")]),
        );
        assert!(err.is_some());
        assert!(ig.matched("bar", false).is_whitelist());
    }

    #[test]
    fn not_present_empty() {
        let (_, err) = IgnoreBuilder::new().build().add_child(TD, &[]);
        assert!(err.is_none());
    }

    // This tests that .gitignore files beyond a .git barrier aren't
    // matched, but .ignore files are.
    //
    // vigrep: there is no repository detection, so the nested `foo/.git`
    // is not a barrier and the ancestor `.gitignore` still applies.
    #[test]
    fn stops_at_git_dir() {
        let ig0 = IgnoreBuilder::new().build();
        let (ig1, err) = ig0.add_child(
            TD,
            &files(&[(".git", ""), (".gitignore", "foo"), (".ignore", "bar")]),
        );
        assert!(err.is_none());
        let foo = [TD.as_bytes(), b"/foo"].concat();
        let (ig2, err) = ig1.add_child(&foo, &files(&[(".git", "")]));
        assert!(err.is_none());

        assert!(ig1.matched("foo", false).is_ignore());
        assert!(ig2.matched("foo", false).is_ignore());

        assert!(ig1.matched("bar", false).is_ignore());
        assert!(ig2.matched("bar", false).is_ignore());
    }

    // vigrep: a non-UTF-8 rule line is an error. Upstream reads the file
    // with `BufRead::lines`, gets an I/O error and drops it, so rule loading
    // stopped silently at that line.
    #[test]
    fn invalid_utf8_rule_is_reported() {
        let ig0 = IgnoreBuilder::new().build();
        let (ig, err) = ig0.add_child(
            TD,
            &[(b".gitignore".as_slice(), b"foo\n\xFF\nbar".as_slice())],
        );
        let err = err.expect("an error");
        assert!(err.is_io());
        assert!(ig.matched("foo", false).is_ignore());
        assert!(ig.matched("bar", false).is_none());
    }

    #[test]
    fn deeper_directory_overrides_ancestor() {
        let ig0 = IgnoreBuilder::new().build();
        let (ig1, _) = ig0.add_child(TD, &files(&[(".gitignore", "*.log")]));
        let sub = [TD.as_bytes(), b"/sub"].concat();
        let (ig2, _) =
            ig1.add_child(&sub, &files(&[(".gitignore", "!keep.log")]));
        let keep = [sub.as_slice(), b"/keep.log"].concat();
        let other = [sub.as_slice(), b"/other.log"].concat();
        assert!(ig2.matched(&keep, false).is_whitelist());
        assert!(ig2.matched(&other, false).is_ignore());
        assert_eq!(ig2.parents().count(), 3);
    }
}
