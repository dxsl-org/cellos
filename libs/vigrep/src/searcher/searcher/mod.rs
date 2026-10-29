// Adapted for vigrep (no_std + alloc) from ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f,
// upstream path `crates/searcher/src/searcher/mod.rs`.
// Copyright (c) 2015 Andrew Gallant
// SPDX-License-Identifier: MIT

#[allow(unused_imports)]
use crate::prelude::*;

use ::core::{cell::RefCell, cmp};

use crate::matcher::{LineTerminator, Match, Matcher};

use crate::{
    io,
    searcher::{
        line_buffer::{
            self, BufferAllocation, DEFAULT_BUFFER_CAPACITY, LineBuffer,
            LineBufferBuilder, LineBufferReader, alloc_error,
        },
        searcher::glue::{MultiLine, ReadByLine, SliceByLine},
        sink::{Sink, SinkError},
    },
};

// vigrep: `mmap.rs` (the only `unsafe` upstream) is dropped together with
// `MmapChoice`, encoding/BOM transcoding and the path/file entry points.
mod core;
mod glue;

/// We use this type alias since we want the ergonomics of a matcher's `Match`
/// type, but in practice, we use it for arbitrary ranges, so give it a more
/// accurate name. This is only used in the searcher's internals.
type Range = Match;

/// The behavior of binary detection while searching.
///
/// Binary detection is the process of _heuristically_ identifying whether a
/// given chunk of data is binary or not, and then taking an action based on
/// the result of that heuristic. The motivation behind detecting binary data
/// is that binary data often indicates data that is undesirable to search
/// using textual patterns. Of course, there are many cases in which this isn't
/// true, which is why binary detection is disabled by default.
///
/// Unfortunately, binary detection works differently depending on the type of
/// search being executed:
///
/// 1. When performing a search using a fixed size buffer, binary detection is
///    applied to the buffer's contents as it is filled. Binary detection must
///    be applied to the buffer directly because binary files may not contain
///    line terminators, which could result in exorbitant memory usage.
/// 2. When performing a search using memory maps or by reading data off the
///    heap, then binary detection is only guaranteed to be applied to the
///    parts corresponding to a match. When `Quit` is enabled, then the first
///    few KB of the data are searched for binary data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BinaryDetection(line_buffer::BinaryDetection);

impl BinaryDetection {
    /// No binary detection is performed. Data reported by the searcher may
    /// contain arbitrary bytes.
    ///
    /// This is the default.
    pub fn none() -> BinaryDetection {
        BinaryDetection(line_buffer::BinaryDetection::None)
    }

    /// Binary detection is performed by looking for the given byte.
    ///
    /// When searching is performed using a fixed size buffer, then the
    /// contents of that buffer are always searched for the presence of this
    /// byte. If it is found, then the underlying data is considered binary
    /// and the search stops as if it reached EOF.
    ///
    /// When searching is performed with the entire contents mapped into
    /// memory, then binary detection is more conservative. Namely, only a
    /// fixed sized region at the beginning of the contents are detected for
    /// binary data. As a compromise, any subsequent matching (or context)
    /// lines are also searched for binary data. If binary data is detected at
    /// any point, then the search stops as if it reached EOF.
    pub fn quit(binary_byte: u8) -> BinaryDetection {
        BinaryDetection(line_buffer::BinaryDetection::Quit(binary_byte))
    }

    /// Binary detection is performed by looking for the given byte, and
    /// replacing it with the line terminator configured on the searcher.
    /// (If the searcher is configured to use `CRLF` as the line terminator,
    /// then this byte is replaced by just `LF`.)
    ///
    /// When searching is performed using a fixed size buffer, then the
    /// contents of that buffer are always searched for the presence of this
    /// byte and replaced with the line terminator. In effect, the caller is
    /// guaranteed to never observe this byte while searching.
    ///
    /// When searching is performed with the entire contents mapped into
    /// memory, then this setting has no effect and is ignored.
    pub fn convert(binary_byte: u8) -> BinaryDetection {
        BinaryDetection(line_buffer::BinaryDetection::Convert(binary_byte))
    }

    /// If this binary detection uses the "quit" strategy, then this returns
    /// the byte that will cause a search to quit. In any other case, this
    /// returns `None`.
    pub fn quit_byte(&self) -> Option<u8> {
        match self.0 {
            line_buffer::BinaryDetection::Quit(b) => Some(b),
            _ => None,
        }
    }

    /// If this binary detection uses the "convert" strategy, then this returns
    /// the byte that will be replaced by the line terminator. In any other
    /// case, this returns `None`.
    pub fn convert_byte(&self) -> Option<u8> {
        match self.0 {
            line_buffer::BinaryDetection::Convert(b) => Some(b),
            _ => None,
        }
    }
}

/// The internal configuration of a searcher. This is shared among several
/// search related types, but is only ever written to by the SearcherBuilder.
#[derive(Clone, Debug)]
pub struct Config {
    /// The line terminator to use.
    line_term: LineTerminator,
    /// Whether to invert matching.
    invert_match: bool,
    /// The number of lines after a match to include.
    after_context: usize,
    /// The number of lines before a match to include.
    before_context: usize,
    /// Whether to enable unbounded context or not.
    passthru: bool,
    /// Whether to count line numbers.
    line_number: bool,
    /// The maximum amount of heap memory to use.
    ///
    /// When not given, no explicit limit is enforced. When set to `0`, no
    /// search strategy is available.
    heap_limit: Option<usize>,
    /// vigrep: the line buffer capacity (upstream's fixed 64 KiB default),
    /// exposed through [`SearcherBuilder::buffer_capacity`].
    buffer_capacity: usize,
    /// The binary data detection strategy.
    binary: BinaryDetection,
    /// Whether to enable matching across multiple lines.
    multi_line: bool,
    /// Whether to stop searching when a non-matching line is found after a
    /// matching line.
    stop_on_nonmatch: bool,
    /// The maximum number of matches this searcher should emit.
    max_matches: Option<u64>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            line_term: LineTerminator::default(),
            invert_match: false,
            after_context: 0,
            before_context: 0,
            passthru: false,
            line_number: true,
            heap_limit: None,
            buffer_capacity: DEFAULT_BUFFER_CAPACITY,
            binary: BinaryDetection::default(),
            multi_line: false,
            stop_on_nonmatch: false,
            max_matches: None,
        }
    }
}

impl Config {
    /// Return the maximal amount of lines needed to fulfill this
    /// configuration's context.
    ///
    /// If this returns `0`, then no context is ever needed.
    fn max_context(&self) -> usize {
        cmp::max(self.before_context, self.after_context)
    }

    /// Build a line buffer from this configuration.
    fn line_buffer(&self) -> LineBuffer {
        let mut builder = LineBufferBuilder::new();
        builder
            .line_terminator(self.line_term.as_byte())
            .binary_detection(self.binary.0);

        builder.capacity(self.buffer_capacity);
        if let Some(limit) = self.heap_limit {
            let (capacity, additional) = if limit <= self.buffer_capacity {
                (limit, 0)
            } else {
                (self.buffer_capacity, limit - self.buffer_capacity)
            };
            builder
                .capacity(capacity)
                .buffer_alloc(BufferAllocation::Error(additional));
        }
        builder.build()
    }
}

/// An error that can occur when building a searcher.
///
/// This error occurs when a non-sensical configuration is present when trying
/// to construct a `Searcher` from a `SearcherBuilder`.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConfigError {
    /// Indicates that the heap limit configuration prevents all possible
    /// search strategies from being used, i.e. the heap limit is set to 0.
    SearchUnavailable,
    /// Occurs when a matcher reports a line terminator that is different than
    /// the one configured in the searcher.
    MismatchedLineTerminators {
        /// The matcher's line terminator.
        matcher: LineTerminator,
        /// The searcher's line terminator.
        searcher: LineTerminator,
    },
}

impl ::core::error::Error for ConfigError {}

impl ::core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match *self {
            ConfigError::SearchUnavailable => {
                write!(f, "grep config error: no available searchers")
            }
            ConfigError::MismatchedLineTerminators { matcher, searcher } => {
                write!(
                    f,
                    "grep config error: mismatched line terminators, \
                     matcher has {:?} but searcher has {:?}",
                    matcher, searcher
                )
            }
        }
    }
}

/// A builder for configuring a searcher.
///
/// A search builder permits specifying the configuration of a searcher,
/// including options like whether to invert the search or to enable multi
/// line search.
///
/// Once a searcher has been built, it is beneficial to reuse that searcher
/// for multiple searches, if possible.
#[derive(Clone, Debug)]
pub struct SearcherBuilder {
    config: Config,
}

impl Default for SearcherBuilder {
    fn default() -> SearcherBuilder {
        SearcherBuilder::new()
    }
}

impl SearcherBuilder {
    /// Create a new searcher builder with a default configuration.
    pub fn new() -> SearcherBuilder {
        SearcherBuilder { config: Config::default() }
    }

    /// Build a searcher with the given matcher.
    pub fn build(&self) -> Searcher {
        let mut config = self.config.clone();
        if config.passthru {
            config.before_context = 0;
            config.after_context = 0;
        }

        Searcher {
            config,
            line_buffer: RefCell::new(self.config.line_buffer()),
            multi_line_buffer: RefCell::new(vec![]),
        }
    }

    /// Set the line terminator that is used by the searcher.
    ///
    /// When using a searcher, if the matcher provided has a line terminator
    /// set, then it must be the same as this one. If they aren't, building
    /// a searcher will return an error.
    ///
    /// By default, this is set to `b'\n'`.
    pub fn line_terminator(
        &mut self,
        line_term: LineTerminator,
    ) -> &mut SearcherBuilder {
        self.config.line_term = line_term;
        self
    }

    /// Whether to invert matching, whereby lines that don't match are reported
    /// instead of reporting lines that do match.
    ///
    /// By default, this is disabled.
    pub fn invert_match(&mut self, yes: bool) -> &mut SearcherBuilder {
        self.config.invert_match = yes;
        self
    }

    /// Whether to count and include line numbers with matching lines.
    ///
    /// This is enabled by default. There is a small performance penalty
    /// associated with computing line numbers, so this can be disabled when
    /// this isn't desirable.
    pub fn line_number(&mut self, yes: bool) -> &mut SearcherBuilder {
        self.config.line_number = yes;
        self
    }

    /// Whether to enable multi line search or not.
    ///
    /// When multi line search is enabled, matches *may* match across multiple
    /// lines. Conversely, when multi line search is disabled, it is impossible
    /// for any match to span more than one line.
    ///
    /// **Warning:** multi line search requires having the entire contents to
    /// search in memory at once. When searching a reader, the entire contents
    /// of the stream are read on to the heap (bounded by the heap limit)
    /// before starting the search.
    ///
    /// This is disabled by default.
    pub fn multi_line(&mut self, yes: bool) -> &mut SearcherBuilder {
        self.config.multi_line = yes;
        self
    }

    /// Whether to include a fixed number of lines after every match.
    ///
    /// When this is set to a non-zero number, then the searcher will report
    /// `line_count` contextual lines after every match.
    ///
    /// This is set to `0` by default.
    pub fn after_context(
        &mut self,
        line_count: usize,
    ) -> &mut SearcherBuilder {
        self.config.after_context = line_count;
        self
    }

    /// Whether to include a fixed number of lines before every match.
    ///
    /// When this is set to a non-zero number, then the searcher will report
    /// `line_count` contextual lines before every match.
    ///
    /// This is set to `0` by default.
    pub fn before_context(
        &mut self,
        line_count: usize,
    ) -> &mut SearcherBuilder {
        self.config.before_context = line_count;
        self
    }

    /// Whether to enable the "passthru" feature or not.
    ///
    /// When passthru is enabled, it effectively treats all non-matching lines
    /// as contextual lines. In other words, enabling this is akin to
    /// requesting an unbounded number of before and after contextual lines.
    ///
    /// When passthru mode is enabled, any `before_context` or `after_context`
    /// settings are ignored by setting them to `0`.
    ///
    /// This is disabled by default.
    pub fn passthru(&mut self, yes: bool) -> &mut SearcherBuilder {
        self.config.passthru = yes;
        self
    }

    /// Set an approximate limit on the amount of heap space used by a
    /// searcher.
    ///
    /// The heap limit is enforced in two scenarios:
    ///
    /// * When searching using a fixed size buffer, the heap limit controls
    ///   how big this buffer is allowed to be. Assuming contexts are disabled,
    ///   the minimum size of this buffer is the length (in bytes) of the
    ///   largest single line in the contents being searched. If any line
    ///   exceeds the heap limit, then an error will be returned.
    /// * When performing a multi line search, a fixed size buffer cannot be
    ///   used. Thus, the entire contents are read on to the heap and the heap
    ///   limit set here is enforced.
    ///
    /// If a heap limit is set to `0`, then no search strategy is available
    /// and the searcher will return an error immediately.
    ///
    /// By default, no limit is set.
    pub fn heap_limit(
        &mut self,
        bytes: Option<usize>,
    ) -> &mut SearcherBuilder {
        self.config.heap_limit = bytes;
        self
    }

    /// vigrep: set the capacity of the line buffer used by reader searches.
    ///
    /// This is the amount of data held in memory before any long-line
    /// growth, and the size of each read made to the underlying reader.
    /// When a heap limit is set and is smaller than this capacity, the heap
    /// limit wins; otherwise the line buffer may grow by at most
    /// `heap_limit - capacity` bytes to fit a long line (plus context).
    ///
    /// The default is upstream's 64 KiB.
    pub fn buffer_capacity(&mut self, bytes: usize) -> &mut SearcherBuilder {
        self.config.buffer_capacity = bytes;
        self
    }

    /// Set the binary detection strategy.
    ///
    /// The binary detection strategy determines not only how the searcher
    /// detects binary data, but how it responds to the presence of binary
    /// data. See the [`BinaryDetection`] type for more information.
    ///
    /// By default, binary detection is disabled.
    pub fn binary_detection(
        &mut self,
        detection: BinaryDetection,
    ) -> &mut SearcherBuilder {
        self.config.binary = detection;
        self
    }

    /// Stop searching a file when a non-matching line is found after a
    /// matching line.
    ///
    /// This is useful for searching sorted files where it is expected that all
    /// the matches will be on adjacent lines.
    pub fn stop_on_nonmatch(
        &mut self,
        stop_on_nonmatch: bool,
    ) -> &mut SearcherBuilder {
        self.config.stop_on_nonmatch = stop_on_nonmatch;
        self
    }

    /// Sets the maximum number of matches that should be emitted by this
    /// searcher.
    ///
    /// If multi line search is enabled and a match spans multiple lines, then
    /// that match is counted exactly once for the purposes of enforcing this
    /// limit, regardless of how many lines it spans.
    ///
    /// Note that `0` is a legal value. This will cause the searcher to
    /// immediately quit without searching anything.
    ///
    /// By default, no limit is set.
    #[inline]
    pub fn max_matches(&mut self, limit: Option<u64>) -> &mut SearcherBuilder {
        self.config.max_matches = limit;
        self
    }
}

/// A searcher executes searches over a haystack and writes results to a caller
/// provided sink.
///
/// Matches are detected via implementations of the `Matcher` trait, which must
/// be provided by the caller when executing a search.
///
/// When possible, a searcher should be reused.
#[derive(Clone, Debug)]
pub struct Searcher {
    /// The configuration for this searcher.
    ///
    /// We make most of these settings available to users of `Searcher` via
    /// public API methods, which can be queried in implementations of `Sink`
    /// if necessary.
    config: Config,
    /// A line buffer for use in line oriented searching.
    ///
    /// We wrap it in a RefCell to permit lending out borrows of `Searcher`
    /// to sinks. We still require a mutable borrow to execute a search, so
    /// we statically prevent callers from causing RefCell to panic at runtime
    /// due to a borrowing violation.
    line_buffer: RefCell<LineBuffer>,
    /// A buffer in which to store the contents of a reader when performing a
    /// multi line search. In particular, multi line searches cannot be
    /// performed incrementally, and need the entire haystack in memory at
    /// once.
    multi_line_buffer: RefCell<Vec<u8>>,
}

impl Searcher {
    /// Create a new searcher with a default configuration.
    ///
    /// To configure the searcher (e.g., invert matching, enable memory maps,
    /// enable contexts, etc.), use the [`SearcherBuilder`].
    pub fn new() -> Searcher {
        SearcherBuilder::new().build()
    }

    /// Execute a search over any implementation of [`io::Read`] and write
    /// the results to the given sink.
    ///
    /// When possible, this implementation will search the reader incrementally
    /// without reading it into memory. In some cases---for example, if multi
    /// line search is enabled---an incremental search isn't possible and the
    /// given reader is consumed completely and placed on the heap before
    /// searching begins.
    ///
    /// vigrep: bytes are searched as read; there is no BOM sniffing or
    /// transcoding.
    pub fn search_reader<M, R, S>(
        &mut self,
        matcher: M,
        read_from: R,
        write_to: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        R: io::Read,
        S: Sink,
    {
        self.check_config(&matcher).map_err(S::Error::error_config)?;

        if self.multi_line_with_matcher(&matcher) {
            log::trace!(
                "generic reader: reading everything to heap for multiline"
            );
            self.fill_multi_line_buffer_from_reader::<_, S>(read_from)?;
            log::trace!("generic reader: searching via multiline strategy");
            MultiLine::new(
                self,
                matcher,
                &*self.multi_line_buffer.borrow(),
                write_to,
            )
            .run()
        } else {
            let mut line_buffer = self.line_buffer.borrow_mut();
            let rdr = LineBufferReader::new(read_from, &mut *line_buffer);
            log::trace!("generic reader: searching via roll buffer strategy");
            ReadByLine::new(self, matcher, rdr, write_to).run()
        }
    }

    /// Execute a search over the given slice and write the results to the
    /// given sink.
    pub fn search_slice<M, S>(
        &mut self,
        matcher: M,
        slice: &[u8],
        write_to: S,
    ) -> Result<(), S::Error>
    where
        M: Matcher,
        S: Sink,
    {
        self.check_config(&matcher).map_err(S::Error::error_config)?;

        if self.multi_line_with_matcher(&matcher) {
            log::trace!("slice reader: searching via multiline strategy");
            MultiLine::new(self, matcher, slice, write_to).run()
        } else {
            log::trace!("slice reader: searching via slice-by-line strategy");
            SliceByLine::new(self, matcher, slice, write_to).run()
        }
    }

    /// Set the binary detection method used on this searcher.
    pub fn set_binary_detection(&mut self, detection: BinaryDetection) {
        self.config.binary = detection.clone();
        self.line_buffer.borrow_mut().set_binary_detection(detection.0);
    }

    /// Check that the searcher's configuration and the matcher are consistent
    /// with each other.
    fn check_config<M: Matcher>(&self, matcher: M) -> Result<(), ConfigError> {
        if self.config.heap_limit == Some(0) {
            return Err(ConfigError::SearchUnavailable);
        }
        let matcher_line_term = match matcher.line_terminator() {
            None => return Ok(()),
            Some(line_term) => line_term,
        };
        if matcher_line_term != self.config.line_term {
            return Err(ConfigError::MismatchedLineTerminators {
                matcher: matcher_line_term,
                searcher: self.config.line_term,
            });
        }
        Ok(())
    }
}

/// The following methods permit querying the configuration of a searcher.
/// These can be useful in generic implementations of [`Sink`], where the
/// output may be tailored based on how the searcher is configured.
impl Searcher {
    /// Returns the line terminator used by this searcher.
    #[inline]
    pub fn line_terminator(&self) -> LineTerminator {
        self.config.line_term
    }

    /// Returns the type of binary detection configured on this searcher.
    #[inline]
    pub fn binary_detection(&self) -> &BinaryDetection {
        &self.config.binary
    }

    /// Returns true if and only if this searcher is configured to invert its
    /// search results. That is, matching lines are lines that do **not** match
    /// the searcher's matcher.
    #[inline]
    pub fn invert_match(&self) -> bool {
        self.config.invert_match
    }

    /// Returns true if and only if this searcher is configured to count line
    /// numbers.
    #[inline]
    pub fn line_number(&self) -> bool {
        self.config.line_number
    }

    /// Returns true if and only if this searcher is configured to perform
    /// multi line search.
    #[inline]
    pub fn multi_line(&self) -> bool {
        self.config.multi_line
    }

    /// Returns true if and only if this searcher is configured to stop when it
    /// finds a non-matching line after a matching one.
    #[inline]
    pub fn stop_on_nonmatch(&self) -> bool {
        self.config.stop_on_nonmatch
    }

    /// Returns the maximum number of matches emitted by this searcher, if
    /// such a limit was set.
    ///
    /// If multi line search is enabled and a match spans multiple lines, then
    /// that match is counted exactly once for the purposes of enforcing this
    /// limit, regardless of how many lines it spans.
    ///
    /// Note that `0` is a legal value. This will cause the searcher to
    /// immediately quit without searching anything.
    #[inline]
    pub fn max_matches(&self) -> Option<u64> {
        self.config.max_matches
    }

    /// Returns true if and only if this searcher will choose a multi-line
    /// strategy given the provided matcher.
    ///
    /// This may diverge from the result of `multi_line` in cases where the
    /// searcher has been configured to execute a search that can report
    /// matches over multiple lines, but where the matcher guarantees that it
    /// will never produce a match over multiple lines.
    pub fn multi_line_with_matcher<M: Matcher>(&self, matcher: M) -> bool {
        if !self.multi_line() {
            return false;
        }
        if let Some(line_term) = matcher.line_terminator() {
            if line_term == self.line_terminator() {
                return false;
            }
        }
        if let Some(non_matching) = matcher.non_matching_bytes() {
            // If the line terminator is CRLF, we don't actually need to care
            // whether the regex can match `\r` or not. Namely, a `\r` is
            // neither necessary nor sufficient to terminate a line. A `\n` is
            // always required.
            if non_matching.contains(self.line_terminator().as_byte()) {
                return false;
            }
        }
        true
    }

    /// Returns the number of "after" context lines to report. When context
    /// reporting is not enabled, this returns `0`.
    #[inline]
    pub fn after_context(&self) -> usize {
        self.config.after_context
    }

    /// Returns the number of "before" context lines to report. When context
    /// reporting is not enabled, this returns `0`.
    #[inline]
    pub fn before_context(&self) -> usize {
        self.config.before_context
    }

    /// Returns true if and only if the searcher has "passthru" mode enabled.
    #[inline]
    pub fn passthru(&self) -> bool {
        self.config.passthru
    }

    /// vigrep: returns the configured line buffer capacity.
    #[inline]
    pub fn buffer_capacity(&self) -> usize {
        self.config.buffer_capacity
    }

    /// Returns the configured heap limit, if any.
    #[inline]
    pub fn heap_limit(&self) -> Option<usize> {
        self.config.heap_limit
    }

    /// Fill the buffer for use with multi-line searching from the given
    /// reader. This reads from the reader until EOF or until an error occurs.
    /// If the contents exceed the configured heap limit, then an error is
    /// returned.
    fn fill_multi_line_buffer_from_reader<R: io::Read, S: Sink>(
        &self,
        mut read_from: R,
    ) -> Result<(), S::Error> {
        assert!(self.config.multi_line);

        let mut buf = self.multi_line_buffer.borrow_mut();
        buf.clear();

        // If we don't have a heap limit, then read to the end...
        let heap_limit = match self.config.heap_limit {
            Some(heap_limit) => heap_limit,
            None => {
                read_to_end(&mut read_from, &mut *buf)
                    .map_err(S::Error::error_io)?;
                return Ok(());
            }
        };
        if heap_limit == 0 {
            return Err(S::Error::error_io(alloc_error(heap_limit)));
        }

        // ... otherwise we need to roll our own. This is likely quite a bit
        // slower than what is optimal, but we avoid worry about memory safety
        // until there's a compelling reason to speed this up.
        buf.resize(cmp::min(self.config.buffer_capacity, heap_limit), 0);
        let mut pos = 0;
        loop {
            let nread = match read_from.read(&mut buf[pos..]) {
                Ok(nread) => nread,
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(err) => return Err(S::Error::error_io(err)),
            };
            if nread == 0 {
                buf.resize(pos, 0);
                return Ok(());
            }

            pos += nread;
            if buf[pos..].is_empty() {
                let additional = heap_limit - buf.len();
                if additional == 0 {
                    return Err(S::Error::error_io(alloc_error(heap_limit)));
                }
                let limit = buf.len() + additional;
                let doubled = 2 * buf.len();
                buf.resize(cmp::min(doubled, limit), 0);
            }
        }
    }
}

/// vigrep: `std::io::Read::read_to_end` for [`io::Read`], retrying
/// interrupted reads.
fn read_to_end<R: io::Read>(
    rdr: &mut R,
    buf: &mut Vec<u8>,
) -> Result<(), io::Error> {
    const CHUNK: usize = 8 * (1 << 10);
    loop {
        let len = buf.len();
        buf.resize(len + CHUNK, 0);
        let result = rdr.read(&mut buf[len..]);
        buf.truncate(len + *result.as_ref().unwrap_or(&0));
        match result {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::searcher::testutil::{KitchenSink, RegexMatcher};

    use super::*;

    #[test]
    fn config_error_heap_limit() {
        let matcher = RegexMatcher::new("");
        let sink = KitchenSink::new();
        let mut searcher = SearcherBuilder::new().heap_limit(Some(0)).build();
        let res = searcher.search_slice(matcher, &[], sink);
        assert!(res.is_err());
    }

    #[test]
    fn config_error_line_terminator() {
        let mut matcher = RegexMatcher::new("");
        matcher.set_line_term(Some(LineTerminator::byte(b'z')));

        let sink = KitchenSink::new();
        let mut searcher = Searcher::new();
        let res = searcher.search_slice(matcher, &[], sink);
        assert!(res.is_err());
    }

    #[test]
    fn uft8_bom_sniffing() {
        // See: https://github.com/BurntSushi/ripgrep/issues/1638
        // vigrep: upstream strips the UTF-8 BOM (reporting "1:0:foo" and a
        // byte count of 3). vigrep has no BOM sniffing or transcoding, so the
        // BOM stays part of the first line and offsets count original bytes.
        let matcher = RegexMatcher::new("foo");
        let haystack: &[u8] = &[0xef, 0xbb, 0xbf, 0x66, 0x6f, 0x6f];

        let mut sink = KitchenSink::new();
        let mut searcher = SearcherBuilder::new().build();

        let res = searcher.search_slice(matcher, haystack, &mut sink);
        assert!(res.is_ok());

        let sink_output = String::from_utf8(sink.as_bytes().to_vec()).unwrap();
        assert_eq!(sink_output, "1:0:\u{feff}foo\nbyte count:6\n");
    }
}
