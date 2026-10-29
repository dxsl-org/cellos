// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/ignore/src/gitignore.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

/*!
The gitignore module provides a way to match globs from a gitignore file
against file paths.

Note that this module implements the specification as described in the
`gitignore` man page from scratch. That is, this module does *not* shell out to
the `git` command line tool.

vigrep: paths are bytes and rules come from bytes supplied by the caller
([`GitignoreBuilder::add_bytes`], [`GitignoreBuilder::add_line`]); nothing
here opens a file, reads git configuration or consults the environment, so
upstream's `Gitignore::new`, `Gitignore::global`, `build_global` and
`gitconfig_excludes_path` are not ported.
*/

#[allow(unused_imports)]
use crate::prelude::*;

use alloc::sync::Arc;

use {
    crate::glob::{Candidate, GlobBuilder, GlobSet, GlobSetBuilder},
    regex_automata::util::pool::Pool,
};

use crate::ignore::{
    Error, Match, PartialErrorBuilder,
    pathutil::{has_root, is_file_name, parent, same_path, strip_prefix},
};

/// Glob represents a single glob in a gitignore file.
///
/// This is used to report information about the highest precedent glob that
/// matched in one or more gitignore files.
#[derive(Clone, Debug)]
pub struct Glob {
    /// The file path that this glob was extracted from.
    from: Option<Vec<u8>>,
    /// The original glob string.
    original: String,
    /// The actual glob string used to convert to a regex.
    actual: String,
    /// Whether this is a whitelisted glob or not.
    is_whitelist: bool,
    /// Whether this glob should only match directories or not.
    is_only_dir: bool,
}

impl Glob {
    /// Returns the file path that defined this glob.
    pub fn from(&self) -> Option<&[u8]> {
        self.from.as_deref()
    }

    /// The original glob as it was defined in a gitignore file.
    pub fn original(&self) -> &str {
        &self.original
    }

    /// The actual glob that was compiled to respect gitignore
    /// semantics.
    pub fn actual(&self) -> &str {
        &self.actual
    }

    /// Whether this was a whitelisted glob or not.
    pub fn is_whitelist(&self) -> bool {
        self.is_whitelist
    }

    /// Whether this glob must match a directory or not.
    pub fn is_only_dir(&self) -> bool {
        self.is_only_dir
    }

    /// Returns true if and only if this glob has a `**/` prefix.
    fn has_doublestar_prefix(&self) -> bool {
        self.actual.starts_with("**/") || self.actual == "**"
    }
}

/// Gitignore is a matcher for the globs in one or more gitignore files
/// in the same directory.
#[derive(Clone, Debug)]
pub struct Gitignore {
    set: GlobSet,
    root: Vec<u8>,
    globs: Vec<Glob>,
    num_ignores: u64,
    num_whitelists: u64,
    matches: Option<Arc<Pool<Vec<usize>>>>,
}

impl Gitignore {
    /// Creates a new empty gitignore matcher that never matches anything.
    ///
    /// Its path is empty.
    pub fn empty() -> Gitignore {
        Gitignore {
            set: GlobSet::empty(),
            root: Vec::new(),
            globs: vec![],
            num_ignores: 0,
            num_whitelists: 0,
            matches: None,
        }
    }

    /// Returns the directory containing this gitignore matcher.
    ///
    /// All matches are done relative to this path.
    pub fn path(&self) -> &[u8] {
        &self.root
    }

    /// Returns true if and only if this gitignore has zero globs, and
    /// therefore never matches any file path.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Returns the total number of globs, which should be equivalent to
    /// `num_ignores + num_whitelists`.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Returns the total number of ignore globs.
    pub fn num_ignores(&self) -> u64 {
        self.num_ignores
    }

    /// Returns the total number of whitelisted globs.
    pub fn num_whitelists(&self) -> u64 {
        self.num_whitelists
    }

    /// Returns whether the given path (file or directory) matched a pattern in
    /// this gitignore matcher.
    ///
    /// `is_dir` should be true if the path refers to a directory and false
    /// otherwise.
    ///
    /// The given path is matched relative to the path given when building
    /// the matcher. Specifically, before matching `path`, its prefix (as
    /// determined by a common suffix of the directory containing this
    /// gitignore) is stripped. If there is no common suffix/prefix overlap,
    /// then `path` is assumed to be relative to this matcher.
    pub fn matched<P: AsRef<[u8]>>(
        &self,
        path: P,
        is_dir: bool,
    ) -> Match<&Glob> {
        if self.is_empty() {
            return Match::None;
        }
        self.matched_stripped(self.strip(path.as_ref()), is_dir)
    }

    /// Returns whether the given path (file or directory, and expected to be
    /// under the root) or any of its parent directories (up to the root)
    /// matched a pattern in this gitignore matcher.
    ///
    /// NOTE: This method is more expensive than walking the directory hierarchy
    /// top-to-bottom and matching the entries. But, is easier to use in cases
    /// when a list of paths are available without a hierarchy.
    ///
    /// `is_dir` should be true if the path refers to a directory and false
    /// otherwise.
    ///
    /// The given path is matched relative to the path given when building
    /// the matcher. Specifically, before matching `path`, its prefix (as
    /// determined by a common suffix of the directory containing this
    /// gitignore) is stripped. If there is no common suffix/prefix overlap,
    /// then `path` is assumed to be relative to this matcher.
    ///
    /// # Panics
    ///
    /// This method panics if the given file path is not under the root path
    /// of this matcher.
    pub fn matched_path_or_any_parents<P: AsRef<[u8]>>(
        &self,
        path: P,
        is_dir: bool,
    ) -> Match<&Glob> {
        if self.is_empty() {
            return Match::None;
        }
        let mut path = self.strip(path.as_ref());
        assert!(!has_root(path), "path is expected to be under the root");

        match self.matched_stripped(path, is_dir) {
            Match::None => (), // walk up
            a_match => return a_match,
        }
        while let Some(parent) = parent(path) {
            match self.matched_stripped(parent, /* is_dir */ true) {
                Match::None => path = parent, // walk up
                a_match => return a_match,
            }
        }
        Match::None
    }

    /// Like matched, but takes a path that has already been stripped.
    fn matched_stripped<P: AsRef<[u8]>>(
        &self,
        path: P,
        is_dir: bool,
    ) -> Match<&Glob> {
        if self.is_empty() {
            return Match::None;
        }
        let path = path.as_ref();
        let mut matches = self.matches.as_ref().unwrap().get();
        let candidate = Candidate::new(path);
        self.set.matches_candidate_into(&candidate, &mut *matches);
        for &i in matches.iter().rev() {
            let glob = &self.globs[i];
            if !glob.is_only_dir() || is_dir {
                return if glob.is_whitelist() {
                    Match::Whitelist(glob)
                } else {
                    Match::Ignore(glob)
                };
            }
        }
        Match::None
    }

    /// Strips the given path such that it's suitable for matching with this
    /// gitignore matcher.
    fn strip<'a, P: 'a + AsRef<[u8]> + ?Sized>(
        &'a self,
        path: &'a P,
    ) -> &'a [u8] {
        let mut path = path.as_ref();
        // A leading ./ is completely superfluous. We also strip it from
        // our gitignore root path, so we need to strip it from our candidate
        // path too.
        if let Some(p) = strip_prefix("./", path) {
            path = p;
        }
        // Strip any common prefix between the candidate path and the root
        // of the gitignore, to make sure we get relative matching right.
        // BUT, a file name might not have any directory components to it,
        // in which case, we don't want to accidentally strip any part of the
        // file name.
        //
        // As an additional special case, if the root is just `.`, then we
        // shouldn't try to strip anything, e.g., when path begins with a `.`.
        if !same_path(&self.root, b".") && !is_file_name(path) {
            if let Some(p) = strip_prefix(&self.root, path) {
                path = p;
                // If we're left with a leading slash, get rid of it.
                if let Some(p) = strip_prefix("/", path) {
                    path = p;
                }
            }
        }
        path
    }
}

/// Builds a matcher for a single set of globs from a .gitignore file.
#[derive(Clone, Debug)]
pub struct GitignoreBuilder {
    builder: GlobSetBuilder,
    root: Vec<u8>,
    globs: Vec<Glob>,
    case_insensitive: bool,
    allow_unclosed_class: bool,
}

impl GitignoreBuilder {
    /// Create a new builder for a gitignore file.
    ///
    /// The path given should be the path at which the globs for this gitignore
    /// file should be matched. Note that paths are always matched relative
    /// to the root path given here. Generally, the root path should correspond
    /// to the *directory* containing a `.gitignore` file.
    pub fn new<P: AsRef<[u8]>>(root: P) -> GitignoreBuilder {
        let root = root.as_ref();
        GitignoreBuilder {
            builder: GlobSetBuilder::new(),
            root: strip_prefix("./", root).unwrap_or(root).to_vec(),
            globs: vec![],
            case_insensitive: false,
            allow_unclosed_class: true,
        }
    }

    /// Builds a new matcher from the globs added so far.
    ///
    /// Once a matcher is built, no new globs can be added to it.
    pub fn build(&self) -> Result<Gitignore, Error> {
        let nignore = self.globs.iter().filter(|g| !g.is_whitelist()).count();
        let nwhite = self.globs.iter().filter(|g| g.is_whitelist()).count();
        let set = self
            .builder
            .build()
            .map_err(|err| Error::Glob { glob: None, err: err.to_string() })?;
        Ok(Gitignore {
            set,
            root: self.root.clone(),
            globs: self.globs.clone(),
            num_ignores: nignore as u64,
            num_whitelists: nwhite as u64,
            matches: Some(Arc::new(Pool::new(|| vec![]))),
        })
    }

    /// Add each glob from the contents of the gitignore file at the path
    /// given.
    ///
    /// vigrep: upstream's `add(path)` opens and reads the file; here the
    /// caller supplies the bytes it read (e.g. through a VFS handle) and
    /// `path` only labels globs and errors. Lines are split as
    /// `BufRead::lines` does (LF, with one CR before it removed); a line that
    /// is not valid UTF-8 ends processing with an I/O error, as upstream.
    ///
    /// The file given should be formatted as a `gitignore` file.
    ///
    /// Note that partial errors can be returned. For example, if there was
    /// a problem adding one glob, an error for that will be returned, but
    /// all other valid globs will still be added.
    pub fn add_bytes<P: AsRef<[u8]>>(
        &mut self,
        path: P,
        contents: &[u8],
    ) -> Option<Error> {
        let path = path.as_ref();
        log::debug!("adding gitignore file: {}", bstr::BStr::new(path));
        let mut errs = PartialErrorBuilder::default();
        for (i, line) in lines(contents).enumerate() {
            let lineno = (i + 1) as u64;
            let line = match core::str::from_utf8(line) {
                Ok(line) => line,
                Err(_) => {
                    let err = crate::io::Error::new(
                        crate::io::ErrorKind::InvalidData,
                        "stream did not contain valid UTF-8",
                    );
                    errs.push(Error::Io(err).tagged(path, lineno));
                    break;
                }
            };

            // Match Git's handling of .gitignore files that begin with the Unicode BOM
            const UTF8_BOM: &str = "\u{feff}";
            let line =
                if i == 0 { line.trim_start_matches(UTF8_BOM) } else { &line };

            if let Err(err) = self.add_line(Some(path.to_vec()), &line) {
                errs.push(err.tagged(path, lineno));
            }
        }
        errs.into_error_option()
    }

    /// Add each glob line from the string given.
    ///
    /// If this string came from a particular `gitignore` file, then its path
    /// should be provided here.
    ///
    /// The string given should be formatted as a `gitignore` file.
    #[cfg(test)]
    fn add_str(
        &mut self,
        from: Option<Vec<u8>>,
        gitignore: &str,
    ) -> Result<&mut GitignoreBuilder, Error> {
        for line in gitignore.lines() {
            self.add_line(from.clone(), line)?;
        }
        Ok(self)
    }

    /// Add a line from a gitignore file to this builder.
    ///
    /// If this line came from a particular `gitignore` file, then its path
    /// should be provided here.
    ///
    /// If the line could not be parsed as a glob, then an error is returned.
    pub fn add_line(
        &mut self,
        from: Option<Vec<u8>>,
        mut line: &str,
    ) -> Result<&mut GitignoreBuilder, Error> {
        #![allow(deprecated)]

        if line.starts_with("#") {
            return Ok(self);
        }
        if !line.ends_with("\\ ") {
            line = line.trim_right();
        }
        if line.is_empty() {
            return Ok(self);
        }
        let mut glob = Glob {
            from,
            original: line.to_string(),
            actual: String::new(),
            is_whitelist: false,
            is_only_dir: false,
        };
        let mut is_absolute = false;
        if line.starts_with("\\!") || line.starts_with("\\#") {
            line = &line[1..];
            is_absolute = line.chars().nth(0) == Some('/');
        } else {
            if line.starts_with("!") {
                glob.is_whitelist = true;
                line = &line[1..];
            }
            if line.starts_with("/") {
                // `man gitignore` says that if a glob starts with a slash,
                // then the glob can only match the beginning of a path
                // (relative to the location of gitignore). We achieve this by
                // simply banning wildcards from matching /.
                line = &line[1..];
                is_absolute = true;
            }
        }
        // If it ends with a slash, then this should only match directories,
        // but the slash should otherwise not be used while globbing.
        if line.as_bytes().last() == Some(&b'/') {
            glob.is_only_dir = true;
            line = &line[..line.len() - 1];
            // If the slash was escaped, then remove the escape.
            // See: https://github.com/BurntSushi/ripgrep/issues/2236
            if line.as_bytes().last() == Some(&b'\\') {
                line = &line[..line.len() - 1];
            }
        }
        glob.actual = line.to_string();
        // If there is a literal slash, then this is a glob that must match the
        // entire path name. Otherwise, we should let it match anywhere, so use
        // a **/ prefix.
        if !is_absolute && !line.chars().any(|c| c == '/') {
            // ... but only if we don't already have a **/ prefix.
            if !glob.has_doublestar_prefix() {
                glob.actual = format!("**/{}", glob.actual);
            }
        }
        // If the glob ends with `/**`, then we should only match everything
        // inside a directory, but not the directory itself. Standard globs
        // will match the directory. So we add `/*` to force the issue.
        if glob.actual.ends_with("/**") {
            glob.actual = format!("{}/*", glob.actual);
        }
        let parsed = GlobBuilder::new(&glob.actual)
            .literal_separator(true)
            .case_insensitive(self.case_insensitive)
            .backslash_escape(true)
            .allow_unclosed_class(self.allow_unclosed_class)
            .build()
            .map_err(|err| Error::Glob {
                glob: Some(glob.original.clone()),
                err: err.kind().to_string(),
            })?;
        self.builder.add(parsed);
        self.globs.push(glob);
        Ok(self)
    }

    /// Toggle whether the globs should be matched case insensitively or not.
    ///
    /// When this option is changed, only globs added after the change will be
    /// affected.
    ///
    /// This is disabled by default.
    pub fn case_insensitive(
        &mut self,
        yes: bool,
    ) -> Result<&mut GitignoreBuilder, Error> {
        // TODO: This should not return a `Result`. Fix this in the next semver
        // release.
        self.case_insensitive = yes;
        Ok(self)
    }

    /// Toggle whether unclosed character classes are allowed. When allowed,
    /// a `[` without a matching `]` is treated literally instead of resulting
    /// in a parse error.
    ///
    /// For example, if this is set then the glob `[abc` will be treated as the
    /// literal string `[abc` instead of returning an error.
    ///
    /// By default, this is true in order to match established `gitignore`
    /// semantics. Generally speaking, enabling this leads to worse failure
    /// modes since the glob parser becomes more permissive. You might want to
    /// enable this when compatibility (e.g., with POSIX glob implementations)
    /// is more important than good error messages.
    pub fn allow_unclosed_class(
        &mut self,
        yes: bool,
    ) -> &mut GitignoreBuilder {
        self.allow_unclosed_class = yes;
        self
    }
}

/// vigrep: the line splitting of `BufRead::lines`: LF-terminated lines with a
/// single CR before the LF removed; a final unterminated line is yielded and
/// an empty input yields nothing.
fn lines(mut contents: &[u8]) -> impl Iterator<Item = &[u8]> {
    core::iter::from_fn(move || {
        if contents.is_empty() {
            return None;
        }
        let (mut line, rest) = match memchr::memchr(b'\n', contents) {
            Some(i) => (&contents[..i], &contents[i + 1..]),
            None => (contents, &contents[contents.len()..]),
        };
        if contents.len() > line.len() && line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        contents = rest;
        Some(line)
    })
}

#[cfg(test)]
mod tests {
    use super::{Gitignore, GitignoreBuilder, lines};

    fn gi_from_str<P: AsRef<[u8]>>(root: P, s: &str) -> Gitignore {
        let mut builder = GitignoreBuilder::new(root);
        builder.add_str(None, s).unwrap();
        builder.build().unwrap()
    }

    macro_rules! ignored {
        ($name:ident, $root:expr, $gi:expr, $path:expr) => {
            ignored!($name, $root, $gi, $path, false);
        };
        ($name:ident, $root:expr, $gi:expr, $path:expr, $is_dir:expr) => {
            #[test]
            fn $name() {
                let gi = gi_from_str($root, $gi);
                assert!(gi.matched($path, $is_dir).is_ignore());
            }
        };
    }

    macro_rules! not_ignored {
        ($name:ident, $root:expr, $gi:expr, $path:expr) => {
            not_ignored!($name, $root, $gi, $path, false);
        };
        ($name:ident, $root:expr, $gi:expr, $path:expr, $is_dir:expr) => {
            #[test]
            fn $name() {
                let gi = gi_from_str($root, $gi);
                assert!(!gi.matched($path, $is_dir).is_ignore());
            }
        };
    }

    const ROOT: &'static str = "/home/foobar/rust/rg";

    ignored!(ig1, ROOT, "months", "months");
    ignored!(ig2, ROOT, "*.lock", "Cargo.lock");
    ignored!(ig3, ROOT, "*.rs", "src/main.rs");
    ignored!(ig4, ROOT, "src/*.rs", "src/main.rs");
    ignored!(ig5, ROOT, "/*.c", "cat-file.c");
    ignored!(ig6, ROOT, "/src/*.rs", "src/main.rs");
    ignored!(ig7, ROOT, "!src/main.rs\n*.rs", "src/main.rs");
    ignored!(ig8, ROOT, "foo/", "foo", true);
    ignored!(ig9, ROOT, "**/foo", "foo");
    ignored!(ig10, ROOT, "**/foo", "src/foo");
    ignored!(ig11, ROOT, "**/foo/**", "src/foo/bar");
    ignored!(ig12, ROOT, "**/foo/**", "wat/src/foo/bar/baz");
    ignored!(ig13, ROOT, "**/foo/bar", "foo/bar");
    ignored!(ig14, ROOT, "**/foo/bar", "src/foo/bar");
    ignored!(ig15, ROOT, "abc/**", "abc/x");
    ignored!(ig16, ROOT, "abc/**", "abc/x/y");
    ignored!(ig17, ROOT, "abc/**", "abc/x/y/z");
    ignored!(ig18, ROOT, "a/**/b", "a/b");
    ignored!(ig19, ROOT, "a/**/b", "a/x/b");
    ignored!(ig20, ROOT, "a/**/b", "a/x/y/b");
    ignored!(ig21, ROOT, r"\!xy", "!xy");
    ignored!(ig22, ROOT, r"\#foo", "#foo");
    ignored!(ig23, ROOT, "foo", "./foo");
    ignored!(ig24, ROOT, "target", "grep/target");
    ignored!(ig25, ROOT, "Cargo.lock", "./tabwriter-bin/Cargo.lock");
    ignored!(ig26, ROOT, "/foo/bar/baz", "./foo/bar/baz");
    ignored!(ig27, ROOT, "foo/", "xyz/foo", true);
    ignored!(ig28, "./src", "/llvm/", "./src/llvm", true);
    ignored!(ig29, ROOT, "node_modules/ ", "node_modules", true);
    ignored!(ig30, ROOT, "**/", "foo/bar", true);
    ignored!(ig31, ROOT, "path1/*", "path1/foo");
    ignored!(ig32, ROOT, ".a/b", ".a/b");
    ignored!(ig33, "./", ".a/b", ".a/b");
    ignored!(ig34, ".", ".a/b", ".a/b");
    ignored!(ig35, "./.", ".a/b", ".a/b");
    ignored!(ig36, "././", ".a/b", ".a/b");
    ignored!(ig37, "././.", ".a/b", ".a/b");
    ignored!(ig38, ROOT, "\\[", "[");
    ignored!(ig39, ROOT, "\\?", "?");
    ignored!(ig40, ROOT, "\\*", "*");
    ignored!(ig41, ROOT, "\\a", "a");
    ignored!(ig42, ROOT, "s*.rs", "sfoo.rs");
    ignored!(ig43, ROOT, "**", "foo.rs");
    ignored!(ig44, ROOT, "**/**/*", "a/foo.rs");

    not_ignored!(ignot1, ROOT, "amonths", "months");
    not_ignored!(ignot2, ROOT, "monthsa", "months");
    not_ignored!(ignot3, ROOT, "/src/*.rs", "src/grep/src/main.rs");
    not_ignored!(ignot4, ROOT, "/*.c", "mozilla-sha1/sha1.c");
    not_ignored!(ignot5, ROOT, "/src/*.rs", "src/grep/src/main.rs");
    not_ignored!(ignot6, ROOT, "*.rs\n!src/main.rs", "src/main.rs");
    not_ignored!(ignot7, ROOT, "foo/", "foo", false);
    not_ignored!(ignot8, ROOT, "**/foo/**", "wat/src/afoo/bar/baz");
    not_ignored!(ignot9, ROOT, "**/foo/**", "wat/src/fooa/bar/baz");
    not_ignored!(ignot10, ROOT, "**/foo/bar", "foo/src/bar");
    not_ignored!(ignot11, ROOT, "#foo", "#foo");
    not_ignored!(ignot12, ROOT, "\n\n\n", "foo");
    not_ignored!(ignot13, ROOT, "foo/**", "foo", true);
    not_ignored!(
        ignot14,
        "./third_party/protobuf",
        "m4/ltoptions.m4",
        "./third_party/protobuf/csharp/src/packages/repositories.config"
    );
    not_ignored!(ignot15, ROOT, "!/bar", "foo/bar");
    not_ignored!(ignot16, ROOT, "*\n!**/", "foo", true);
    not_ignored!(ignot17, ROOT, "src/*.rs", "src/grep/src/main.rs");
    not_ignored!(ignot18, ROOT, "path1/*", "path2/path1/foo");
    not_ignored!(ignot19, ROOT, "s*.rs", "src/foo.rs");

    /// vigrep: `BufRead::lines` splitting, as upstream's `add` relied on.
    #[test]
    fn lines_match_bufread() {
        use std::io::BufRead;

        for input in [
            "",
            "a",
            "a\n",
            "a\r\n",
            "a\r",
            "\n\n",
            "a\rb\n",
            "\r\n\r\n",
            "x\ny\r\nz",
        ] {
            let std: Vec<String> =
                input.as_bytes().lines().map(|l| l.unwrap()).collect();
            let ours: Vec<&[u8]> = lines(input.as_bytes()).collect();
            let ours: Vec<&str> = ours
                .iter()
                .map(|l| core::str::from_utf8(l).unwrap())
                .collect();
            assert_eq!(std, ours, "{input:?}");
        }
    }

    // See: https://github.com/BurntSushi/ripgrep/issues/106
    #[test]
    fn regression_106() {
        gi_from_str("/", " ");
    }

    #[test]
    fn case_insensitive() {
        let gi = GitignoreBuilder::new(ROOT)
            .case_insensitive(true)
            .unwrap()
            .add_str(None, "*.html")
            .unwrap()
            .build()
            .unwrap();
        assert!(gi.matched("foo.html", false).is_ignore());
        assert!(gi.matched("foo.HTML", false).is_ignore());
        assert!(!gi.matched("foo.htm", false).is_ignore());
        assert!(!gi.matched("foo.HTM", false).is_ignore());
    }

    ignored!(cs1, ROOT, "*.html", "foo.html");
    not_ignored!(cs2, ROOT, "*.html", "foo.HTML");
    not_ignored!(cs3, ROOT, "*.html", "foo.htm");
    not_ignored!(cs4, ROOT, "*.html", "foo.HTM");
}
