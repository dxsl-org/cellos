//! `rg` argv (proposal §3.2, `command-contracts.md` §1 and §2.1).
//!
//! Words are scanned left to right until `--` or the first operand; a bare `-` is an
//! operand. Every other `-` word before `--` must be an exact supported spelling: no
//! combined short options, `--name=value`, abbreviations or upstream-only flags. Value
//! options consume exactly the next word. Parsing is pure and runs at admission, so every
//! error here is status 2 before any effect.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::text::escape_line;

/// Encoded argv bound `Σ(len + 1)` over every word including `rg` (contract §1, D76-13).
pub const MAX_ARGV_BYTES: usize = 512;
/// At most this many path operands.
pub const MAX_PATHS: usize = 16;
/// At most this many patterns (positional or `-e`).
pub const MAX_PATTERNS: usize = 32;
/// Aggregate UTF-8 pattern bytes.
pub const MAX_PATTERN_BYTES: usize = 4096;
/// At most this many `-g` globs.
pub const MAX_GLOBS: usize = 32;
/// Aggregate glob bytes.
pub const MAX_GLOB_BYTES: usize = 4096;
/// Largest `-A`/`-B`/`-C` value.
pub const MAX_CONTEXT: usize = 20;

/// Help text printed for a sole `-h`/`--help`.
pub const USAGE: &str = "\
usage: rg [OPTIONS] PATTERN [PATH ...]
       rg [OPTIONS] -e PATTERN [-e PATTERN ...] [PATH ...]
       rg -h | --help

Search the files below each PATH (default: the current directory, never stdin) for
lines matching PATTERN. With -e every operand is a PATH. A sole PATH '-' searches stdin
and requires -a. Options precede operands; '--' ends the options and is required before
an operand that starts with '-'. Rows are cellbox.search.line records.

Options:
  -e, --regexp PATTERN      add a pattern (repeatable; at most 32, 4096 bytes in total)
  -F, --fixed-strings       treat every pattern as a literal string
  -i, --ignore-case         ignore case (Unicode simple case folding; no smart case)
  -w, --word-regexp         match whole words only (not with -x)
  -x, --line-regexp         match whole lines only (not with -w)
  -n, --line-number         show line numbers (already the default)
  -q, --quiet               print no rows, still search all input (not with -A/-B/-C)
  -A, --after-context N     show N lines after each match (N = 0..20)
  -B, --before-context N    show N lines before each match (N = 0..20)
  -C, --context N           show N lines before and after (N = 0..20; not with -A/-B)
  -g, --glob GLOB           include matching files, exclude with '!GLOB' (repeatable;
                            at most 32, 4096 bytes in total)
      --hidden              search hidden (dot) entries
      --no-ignore           disable .gitignore/.ignore/.rgignore rules
  -a                        search files containing NUL bytes instead of skipping them
      --encoding none       no transcoding or BOM handling (the only value; the default)
      --text                render plain text instead of typed JSONL when not at the
                            terminal
  -h, --help                show this help

Differences from upstream ripgrep:
  --text selects the plain-text renderer; it is NOT an alias for -a.
  Displayed paths are absolute, the same bytes as the record's path.
  Symbolic links are never followed.

Text output is PATH:LINE:TEXT for a match and PATH-LINE-TEXT for context ('<stdin>'
for stdin). Control characters, invalid UTF-8 and ':'/'-' in paths are escaped.

Exit status: 0 a line matched, 1 no line matched, 2 error (even after matches),
130 interrupted.
";

/// Parsed `rg` argv.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RgArgs {
    /// Patterns in argument order; never empty unless `help`.
    pub patterns: Vec<String>,
    /// Path operands in argument order; empty means the shell cwd (or stdin when `stdin`).
    pub paths: Vec<String>,
    /// The sole path was `-`: search stdin bytes (`search_binary` is then set).
    pub stdin: bool,
    pub fixed: bool,
    pub ignore_case: bool,
    pub word: bool,
    pub line: bool,
    pub quiet: bool,
    pub before: usize,
    pub after: usize,
    /// `-g` globs in argument order.
    pub globs: Vec<String>,
    pub hidden: bool,
    pub no_ignore: bool,
    /// `-a`: search NUL-containing input as bytes instead of skipping it.
    pub search_binary: bool,
    /// `--text`: the plain-text renderer (not upstream's `-a` alias).
    pub text_display: bool,
    /// Sole `-h`/`--help`; every other field is the default.
    pub help: bool,
}

/// A usage error: the message without the `rg: ` prefix (status 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opt {
    Regexp,
    Fixed,
    IgnoreCase,
    Word,
    Line,
    LineNumber,
    Quiet,
    After,
    Before,
    Context,
    Glob,
    Hidden,
    NoIgnore,
    Binary,
    Encoding,
    Text,
    Help,
}

impl Opt {
    fn repeatable(self) -> bool {
        matches!(self, Opt::Regexp | Opt::Glob)
    }

    fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// Every supported spelling; anything else starting with `-` (except a bare `-`) rejects.
const SPELLINGS: &[(&str, Opt)] = &[
    ("-e", Opt::Regexp),
    ("--regexp", Opt::Regexp),
    ("-F", Opt::Fixed),
    ("--fixed-strings", Opt::Fixed),
    ("-i", Opt::IgnoreCase),
    ("--ignore-case", Opt::IgnoreCase),
    ("-w", Opt::Word),
    ("--word-regexp", Opt::Word),
    ("-x", Opt::Line),
    ("--line-regexp", Opt::Line),
    ("-n", Opt::LineNumber),
    ("--line-number", Opt::LineNumber),
    ("-q", Opt::Quiet),
    ("--quiet", Opt::Quiet),
    ("-A", Opt::After),
    ("--after-context", Opt::After),
    ("-B", Opt::Before),
    ("--before-context", Opt::Before),
    ("-C", Opt::Context),
    ("--context", Opt::Context),
    ("-g", Opt::Glob),
    ("--glob", Opt::Glob),
    ("--hidden", Opt::Hidden),
    ("--no-ignore", Opt::NoIgnore),
    ("-a", Opt::Binary),
    ("--encoding", Opt::Encoding),
    ("--text", Opt::Text),
    ("-h", Opt::Help),
    ("--help", Opt::Help),
];

fn lookup(word: &str) -> Option<Opt> {
    SPELLINGS.iter().find(|(spelling, _)| *spelling == word).map(|&(_, opt)| opt)
}

fn usage<T>(msg: String) -> Result<T, Usage> {
    Err(Usage(msg))
}

/// `word` as safe display text: argv words are untrusted and reach the terminal.
fn shown(word: &str) -> String {
    let mut out = Vec::new();
    escape_line(word.as_bytes(), &mut out);
    // The escaped form is ASCII plus valid UTF-8 by construction.
    String::from_utf8(out).unwrap_or_default()
}

/// The value of the option at `words[*index - 1]`: exactly the next word, whatever it is.
fn value<'a>(words: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, Usage> {
    match words.get(*index) {
        Some(word) => {
            *index += 1;
            Ok(word.as_str())
        }
        None => usage(format!("option {} requires a value", option)),
    }
}

/// A context line count: decimal digits only, `0..=MAX_CONTEXT`.
fn context_lines(option: &str, text: &str) -> Result<usize, Usage> {
    let decimal = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    match text.parse::<usize>() {
        Ok(n) if decimal && n <= MAX_CONTEXT => Ok(n),
        _ => usage(format!(
            "option {} needs a decimal line count 0..{}, got '{}'",
            option,
            MAX_CONTEXT,
            shown(text)
        )),
    }
}

/// Parse `argv` (`argv[0]` is `rg`) into [`RgArgs`]; every failure is a usage error.
pub fn parse(argv: &[String]) -> Result<RgArgs, Usage> {
    parse_within(argv, MAX_ARGV_BYTES)
}

/// [`parse`] with an explicit encoded-argv bound. The 512-byte argv bound shadows the
/// 4,096-byte pattern and glob bounds, which stay enforced (and testable) independently.
pub(super) fn parse_within(argv: &[String], max_argv_bytes: usize) -> Result<RgArgs, Usage> {
    let encoded = argv.iter().fold(0usize, |sum, word| sum.saturating_add(word.len()).saturating_add(1));
    if encoded > max_argv_bytes {
        return usage(format!("argv is {} bytes encoded; the limit is {}", encoded, max_argv_bytes));
    }
    if argv.iter().any(|word| word.as_bytes().contains(&0)) {
        return usage(String::from("an argument contains a NUL byte"));
    }
    let words = argv.get(1..).unwrap_or(&[]);
    let mut args = RgArgs::default();
    let mut seen = 0u32;
    let mut context = None;
    let mut ended = false;
    let mut index = 0;

    // Options, up to `--` or the first operand.
    while let Some(word) = words.get(index) {
        let word = word.as_str();
        if word == "--" {
            ended = true;
            index += 1;
            break;
        }
        if word == "-" || !word.starts_with('-') {
            break;
        }
        let Some(opt) = lookup(word) else {
            return usage(format!("unsupported option: {}", shown(word)));
        };
        if !opt.repeatable() && seen & opt.bit() != 0 {
            return usage(format!("option given more than once: {}", word));
        }
        seen |= opt.bit();
        index += 1;
        match opt {
            Opt::Regexp => args.patterns.push(String::from(value(words, &mut index, word)?)),
            Opt::Fixed => args.fixed = true,
            Opt::IgnoreCase => args.ignore_case = true,
            Opt::Word => args.word = true,
            Opt::Line => args.line = true,
            // Line numbers are always rendered and never change records.
            Opt::LineNumber => {}
            Opt::Quiet => args.quiet = true,
            Opt::After => args.after = context_lines(word, value(words, &mut index, word)?)?,
            Opt::Before => args.before = context_lines(word, value(words, &mut index, word)?)?,
            Opt::Context => context = Some(context_lines(word, value(words, &mut index, word)?)?),
            Opt::Glob => args.globs.push(String::from(value(words, &mut index, word)?)),
            Opt::Hidden => args.hidden = true,
            Opt::NoIgnore => args.no_ignore = true,
            Opt::Binary => args.search_binary = true,
            Opt::Encoding => {
                let encoding = value(words, &mut index, word)?;
                if encoding != "none" {
                    return usage(format!("unsupported encoding: {} (only 'none')", shown(encoding)));
                }
            }
            Opt::Text => args.text_display = true,
            Opt::Help => {
                if words.len() != 1 {
                    return usage(format!("{} must be the only argument", word));
                }
                return Ok(RgArgs { help: true, ..RgArgs::default() });
            }
        }
    }

    // Operands. Options precede operands: past the first operand, a `-` word still needs
    // `--` (which may come there) to be an operand.
    let positional = args.patterns.is_empty();
    for word in &words[index..] {
        let word = word.as_str();
        if !ended {
            if word == "--" {
                ended = true;
                continue;
            }
            if word != "-" && word.starts_with('-') {
                return usage(format!(
                    "option after an operand: {} (options precede operands; use -- before an \
                     operand starting with '-')",
                    shown(word)
                ));
            }
        }
        if positional && args.patterns.is_empty() {
            args.patterns.push(String::from(word));
        } else {
            args.paths.push(String::from(word));
        }
    }

    let has = |opt: Opt| seen & opt.bit() != 0;
    if has(Opt::Word) && has(Opt::Line) {
        return usage(String::from("-w and -x cannot be combined"));
    }
    if has(Opt::Quiet) && (has(Opt::After) || has(Opt::Before) || has(Opt::Context)) {
        return usage(String::from("-q cannot be combined with -A, -B or -C"));
    }
    if has(Opt::Context) && (has(Opt::After) || has(Opt::Before)) {
        return usage(String::from("-C cannot be combined with -A or -B"));
    }
    if let Some(n) = context {
        args.before = n;
        args.after = n;
    }

    if args.patterns.is_empty() {
        return usage(String::from("missing PATTERN"));
    }
    if args.patterns.len() > MAX_PATTERNS {
        return usage(format!("at most {} patterns", MAX_PATTERNS));
    }
    if args.patterns.iter().map(String::len).sum::<usize>() > MAX_PATTERN_BYTES {
        return usage(format!("patterns exceed {} bytes in total", MAX_PATTERN_BYTES));
    }
    if args.patterns.iter().any(|pattern| pattern.contains('\n')) {
        return usage(String::from("a pattern contains a line feed (multiline search is unsupported)"));
    }
    if args.globs.len() > MAX_GLOBS {
        return usage(format!("at most {} globs", MAX_GLOBS));
    }
    if args.globs.iter().map(String::len).sum::<usize>() > MAX_GLOB_BYTES {
        return usage(format!("globs exceed {} bytes in total", MAX_GLOB_BYTES));
    }
    if args.globs.iter().any(String::is_empty) {
        return usage(String::from("empty glob"));
    }
    if args.paths.len() > MAX_PATHS {
        return usage(format!("at most {} paths", MAX_PATHS));
    }
    if args.paths.iter().any(String::is_empty) {
        return usage(String::from("empty path"));
    }
    if args.paths.iter().any(|path| path == "-") {
        if args.paths.len() != 1 {
            return usage(String::from("stdin path - must be the only path"));
        }
        if !args.search_binary {
            return usage(String::from("stdin path - requires -a"));
        }
        args.stdin = true;
        args.paths.clear();
    }
    Ok(args)
}
