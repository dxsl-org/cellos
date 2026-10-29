//! Host tests of `rg`: argv admission (`args`) and the plain-text renderer (`text`).

use super::args::*;
use super::text::*;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use text_engine::pipeline::Value;

// ── args ──

fn argv(words: &[&str]) -> Vec<String> {
    core::iter::once("rg").chain(words.iter().copied()).map(String::from).collect()
}

fn ok(words: &[&str]) -> RgArgs {
    parse(&argv(words)).unwrap_or_else(|e| panic!("{:?} rejected: {:?}", words, e))
}

fn rejects(words: &[&str]) {
    let result = parse(&argv(words));
    assert!(result.is_err(), "{:?} accepted: {:?}", words, result);
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().copied().map(String::from).collect()
}

/// Pattern `p`, everything else default.
fn base() -> RgArgs {
    RgArgs { patterns: strings(&["p"]), ..RgArgs::default() }
}

#[test]
fn every_supported_spelling_sets_its_field() {
    let globbed = RgArgs { globs: strings(&["*.rs"]), ..base() };
    let cases: &[(&[&str], RgArgs)] = &[
        (&["p"], base()),
        (&["-e", "p"], base()),
        (&["--regexp", "p"], base()),
        (&["-F", "p"], RgArgs { fixed: true, ..base() }),
        (&["--fixed-strings", "p"], RgArgs { fixed: true, ..base() }),
        (&["-i", "p"], RgArgs { ignore_case: true, ..base() }),
        (&["--ignore-case", "p"], RgArgs { ignore_case: true, ..base() }),
        (&["-w", "p"], RgArgs { word: true, ..base() }),
        (&["--word-regexp", "p"], RgArgs { word: true, ..base() }),
        (&["-x", "p"], RgArgs { line: true, ..base() }),
        (&["--line-regexp", "p"], RgArgs { line: true, ..base() }),
        (&["-n", "p"], base()),
        (&["--line-number", "p"], base()),
        (&["-q", "p"], RgArgs { quiet: true, ..base() }),
        (&["--quiet", "p"], RgArgs { quiet: true, ..base() }),
        (&["-A", "3", "p"], RgArgs { after: 3, ..base() }),
        (&["--after-context", "3", "p"], RgArgs { after: 3, ..base() }),
        (&["-B", "2", "p"], RgArgs { before: 2, ..base() }),
        (&["--before-context", "2", "p"], RgArgs { before: 2, ..base() }),
        (&["-C", "4", "p"], RgArgs { before: 4, after: 4, ..base() }),
        (&["--context", "4", "p"], RgArgs { before: 4, after: 4, ..base() }),
        (&["-g", "*.rs", "p"], globbed.clone()),
        (&["--glob", "*.rs", "p"], globbed),
        (&["--hidden", "p"], RgArgs { hidden: true, ..base() }),
        (&["--no-ignore", "p"], RgArgs { no_ignore: true, ..base() }),
        (&["-a", "p"], RgArgs { search_binary: true, ..base() }),
        (&["--encoding", "none", "p"], base()),
        (&["--text", "p"], RgArgs { text_display: true, ..base() }),
    ];
    for (words, expected) in cases {
        assert_eq!(&ok(words), expected, "{:?}", words);
    }
}

#[test]
fn options_combine_and_operands_keep_their_order() {
    let all = ok(&[
        "-F", "-i", "-w", "-n", "-A", "1", "-B", "2", "-g", "a", "--glob", "!b", "--hidden",
        "--no-ignore", "-a", "--encoding", "none", "--text", "-e", "x", "--regexp", "y", "d1",
        "d2",
    ]);
    let expected = RgArgs {
        patterns: strings(&["x", "y"]),
        paths: strings(&["d1", "d2"]),
        fixed: true,
        ignore_case: true,
        word: true,
        before: 2,
        after: 1,
        globs: strings(&["a", "!b"]),
        hidden: true,
        no_ignore: true,
        search_binary: true,
        text_display: true,
        ..RgArgs::default()
    };
    assert_eq!(all, expected);
    // Without -e the first operand is the pattern and the rest are paths.
    let positional = ok(&["pat", "a", "b"]);
    assert_eq!((positional.patterns, positional.paths), (strings(&["pat"]), strings(&["a", "b"])));
    // With -e every operand is a path.
    let explicit = ok(&["-e", "pat", "a", "b"]);
    assert_eq!((explicit.patterns, explicit.paths), (strings(&["pat"]), strings(&["a", "b"])));
    assert_eq!(ok(&["-x", "-q", "p"]), RgArgs { line: true, quiet: true, ..base() });
}

#[test]
fn sole_help_sets_help_and_nothing_else() {
    for help in ["-h", "--help"] {
        assert_eq!(ok(&[help]), RgArgs { help: true, ..RgArgs::default() });
    }
    for words in [
        &["-h", "p"][..],
        &["--help", "p"],
        &["-i", "-h"],
        &["-h", "-i"],
        &["--help", "-h"],
        &["-h", "-h"],
        &["-e", "p", "--help"],
    ] {
        rejects(words);
    }
    // A consumed value or an operand spelled `-h` is not a help request.
    assert_eq!(ok(&["-e", "-h"]), RgArgs { patterns: strings(&["-h"]), ..RgArgs::default() });
    assert_eq!(ok(&["--", "--help"]), RgArgs { patterns: strings(&["--help"]), ..RgArgs::default() });
}

#[test]
fn value_options_consume_exactly_the_next_word() {
    let pattern = ok(&["-e", "--text"]);
    assert_eq!(pattern, RgArgs { patterns: strings(&["--text"]), ..RgArgs::default() });
    assert_eq!(ok(&["--", "--text"]), pattern);
    assert_eq!(ok(&["--regexp", "--"]).patterns, strings(&["--"]));
    let path = ok(&["-e", "p", "--", "--text"]);
    assert_eq!(path, RgArgs { paths: strings(&["--text"]), ..base() });
    let glob = ok(&["-g", "-i", "p"]);
    assert_eq!(glob, RgArgs { globs: strings(&["-i"]), ..base() });
    // `--` may also separate paths after the first operand.
    assert_eq!(ok(&["p", "a", "--", "-b"]).paths, strings(&["a", "-b"]));
    // A value option at the end has no value.
    for option in ["-e", "--regexp", "-A", "--after-context", "-B", "--before-context", "-C", "--context"] {
        rejects(&[option]);
        rejects(&["-e", "p", option]);
    }
    for option in ["-g", "--glob", "--encoding"] {
        rejects(&["-e", "p", option]);
    }
}

#[test]
fn text_selects_the_renderer_and_is_not_binary_search() {
    let text = ok(&["--text", "p"]);
    assert!(text.text_display && !text.search_binary);
    let binary = ok(&["-a", "p"]);
    assert!(binary.search_binary && !binary.text_display);
    // `--text` does not stand in for `-a` on stdin.
    rejects(&["--text", "p", "-"]);
}

#[test]
fn unsupported_and_malformed_options_reject_naming_the_option() {
    let unsupported = [
        // Explicitly unsupported upstream flags.
        "-u", "-uu", "-uuu", "-L", "--follow", "-P", "--pcre2", "-U", "--multiline", "-z",
        "--pre", "--search-zip", "--mmap", "-j", "--threads", "--engine", "-f", "--file",
        "--ignore-file", "--type", "--type-not", "--type-list", "--type-add", "-t", "-T",
        "--json", "-o", "--only-matching", "-r", "--replace", "-c", "--count",
        "--count-matches", "-l", "--files-with-matches", "--files-without-match", "--files",
        "--sort", "--sortr", "--sort-files", "-v", "--invert-match", "-S", "--smart-case",
        "-s", "--case-sensitive", "-m", "--max-count", "-M", "-N", "-H", "-E", "-.",
        "--binary", "--null", "--color", "--vimgrep", "--no-filename", "--byte-offset",
        // Combined short options and attached values.
        "-iw", "-in", "-A3", "-ep", "-gx", "-aF",
        // `--name=value` forms.
        "--glob=x", "--regexp=p", "--after-context=2", "--context=1", "--encoding=none",
        "--text=yes", "--hidden=1",
        // Abbreviations and near misses.
        "--hid", "--no-ign", "--regex", "--fixed", "--ignore", "--line", "--word", "--quie",
        "--tex", "--enc", "--help=1", "---", "-I", "--IGNORE-CASE",
    ];
    for flag in unsupported {
        let error = parse(&argv(&[flag, "p"])).expect_err(flag);
        assert!(error.0.contains(flag), "{:?}: {:?}", flag, error);
    }
}

#[test]
fn encoding_accepts_only_none() {
    for value in ["utf-8", "auto", "NONE", "", "latin1"] {
        rejects(&["--encoding", value, "p"]);
    }
}

#[test]
fn scalar_options_reject_repetition_in_any_spelling() {
    let scalars: [&[&str]; 15] = [
        &["-F", "--fixed-strings"],
        &["-i", "--ignore-case"],
        &["-w", "--word-regexp"],
        &["-x", "--line-regexp"],
        &["-n", "--line-number"],
        &["-q", "--quiet"],
        &["-A", "--after-context"],
        &["-B", "--before-context"],
        &["-C", "--context"],
        &["--hidden"],
        &["--no-ignore"],
        &["-a"],
        &["--text"],
        &["--encoding"],
        &["-h", "--help"],
    ];
    for spellings in scalars {
        let value: &[&str] = match spellings[0] {
            "-A" | "-B" | "-C" => &["1"],
            "--encoding" => &["none"],
            _ => &[],
        };
        for &first in spellings {
            for &second in spellings {
                let mut words: Vec<&str> = vec![first];
                words.extend_from_slice(value);
                words.push(second);
                words.extend_from_slice(value);
                words.push("p");
                rejects(&words);
            }
        }
    }
    let repeated = ok(&["-e", "a", "--regexp", "b", "-g", "x", "--glob", "y", "-g", "x"]);
    assert_eq!(repeated.patterns, strings(&["a", "b"]));
    assert_eq!(repeated.globs, strings(&["x", "y", "x"]));
}

#[test]
fn conflicting_options_reject() {
    for words in [
        &["-w", "-x", "p"][..],
        &["--line-regexp", "--word-regexp", "p"],
        &["-q", "-A", "1", "p"],
        &["-q", "-B", "0", "p"],
        &["-q", "-C", "2", "p"],
        &["--after-context", "0", "--quiet", "p"],
        &["-C", "1", "-A", "1", "p"],
        &["-B", "1", "-C", "1", "p"],
        &["--context", "0", "--before-context", "0", "p"],
    ] {
        rejects(words);
    }
    assert_eq!(ok(&["-A", "1", "-B", "2", "p"]), RgArgs { after: 1, before: 2, ..base() });
}

#[test]
fn context_counts_are_decimal_up_to_twenty() {
    for option in ["-A", "-B", "-C"] {
        assert!(parse(&argv(&[option, "0", "p"])).is_ok());
        assert!(parse(&argv(&[option, "20", "p"])).is_ok());
        assert!(parse(&argv(&[option, "07", "p"])).is_ok());
        for bad in ["21", "-1", "+1", "", " 1", "1 ", "1x", "0x1", "1.0", "99999999999999999999999"] {
            rejects(&[option, bad, "p"]);
        }
    }
    assert_eq!(ok(&["-C", "20", "p"]), RgArgs { before: 20, after: 20, ..base() });
}

#[test]
fn operand_rules() {
    rejects(&[]);
    rejects(&["-i"]);
    rejects(&["--"]);
    // An empty pattern is valid; an empty path is not.
    assert_eq!(ok(&[""]).patterns, strings(&[""]));
    assert_eq!(ok(&["-e", ""]).patterns, strings(&[""]));
    rejects(&["p", ""]);
    rejects(&["-e", "p", ""]);
    rejects(&["-e", "p", "a", ""]);
    // An empty glob is not a rule.
    rejects(&["-g", "", "p"]);
    // NUL in any word.
    rejects(&["a\0b"]);
    rejects(&["-e", "a\0"]);
    rejects(&["p", "dir\0"]);
    rejects(&["-g", "\0", "p"]);
    // A literal LF in a pattern is unsupported multiline search.
    rejects(&["a\nb"]);
    rejects(&["-e", "x", "-e", "\n"]);
    // Options precede operands; a later `-` word needs `--`.
    rejects(&["p", "-i"]);
    rejects(&["p", "dir", "--hidden"]);
    rejects(&["-e", "p", "dir", "-x"]);
    assert_eq!(ok(&["p", "--", "-i"]).paths, strings(&["-i"]));
    // A bare `-` is an operand, so it can be the pattern.
    assert_eq!(ok(&["-"]), RgArgs { patterns: strings(&["-"]), ..RgArgs::default() });
}

#[test]
fn stdin_is_the_sole_path_and_requires_a() {
    rejects(&["p", "-"]);
    rejects(&["-e", "p", "-"]);
    let stdin = ok(&["-a", "x", "-"]);
    assert_eq!(stdin, RgArgs { patterns: strings(&["x"]), stdin: true, search_binary: true, ..RgArgs::default() });
    assert!(stdin.paths.is_empty());
    assert!(ok(&["-a", "-e", "x", "-"]).stdin);
    assert!(ok(&["-a", "--", "x", "-"]).stdin);
    rejects(&["-a", "x", "-", "dir"]);
    rejects(&["-a", "x", "dir", "-"]);
    rejects(&["-a", "x", "-", "-"]);
    rejects(&["-a", "-e", "x", "-", "dir"]);
    // A path merely containing `-` is a path.
    let dashed = ok(&["x", "a-b"]);
    assert!(!dashed.stdin && dashed.paths == strings(&["a-b"]));
}

#[test]
fn limits_hold_at_the_bound_and_reject_one_past_it() {
    let names: Vec<String> = (0..=MAX_PATHS).map(|n| format!("d{}", n)).collect();
    let paths = |n: usize| {
        let mut words = vec!["p"];
        words.extend(names[..n].iter().map(String::as_str));
        parse(&argv(&words))
    };
    assert_eq!(paths(MAX_PATHS).unwrap().paths.len(), MAX_PATHS);
    assert!(paths(MAX_PATHS + 1).is_err());

    let repeated = |option: &'static str, n: usize| {
        let mut words: Vec<&str> = (0..n).flat_map(|_| [option, "x"]).collect();
        if option == "-g" {
            words.push("p");
        }
        parse(&argv(&words))
    };
    assert_eq!(repeated("-e", MAX_PATTERNS).unwrap().patterns.len(), MAX_PATTERNS);
    assert!(repeated("-e", MAX_PATTERNS + 1).is_err());
    assert_eq!(repeated("-g", MAX_GLOBS).unwrap().globs.len(), MAX_GLOBS);
    assert!(repeated("-g", MAX_GLOBS + 1).is_err());

    // Aggregate pattern and glob bytes sit above the 512-byte argv bound, so they are
    // exercised with that bound lifted.
    let long = |n: usize| "a".repeat(n);
    let unbounded = |words: &[&str]| parse_within(&argv(words), usize::MAX);
    let (half, rest) = (long(MAX_PATTERN_BYTES / 2), long(MAX_PATTERN_BYTES / 2 + 1));
    assert!(unbounded(&["-e", &long(MAX_PATTERN_BYTES)]).is_ok());
    assert!(unbounded(&["-e", &long(MAX_PATTERN_BYTES + 1)]).is_err());
    assert!(unbounded(&["-e", &half, "-e", &half]).is_ok());
    assert!(unbounded(&["-e", &half, "-e", &rest]).is_err());
    assert!(unbounded(&[&long(MAX_PATTERN_BYTES + 1)]).is_err());
    assert!(unbounded(&["-g", &long(MAX_GLOB_BYTES), "p"]).is_ok());
    assert!(unbounded(&["-g", &long(MAX_GLOB_BYTES + 1), "p"]).is_err());
    assert!(unbounded(&["-g", &half, "-g", &half, "p"]).is_ok());
    assert!(unbounded(&["-g", &half, "-g", &rest, "p"]).is_err());

    // Encoded argv Σ(len + 1) over every word including `rg`: "rg\0" + pattern + "\0".
    let at_bound = long(MAX_ARGV_BYTES - 4);
    assert_eq!(ok(&[&at_bound]).patterns, strings(&[&at_bound]));
    rejects(&[&long(MAX_ARGV_BYTES - 3)]);
    // The argv bound covers paths and option words too.
    let path = long(MAX_ARGV_BYTES - 3 - 2 - 1);
    assert!(parse(&argv(&["p", &path])).is_ok());
    rejects(&["p", &format!("{}a", path)]);
    rejects(&["-i", &at_bound]);
}

// ── text ──

fn search_row(kind: &str, path: Option<&[u8]>, line_number: u64, line: &[u8]) -> Value {
    let string = |s: &str| Value::String(String::from(s));
    let fields: [(&str, Value); 10] = [
        ("schema", string("cellbox.search.line")),
        ("schema_version", Value::UInt(1)),
        ("kind", string(kind)),
        ("path", path.map_or(Value::Null, |p| Value::Path(p.to_vec()))),
        ("line_number", Value::UInt(line_number)),
        ("absolute_offset", Value::UInt(0)),
        ("line_bytes", Value::Bytes(line.to_vec())),
        ("text", core::str::from_utf8(line).map_or(Value::Null, string)),
        ("terminator", Value::Bytes(b"\n".to_vec())),
        ("submatches", Value::List(Vec::new())),
    ];
    Value::Record(fields.into_iter().map(|(k, v)| (String::from(k), v)).collect())
}

fn with_field(mut row: Value, name: &str, value: Option<Value>) -> Value {
    if let Value::Record(fields) = &mut row {
        let at = fields.iter().position(|(k, _)| k == name).unwrap();
        match value {
            Some(value) => fields[at].1 = value,
            None => {
                fields.remove(at);
            }
        }
    }
    row
}

fn rendered(row: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    render_row(row, &mut out).unwrap();
    out
}

fn line(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    escape_line(bytes, &mut out);
    out
}

fn path(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    escape_path(bytes, &mut out);
    out
}

/// `\xHH` for every UTF-8 byte of `c`.
fn hex_of(c: char) -> Vec<u8> {
    let mut buf = [0u8; 4];
    c.encode_utf8(&mut buf).bytes().flat_map(|b| format!("\\x{:02X}", b).into_bytes()).collect()
}

#[test]
fn rows_render_with_match_and_context_separators() {
    let row = search_row("match", Some(b"/src/main.rs"), 12, b"fn main() {");
    assert_eq!(rendered(&row), b"/src/main.rs:12:fn main() {\n");
    let row = search_row("context", Some(b"/src/main.rs"), 13, b"}");
    assert_eq!(rendered(&row), b"/src/main.rs-13-}\n");
    assert_eq!(rendered(&search_row("match", None, 1, b"x")), b"<stdin>:1:x\n");
    assert_eq!(rendered(&search_row("context", None, 7, b"")), b"<stdin>-7-\n");
    let big = search_row("match", Some(b"/f"), u64::MAX, b"y");
    assert_eq!(rendered(&big), format!("/f:{}:y\n", u64::MAX).as_bytes());
    // `:`/`-` are escaped in the path only.
    let row = search_row("match", Some(b"/a-b:c"), 3, b"x-y:z");
    assert_eq!(rendered(&row), b"/a\\x2Db\\x3Ac:3:x-y:z\n");
    // A path named `<stdin>` keeps its directory prefix; only a Null path is the marker.
    assert_eq!(rendered(&search_row("match", Some(b"/<stdin>"), 1, b"x")), b"/<stdin>:1:x\n");
}

#[test]
fn the_line_comes_from_line_bytes_not_text() {
    let row = search_row("match", Some(b"/f"), 2, b"real\xff");
    let row = with_field(row, "text", Some(Value::String(String::from("decoy"))));
    assert_eq!(rendered(&row), b"/f:2:real\\xFF\n");
}

#[test]
fn rows_append_to_the_output() {
    let mut out = b"prior\n".to_vec();
    render_row(&search_row("match", Some(b"/a"), 1, b"x"), &mut out).unwrap();
    render_row(&search_row("context", Some(b"/a"), 2, b"y"), &mut out).unwrap();
    assert_eq!(out, b"prior\n/a:1:x\n/a-2-y\n");
}

#[test]
fn escaping_matrix() {
    let cases: [(&[u8], &[u8]); 25] = [
        (b"plain text", b"plain text"),
        (b"a\\b", b"a\\\\b"),
        (b"\\n", b"\\\\n"),
        (b"a\nb", b"a\\nb"),
        (b"a\rb", b"a\\rb"),
        (b"crlf\r", b"crlf\\r"),
        (b"a\tb", b"a\\tb"),
        (b"a\0b", b"a\\0b"),
        (b"\x01\x07\x08\x0b\x0c\x1b\x1f", b"\\x01\\x07\\x08\\x0B\\x0C\\x1B\\x1F"),
        (b"\x1b[31mred", b"\\x1B[31mred"),
        (b"del\x7f", b"del\\x7F"),
        // C1 controls as valid UTF-8: every byte of the sequence.
        ("\u{80}\u{85}\u{9b}\u{9f}".as_bytes(), b"\\xC2\\x80\\xC2\\x85\\xC2\\x9B\\xC2\\x9F"),
        // First printable code point after C1 is kept.
        ("\u{a0}".as_bytes(), "\u{a0}".as_bytes()),
        // Invalid UTF-8: lone continuation, invalid lead, overlong, surrogate.
        (b"a\x80b", b"a\\x80b"),
        (b"\xff\xfe", b"\\xFF\\xFE"),
        (b"\xc0\xaf", b"\\xC0\\xAF"),
        (b"\xed\xa0\x80", b"\\xED\\xA0\\x80"),
        // Truncated multi-byte sequences, at the end and before other text.
        (b"ok\xe2\x82", b"ok\\xE2\\x82"),
        (b"ok\xf0\x9f\x99", b"ok\\xF0\\x9F\\x99"),
        (b"\xe2\x82x", b"\\xE2\\x82x"),
        (b"\xc3", b"\\xC3"),
        // `:` and `-` are content in a line.
        (b"a:b-c", b"a:b-c"),
        // Printable non-ASCII is kept.
        ("h\u{e9}llo \u{2713} \u{65e5}\u{672c} \u{1f642}".as_bytes(), "h\u{e9}llo \u{2713} \u{65e5}\u{672c} \u{1f642}".as_bytes()),
        // Valid UTF-8 between invalid bytes is kept.
        (b"\xff\xc3\xa9\xff", b"\\xFF\xc3\xa9\\xFF"),
        (b"", b""),
    ];
    for (input, expected) in cases {
        assert_eq!(line(input), expected, "{:?}", input);
        // Paths share every line escape (no `:`/`-` in these inputs except one).
        if !input.contains(&b':') && !input.contains(&b'-') {
            assert_eq!(path(input), expected, "{:?}", input);
        }
    }
}

#[test]
fn bidi_and_format_controls_are_escaped_per_byte() {
    let escaped = ['\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'];
    for c in escaped {
        let mut input = String::from("a");
        input.push(c);
        input.push('b');
        let mut expected = b"a".to_vec();
        expected.extend(hex_of(c));
        expected.push(b'b');
        assert_eq!(line(input.as_bytes()), expected, "{:?}", c);
        assert_eq!(path(input.as_bytes()), expected, "{:?}", c);
    }
    // Neighbours outside the set are printable text and kept.
    for c in ['\u{061B}', '\u{200D}', '\u{2010}', '\u{2029}', '\u{202F}', '\u{2065}', '\u{206A}'] {
        let mut input = String::new();
        input.push(c);
        assert_eq!(line(input.as_bytes()), input.as_bytes(), "{:?}", c);
    }
    let row = search_row("match", Some("/x\u{202E}y".as_bytes()), 1, "\u{2066}z".as_bytes());
    assert_eq!(rendered(&row), b"/x\\xE2\\x80\\xAEy:1:\\xE2\\x81\\xA6z\n");
}

#[test]
fn path_escapes_separators_and_invalid_bytes() {
    assert_eq!(path(b"/a-b:c"), b"/a\\x2Db\\x3Ac");
    assert_eq!(path(b"/x\\y\nz"), b"/x\\\\y\\nz");
    assert_eq!(path(b"/bad\xff-"), b"/bad\\xFF\\x2D");
    assert_eq!(path("/d\u{e9}j\u{e0}".as_bytes()), "/d\u{e9}j\u{e0}".as_bytes());
    assert_eq!(line(b"/a-b:c"), b"/a-b:c");
}

/// The output is ASCII plus valid UTF-8 with no control byte (ESC in particular) and no
/// bidi/format control character.
fn assert_display_safe(out: &[u8], input: &[u8]) {
    assert!(!out.contains(&0x1b), "ESC for {:?}", input);
    assert!(out.iter().all(|&b| b >= 0x20 && b != 0x7f), "control byte for {:?}", input);
    let text = core::str::from_utf8(out).unwrap_or_else(|_| panic!("invalid UTF-8 for {:?}", input));
    assert!(
        text.chars().all(|c| !c.is_control() && !matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')),
        "unsafe character for {:?}",
        input
    );
}

#[test]
fn no_output_carries_control_bytes() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let input = [a, b, 0x1b];
            assert_display_safe(&line(&input), &input);
            assert_display_safe(&path(&input), &input);
        }
    }
    let every: Vec<u8> = (0..=255u8).chain((0..=255u8).rev()).collect();
    assert_display_safe(&line(&every), &every);
    assert_display_safe(&path(&every), &every);
    let mut rows = Vec::new();
    for c in ["\u{80}", "\u{9f}", "\u{202e}", "\u{1b}", "\u{7f}"] {
        rows.push(search_row("match", Some(c.as_bytes()), 1, c.as_bytes()));
        rows.push(search_row("context", None, 2, &every));
    }
    for row in &rows {
        let out = rendered(row);
        let (body, last) = out.split_at(out.len() - 1);
        assert_eq!(last, b"\n");
        assert_display_safe(body, &out);
    }
}

#[test]
fn rows_that_are_not_search_lines_reject_and_write_nothing() {
    let good = || search_row("match", Some(b"/f"), 1, b"x");
    let string = |s: &str| Value::String(String::from(s));
    let bad = [
        Value::Null,
        string("cellbox.search.line"),
        Value::Bytes(b"/f:1:x".to_vec()),
        Value::List(vec![good()]),
        Value::Record(Vec::new()),
        with_field(good(), "schema", None),
        with_field(good(), "schema", Some(string("cellbox.ps.process"))),
        with_field(good(), "schema", Some(Value::Bytes(b"cellbox.search.line".to_vec()))),
        with_field(good(), "schema_version", None),
        with_field(good(), "schema_version", Some(Value::UInt(2))),
        with_field(good(), "schema_version", Some(Value::Int(1))),
        with_field(good(), "kind", None),
        with_field(good(), "kind", Some(string("Match"))),
        with_field(good(), "kind", Some(string("other"))),
        with_field(good(), "path", None),
        with_field(good(), "path", Some(string("/f"))),
        with_field(good(), "path", Some(Value::Bytes(b"/f".to_vec()))),
        with_field(good(), "line_number", None),
        with_field(good(), "line_number", Some(Value::Int(1))),
        with_field(good(), "line_number", Some(string("1"))),
        with_field(good(), "line_bytes", None),
        with_field(good(), "line_bytes", Some(string("x"))),
        with_field(good(), "line_bytes", Some(Value::Null)),
    ];
    for row in &bad {
        let mut out = b"kept".to_vec();
        assert!(render_row(row, &mut out).is_err(), "{:?}", row);
        assert_eq!(out, b"kept", "{:?}", row);
    }
    assert_eq!(rendered(&good()), b"/f:1:x\n");
}

// ── source: search over an in-memory tree ──

use super::source::{Limits, StdinSource, Summary, TreeSource};
use super::{build_matcher, build_searcher, build_walk, StdinInput, RG_PROFILE};
use crate::typed::{ByteSource, StreamStop};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use core::cell::{Cell, RefCell};
use text_engine::pipeline::{InterruptLatch, Item, RowSource};
use vigrep::io;
use vigrep::walk::{DirEntry, EntryKind, FileState, Handle, PageInfo, ViSearchTree};

/// Counters a test reads after the tree moved into the source.
#[derive(Default)]
struct Counts {
    open_dirs: Cell<usize>,
    open_files: Cell<usize>,
    peak_dirs: Cell<usize>,
    peak_files: Cell<usize>,
    /// Every tree request (`close` excluded).
    ops: Cell<usize>,
    reads: Cell<usize>,
}

/// An in-memory tree: a path's children are the nodes whose parent it is.
#[derive(Default)]
struct Tree {
    nodes: BTreeMap<Vec<u8>, (EntryKind, Vec<u8>)>,
    denied: BTreeSet<Vec<u8>>,
    /// `file_state` reports `modified` once this file saw this many reads.
    modified_after: BTreeMap<Vec<u8>, usize>,
    chunk: usize,
    counts: Rc<Counts>,
}

struct Dir {
    path: Vec<u8>,
    /// Entries already listed.
    pos: usize,
}

struct File {
    path: Vec<u8>,
    reads: usize,
}

fn parent_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) => b"/",
        Some(i) => &path[..i],
        None => b"",
    }
}

impl Tree {
    fn new() -> Tree {
        let mut tree = Tree { chunk: 4000, ..Tree::default() };
        tree.nodes.insert(b"/".to_vec(), (EntryKind::Dir, Vec::new()));
        tree
    }

    fn add(&mut self, path: &str, kind: EntryKind, contents: &[u8]) -> &mut Tree {
        let path = path.as_bytes().to_vec();
        let mut parent = parent_of(&path).to_vec();
        while !self.nodes.contains_key(&parent) {
            let next = parent_of(&parent).to_vec();
            self.nodes.insert(parent.clone(), (EntryKind::Dir, Vec::new()));
            parent = next;
        }
        self.nodes.insert(path, (kind, contents.to_vec()));
        self
    }

    fn file(&mut self, path: &str, contents: &[u8]) -> &mut Tree {
        self.add(path, EntryKind::File, contents)
    }

    fn op(&self) {
        self.counts.ops.set(self.counts.ops.get() + 1);
    }
}

impl ViSearchTree for Tree {
    type Dir = Dir;
    type File = File;

    fn open_dir(&mut self, path: &[u8]) -> Result<Dir, io::Error> {
        self.op();
        if self.denied.contains(path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match self.nodes.get(path) {
            None => Err(io::Error::new(io::ErrorKind::NotFound, "not found or I/O error")),
            Some((EntryKind::Dir, _)) => {
                let c = &self.counts;
                c.open_dirs.set(c.open_dirs.get() + 1);
                c.peak_dirs.set(c.peak_dirs.get().max(c.open_dirs.get()));
                Ok(Dir { path: path.to_vec(), pos: 0 })
            }
            Some(_) => Err(io::ErrorKind::NotADirectory.into()),
        }
    }

    fn next_page(&mut self, dir: &mut Dir, entries: &mut Vec<DirEntry>) -> Result<PageInfo, io::Error> {
        self.op();
        // Pages like the VFS: at most 4,000 packed bytes (2-byte header + name).
        let children = self
            .nodes
            .iter()
            .filter(|(path, _)| path.as_slice() != b"/" && parent_of(path) == dir.path.as_slice());
        let mut packed = 0;
        let mut complete = true;
        for (path, (kind, _)) in children.skip(dir.pos) {
            let name = path[path.iter().rposition(|&b| b == b'/').unwrap() + 1..].to_vec();
            if packed + 2 + name.len() > 4000 {
                complete = false;
                break;
            }
            packed += 2 + name.len();
            entries.push(DirEntry { kind: *kind, name });
            dir.pos += 1;
        }
        Ok(PageInfo { complete, changed: false })
    }

    fn open_file(&mut self, path: &[u8]) -> Result<File, io::Error> {
        self.op();
        if self.denied.contains(path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match self.nodes.get(path) {
            Some((EntryKind::File, _)) => {
                let c = &self.counts;
                c.open_files.set(c.open_files.get() + 1);
                c.peak_files.set(c.peak_files.get().max(c.open_files.get()));
                Ok(File { path: path.to_vec(), reads: 0 })
            }
            _ => Err(io::Error::new(io::ErrorKind::NotFound, "not found or I/O error")),
        }
    }

    fn read_chunk(&mut self, file: &mut File, offset: u64, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.op();
        self.counts.reads.set(self.counts.reads.get() + 1);
        file.reads += 1;
        let contents = &self.nodes[&file.path].1;
        let start = (offset as usize).min(contents.len());
        let n = buf.len().min(self.chunk).min(contents.len() - start);
        buf[..n].copy_from_slice(&contents[start..start + n]);
        Ok(n)
    }

    fn file_state(&mut self, file: &mut File) -> Result<FileState, io::Error> {
        self.op();
        let modified = self.modified_after.get(&file.path).is_some_and(|&n| file.reads >= n);
        Ok(FileState { size: self.nodes[&file.path].1.len() as u64, modified })
    }

    fn close(&mut self, handle: Handle<Dir, File>) {
        let c = &self.counts;
        match handle {
            Handle::Dir(_) => c.open_dirs.set(c.open_dirs.get() - 1),
            Handle::File(_) => c.open_files.set(c.open_files.get() - 1),
        }
    }
}

type Reports = Rc<RefCell<Vec<String>>>;

fn args_for(words: &[&str]) -> RgArgs {
    parse(&argv(words)).unwrap_or_else(|e| panic!("{:?}: {:?}", words, e))
}

/// A tree-mode source over `tree` for `args` (paths are absolute in tests).
fn tree_source(tree: Tree, args: &RgArgs, latch: &Rc<InterruptLatch>) -> (TreeSource<Tree>, Reports) {
    tree_source_with(tree, args, latch, RG_PROFILE.limits)
}

fn tree_source_with(tree: Tree, args: &RgArgs, latch: &Rc<InterruptLatch>, limits: Limits) -> (TreeSource<Tree>, Reports) {
    let matcher = build_matcher(args, &RG_PROFILE).expect("matcher");
    let roots: Vec<Vec<u8>> = args.paths.iter().map(|p| p.as_bytes().to_vec()).collect();
    let walker = build_walk(args, &roots, b"/", &RG_PROFILE).expect("walk").build();
    let reports: Reports = Rc::new(RefCell::new(Vec::new()));
    let sink = reports.clone();
    let source = TreeSource::new(
        tree,
        walker,
        build_searcher(args, &RG_PROFILE),
        matcher,
        args.quiet,
        limits,
        latch.clone(),
        Box::new(move |line: &str| sink.borrow_mut().push(String::from(line))),
    );
    (source, reports)
}

fn drain(source: &mut dyn RowSource) -> Vec<Value> {
    let mut rows = Vec::new();
    while let Some(item) = source.next_item().expect("pull") {
        let Item::Row(row) = item else { panic!("not a row: {:?}", item) };
        rows.push(row);
    }
    rows
}

fn get<'a>(row: &'a Value, name: &str) -> &'a Value {
    let Value::Record(fields) = row else { panic!("not a record") };
    &fields.iter().find(|(key, _)| key == name).unwrap_or_else(|| panic!("no {}", name)).1
}

fn uint(row: &Value, name: &str) -> u64 {
    match get(row, name) {
        Value::UInt(n) => *n,
        other => panic!("{} is {:?}", name, other),
    }
}

/// `PATH:LINE:TEXT` (or `-` for context) of each row, through the renderer.
fn lines(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let mut out = Vec::new();
            render_row(row, &mut out).unwrap();
            String::from_utf8(out).unwrap().trim_end_matches('\n').to_string()
        })
        .collect()
}

fn run(tree: Tree, words: &[&str]) -> (Vec<String>, Option<i32>, Summary, Vec<String>, Rc<Counts>) {
    let counts = tree.counts.clone();
    let (mut source, reports) = tree_source(tree, &args_for(words), &InterruptLatch::inert());
    let rows = drain(&mut source);
    let summary = source.summary();
    let completion = source.completion();
    drop(source);
    let reports = reports.borrow().clone();
    (lines(&rows), completion, summary, reports, counts)
}

fn released(counts: &Counts) {
    assert_eq!(counts.open_dirs.get(), 0, "directory handles leaked");
    assert_eq!(counts.open_files.get(), 0, "file handles leaked");
}

#[test]
fn records_have_the_contract_fields_in_order() {
    let mut tree = Tree::new();
    tree.file("/r/a", b"one foo\r\nfoo foo\n\xff foo\nlast foo");
    let counts = tree.counts.clone();
    let (mut source, _) = tree_source(tree, &args_for(&["-B", "1", "foo", "/r/a"]), &InterruptLatch::inert());
    let rows = drain(&mut source);
    assert_eq!(rows.len(), 4);
    let Value::Record(fields) = &rows[0] else { panic!() };
    let names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "schema", "schema_version", "kind", "path", "line_number", "absolute_offset", "line_bytes", "text",
            "terminator", "submatches"
        ]
    );
    assert_eq!(get(&rows[0], "schema"), &Value::String(String::from("cellbox.search.line")));
    assert_eq!(get(&rows[0], "schema_version"), &Value::UInt(1));
    assert_eq!(get(&rows[0], "path"), &Value::Path(b"/r/a".to_vec()));
    // CR is content; the LF is the terminator.
    assert_eq!(get(&rows[0], "line_bytes"), &Value::Bytes(b"one foo\r".to_vec()));
    assert_eq!(get(&rows[0], "text"), &Value::String(String::from("one foo\r")));
    assert_eq!(get(&rows[0], "terminator"), &Value::Bytes(b"\n".to_vec()));
    // Two submatches, ordered, non-overlapping, relative to line_bytes.
    let sub = |s: u64, e: u64| Value::Record(vec![(String::from("start"), Value::UInt(s)), (String::from("end"), Value::UInt(e))]);
    assert_eq!(get(&rows[1], "submatches"), &Value::List(vec![sub(0, 3), sub(4, 7)]));
    assert_eq!(uint(&rows[1], "absolute_offset"), 9);
    // Invalid UTF-8: text is Null, bytes exact.
    assert_eq!(get(&rows[2], "text"), &Value::Null);
    assert_eq!(get(&rows[2], "line_bytes"), &Value::Bytes(b"\xff foo".to_vec()));
    // Final line without LF: empty terminator.
    assert_eq!(get(&rows[3], "terminator"), &Value::Bytes(Vec::new()));
    assert_eq!(uint(&rows[3], "line_number"), 4);
    assert_eq!(source.completion(), Some(0));
    drop(source);
    released(&counts);
}

#[test]
fn context_rows_have_no_submatches_and_render_with_dashes() {
    let mut tree = Tree::new();
    tree.file("/r/a", b"x\nctx\nfoo\nafter\ny\n");
    let (lines, status, ..) = run(tree, &["-C", "1", "foo", "/r/a"]);
    assert_eq!(lines, ["/r/a-2-ctx", "/r/a:3:foo", "/r/a-4-after"]);
    assert_eq!(status, Some(0));
}

#[test]
fn offsets_are_exact_over_a_bom_and_a_multi_chunk_file() {
    let mut big = b"\xEF\xBB\xBFstart\n".to_vec();
    let mut expected = Vec::new();
    for i in 0..9000u32 {
        if i % 1000 == 7 {
            expected.push((i as u64 + 2, big.len() as u64));
            big.extend_from_slice(format!("needle {}\n", i).as_bytes());
        } else {
            big.extend_from_slice(b"haystack line\n");
        }
    }
    assert!(big.len() > 64 * 1024);
    let mut tree = Tree::new();
    tree.file("/r/big", &big);
    let counts = tree.counts.clone();
    let (mut source, _) = tree_source(tree, &args_for(&["needle", "/r"]), &InterruptLatch::inert());
    let rows = drain(&mut source);
    let got: Vec<(u64, u64)> = rows.iter().map(|r| (uint(r, "line_number"), uint(r, "absolute_offset"))).collect();
    assert_eq!(got, expected);
    let summary = source.summary();
    assert_eq!(summary.lines_scanned, 9001);
    assert_eq!(summary.matching_lines, 9);
    assert_eq!(summary.files_scanned, 1);
    // The 4,000-byte reads covered the file at least twice (preflight + search).
    assert!(counts.reads.get() as u64 >= 2 * big.len() as u64 / 4000);
}

#[test]
fn statuses_follow_the_contract() {
    // 0: a match.
    let mut tree = Tree::new();
    tree.file("/r/a", b"foo\n");
    assert_eq!(run(tree, &["foo", "/r"]).1, Some(0));
    // 1: complete, nothing matched (empty tree, all-binary tree).
    let mut tree = Tree::new();
    tree.add("/r/d", EntryKind::Dir, b"");
    assert_eq!(run(tree, &["foo", "/r"]).1, Some(1));
    let mut tree = Tree::new();
    tree.file("/r/bin", b"foo\0\n");
    let (lines, status, summary, reports, _) = run(tree, &["foo", "/r"]);
    assert!(lines.is_empty());
    assert_eq!(status, Some(1));
    assert_eq!(summary.files_binary_skipped, 1);
    assert!(reports.is_empty(), "{:?}", reports);
    // 2: a missing root.
    let tree = Tree::new();
    let (_, status, summary, reports, _) = run(tree, &["foo", "/nope"]);
    assert_eq!(status, Some(2));
    assert_eq!(summary.errors, 1);
    assert!(reports[0].starts_with("rg: /nope: "), "{:?}", reports);
}

#[test]
fn quiet_counts_without_rows_and_a_later_error_still_fails() {
    let mut tree = Tree::new();
    tree.file("/r/a", b"foo\nfoo\n").file("/r/b/x", b"foo\n");
    tree.denied.insert(b"/r/b".to_vec());
    let (lines, status, summary, reports, counts) = run(tree, &["-q", "foo", "/r"]);
    assert!(lines.is_empty());
    assert_eq!(summary.matching_lines, 2);
    assert_eq!(status, Some(2));
    assert_eq!(reports.len(), 2, "one entry line and the summary: {:?}", reports);
    released(&counts);
}

#[test]
fn a_denied_directory_is_reported_and_the_walk_continues() {
    let mut tree = Tree::new();
    tree.file("/r/a/x", b"foo\n").file("/r/b/y", b"foo\n").file("/r/c/z", b"foo\n");
    tree.denied.insert(b"/r/b".to_vec());
    let (lines, status, summary, reports, counts) = run(tree, &["foo", "/r"]);
    assert_eq!(lines, ["/r/a/x:1:foo", "/r/c/z:1:foo"]);
    assert_eq!(status, Some(2));
    assert_eq!(reports[0], "rg: /r/b: permission denied");
    assert!(reports[1].contains("errors=1") && reports[1].contains("complete=false"), "{:?}", reports);
    assert_eq!(summary.errors, 1);
    released(&counts);
}

#[test]
fn mounts_are_counted_without_changing_the_status_and_other_kinds_fail() {
    let mut tree = Tree::new();
    tree.file("/r/a", b"foo\n").add("/r/m", EntryKind::Mount, b"");
    let (lines, status, summary, reports, _) = run(tree, &["foo", "/r"]);
    assert_eq!(lines, ["/r/a:1:foo"]);
    assert_eq!(status, Some(0));
    assert_eq!(summary.mounts_skipped, 1);
    assert_eq!(reports.len(), 1, "only the summary: {:?}", reports);
    assert!(reports[0].contains("mounts_skipped=1"));

    let mut tree = Tree::new();
    tree.file("/r/a", b"foo\n").add("/r/o", EntryKind::Other, b"");
    let (lines, status, ..) = run(tree, &["foo", "/r"]);
    assert_eq!(lines, ["/r/a:1:foo"]);
    assert_eq!(status, Some(2));
}

#[test]
fn a_row_over_the_record_bound_fails_its_file_and_the_walk_continues() {
    // `line_bytes` doubles in hex and `text` repeats it: a 30,000-byte line
    // encodes above 65,536 bytes.
    let mut long = vec![b'a'; 30_000];
    long.extend_from_slice(b" foo\n");
    let mut tree = Tree::new();
    tree.file("/r/a", &long).file("/r/b", b"foo\n");
    let (lines, status, summary, reports, counts) = run(tree, &["foo", "/r"]);
    assert_eq!(lines, ["/r/b:1:foo"]);
    assert_eq!(status, Some(2));
    assert_eq!(summary.errors, 1);
    assert!(reports[0].starts_with("rg: /r/a: "), "{:?}", reports);
    released(&counts);
}

#[test]
fn diagnostics_escape_paths_and_reasons() {
    let mut tree = Tree::new();
    tree.add("/r/a\x1b[2J-b", EntryKind::Other, b"");
    let (_, status, _, reports, _) = run(tree, &["foo", "/r"]);
    assert_eq!(status, Some(2));
    assert!(reports[0].starts_with("rg: /r/a\\x1B[2J\\x2Db: "), "{:?}", reports);
    assert!(reports.iter().all(|line| !line.contains('\x1b')));
}

#[test]
fn take_closes_early_releasing_every_handle_with_no_further_reads() {
    let mut tree = Tree::new();
    tree.chunk = 64;
    tree.file("/r/a", "foo\n".repeat(4000).as_bytes()).file("/r/b", b"foo\n");
    let counts = tree.counts.clone();
    let (mut source, _) = tree_source(tree, &args_for(&["foo", "/r"]), &InterruptLatch::inert());
    assert!(source.next_item().unwrap().is_some());
    assert_eq!(counts.open_files.get(), 1, "the paused file stays open between pulls");
    source.close();
    released(&counts);
    let ops = counts.ops.get();
    assert!(source.next_item().unwrap().is_none());
    source.close();
    assert_eq!(counts.ops.get(), ops, "nothing after close");
    assert_eq!(source.completion(), Some(0));
    assert!(counts.peak_files.get() <= 1 && counts.peak_dirs.get() <= 1);
}

#[test]
fn close_after_an_error_reports_status_two() {
    let mut tree = Tree::new();
    tree.file("/r/b/x", b"foo\n").file("/r/c", "foo\n".repeat(100).as_bytes());
    tree.denied.insert(b"/r/b".to_vec());
    let (mut source, _) = tree_source(tree, &args_for(&["foo", "/r"]), &InterruptLatch::inert());
    assert!(source.next_item().unwrap().is_some());
    source.close();
    assert_eq!(source.completion(), Some(2));
}

#[test]
fn an_interrupt_mid_file_stops_reads_and_releases_handles() {
    let mut tree = Tree::new();
    tree.chunk = 16;
    tree.file("/r/a", "foo\n".repeat(2000).as_bytes());
    let counts = tree.counts.clone();
    let fire = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0usize));
    let (f, p) = (fire.clone(), polls.clone());
    let latch = InterruptLatch::new(Box::new(move || {
        p.set(p.get() + 1);
        f.get()
    }));
    let (mut source, _) = tree_source(tree, &args_for(&["foo", "/r"]), &latch);
    for _ in 0..3 {
        assert!(source.next_item().unwrap().is_some());
    }
    assert_eq!(counts.open_files.get(), 1);
    fire.set(true);
    // At most 16 checks pass before the latch polls; the queue drains first.
    let mut extra = 0;
    while source.next_item().unwrap().is_some() {
        extra += 1;
        assert!(extra < 2000, "never interrupted");
    }
    assert!(latch.is_set());
    released(&counts);
    assert_eq!(source.completion(), None);
    let ops = counts.ops.get();
    assert!(source.next_item().unwrap().is_none());
    source.close();
    assert_eq!(counts.ops.get(), ops);
    assert_eq!(source.completion(), None);
}

#[test]
fn the_queue_never_exceeds_its_cap_plus_one_row() {
    let mut contents = Vec::new();
    for i in 0..500u32 {
        contents.extend_from_slice(format!("foo {}\n", "x".repeat((i as usize * 37) % 9000)).as_bytes());
    }
    let mut tree = Tree::new();
    tree.file("/r/a", &contents);
    let (mut source, _) = tree_source(tree, &args_for(&["-C", "2", "foo", "/r"]), &InterruptLatch::inert());
    let limits = RG_PROFILE.limits;
    let mut rows = 0;
    // Every pull that advanced the search sees a fresh queue: count rows between
    // two reads of the tree.
    while let Some(item) = source.next_item().unwrap() {
        let Item::Row(row) = item else { panic!() };
        assert!(row.payload_bytes() < 65_536);
        rows += 1;
        let (queued, bytes) = source.queued();
        assert!(queued < limits.queue_rows, "{} queued", queued);
        assert!(bytes < limits.queue_bytes + 65_536, "{} bytes queued", bytes);
    }
    assert_eq!(rows, 500);
}

#[test]
fn stdin_bytes_search_and_statuses() {
    let args = args_for(&["-a", "b", "-"]);
    let mut source = StdinSource::new(
        StdinInput::Bytes(b"a:b\nzz\nb\0b\n".to_vec()),
        build_searcher(&args, &RG_PROFILE),
        build_matcher(&args, &RG_PROFILE).unwrap(),
        false,
        RG_PROFILE.limits,
        RG_PROFILE.stdin_window,
        InterruptLatch::inert(),
        Box::new(|_: &str| {}),
    );
    let rows = drain(&mut source);
    assert_eq!(lines(&rows), ["<stdin>:1:a:b", "<stdin>:3:b\\0b"]);
    assert_eq!(get(&rows[0], "path"), &Value::Null);
    assert_eq!(source.completion(), Some(0));
    assert_eq!(source.summary().lines_scanned, 3);

    let args = args_for(&["-a", "zz", "-"]);
    let mut source = StdinSource::new(
        StdinInput::Bytes(b"a:b\n".to_vec()),
        build_searcher(&args, &RG_PROFILE),
        build_matcher(&args, &RG_PROFILE).unwrap(),
        false,
        RG_PROFILE.limits,
        RG_PROFILE.stdin_window,
        InterruptLatch::inert(),
        Box::new(|_: &str| {}),
    );
    assert!(drain(&mut source).is_empty());
    assert_eq!(source.completion(), Some(1));
}

/// A forward-only byte stream (an external head) handing out `chunk` bytes per read.
struct Stream {
    bytes: Vec<u8>,
    at: usize,
    chunk: usize,
    closed: Rc<Cell<Option<bool>>>,
}

impl ByteSource for Stream {
    fn read_chunk(&mut self, out: &mut [u8]) -> Result<Option<usize>, StreamStop> {
        if self.at == self.bytes.len() {
            return Ok(None);
        }
        let n = out.len().min(self.chunk).min(self.bytes.len() - self.at);
        out[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(Some(n))
    }

    fn close(&mut self, drained: bool) {
        self.closed.set(Some(drained));
    }
}

fn stream_source(bytes: Vec<u8>, words: &[&str], window: usize) -> (StdinSource, Rc<Cell<Option<bool>>>, Reports) {
    let args = args_for(words);
    let closed = Rc::new(Cell::new(None));
    let reports: Reports = Rc::new(RefCell::new(Vec::new()));
    let sink = reports.clone();
    let source = StdinSource::new(
        StdinInput::Stream(Box::new(Stream { bytes, at: 0, chunk: 512, closed: closed.clone() })),
        build_searcher(&args, &RG_PROFILE),
        build_matcher(&args, &RG_PROFILE).unwrap(),
        false,
        RG_PROFILE.limits,
        window,
        InterruptLatch::inert(),
        Box::new(move |line: &str| sink.borrow_mut().push(String::from(line))),
    );
    (source, closed, reports)
}

#[test]
fn a_streamed_stdin_resumes_inside_its_window() {
    let text = "foo\n".repeat(5000).into_bytes();
    let (mut source, closed, reports) = stream_source(text, &["-a", "foo", "-"], RG_PROFILE.stdin_window);
    let rows = drain(&mut source);
    assert_eq!(rows.len(), 5000);
    assert_eq!(uint(&rows[4999], "line_number"), 5000);
    assert_eq!(uint(&rows[4999], "absolute_offset"), 4999 * 4);
    assert_eq!(source.completion(), Some(0));
    assert_eq!(closed.get(), Some(true));
    assert!(reports.borrow().is_empty());
}

#[test]
fn a_streamed_stdin_resume_outside_its_window_fails_with_two() {
    // Dense matches pause the search every 8 rows; with a 16-byte window the
    // restart point is already gone from the stream.
    let text = "foo\n".repeat(5000).into_bytes();
    let (mut source, closed, reports) = stream_source(text, &["-a", "foo", "-"], 16);
    let rows = drain(&mut source);
    assert!(rows.len() < 5000);
    assert_eq!(source.completion(), Some(2));
    assert!(reports.borrow()[0].starts_with("rg: <stdin>: "), "{:?}", reports.borrow());
    assert!(closed.get().is_some(), "the stream is released");
}

#[test]
fn a_take_on_a_stream_stops_the_producer() {
    let text = "foo\n".repeat(5000).into_bytes();
    let (mut source, closed, _) = stream_source(text, &["-a", "foo", "-"], RG_PROFILE.stdin_window);
    assert!(source.next_item().unwrap().is_some());
    source.close();
    assert_eq!(closed.get(), Some(false), "not drained: the producer is stopped");
    assert_eq!(source.completion(), Some(0));
}

// ── heap: the cellbox arena (DV7-10, D76-3 M2) ──

mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    /// Counts the live heap bytes of the current thread (tests run in parallel
    /// threads; each measures only its own allocations).
    pub struct Counting;

    std::thread_local! {
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    fn add(delta: isize) {
        let _ = LIVE.try_with(|live| {
            let now = live.get() + delta;
            live.set(now);
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
        });
    }

    // SAFETY: forwards every call to the system allocator unchanged; only counts.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() {
                add(layout.size() as isize);
            }
            ptr
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) };
            add(-(layout.size() as isize));
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let new = unsafe { System.realloc(ptr, layout, new_size) };
            if !new.is_null() {
                add(new_size as isize - layout.size() as isize);
            }
            new
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    /// Peak live bytes above the current level while `run` executes.
    pub fn peak_of(run: impl FnOnce()) -> usize {
        let base = LIVE.with(Cell::get);
        PEAK.with(|peak| peak.set(base));
        run();
        (PEAK.with(Cell::get) - base).max(0) as usize
    }
}

/// Peak heap of one whole search (matcher and walker construction included),
/// pulling rows the way a streaming sink does (one at a time, dropped).
fn search_peak(tree: Tree, args: &RgArgs) -> (usize, Summary) {
    let mut summary = Summary::default();
    let peak = heap::peak_of(|| {
        let (mut source, _) = tree_source(tree, args, &InterruptLatch::inert());
        while let Some(item) = source.next_item().unwrap() {
            drop(item);
        }
        summary = source.summary();
    });
    (peak, summary)
}

/// The D76-3 M2 target: an `rg` search leaves 1 MiB of the cellbox arena (the
/// size the whole shell ran in before M2) for the shell itself.
const HEAP_TARGET: usize = crate::HEAP_ARENA_BYTES - 1024 * 1024;

#[test]
fn heap_peak_with_32_unicode_case_insensitive_patterns() {
    let mut contents = Vec::new();
    for i in 0..3000u32 {
        contents.extend_from_slice(format!("Ünïcödé wörd{} ΣΊΣΥΦΟΣ straße line {}\n", i % 40, i).as_bytes());
    }
    let mut tree = Tree::new();
    tree.file("/r/a", &contents);
    let mut args = args_for(&["-i", "x", "/r"]);
    args.patterns = (0..32).map(|i| format!("\\w+ wörd{}\\b", i)).collect();
    let (peak, summary) = search_peak(tree, &args);
    assert!(summary.matching_lines > 0 && summary.errors == 0, "{:?}", summary);
    assert!(peak <= HEAP_TARGET, "peak {} bytes", peak);
}

#[test]
fn heap_peak_with_a_full_directory_and_maximal_ignore_files() {
    let mut tree = Tree::new();
    // 4,096 entries in one directory (the per-directory limit), 24-byte names.
    for i in 0..4095u32 {
        tree.file(&format!("/r/wide/entry-name-{:06}.txt", i), b"foo\n");
    }
    // Ignore files at two levels: 16 KiB, at most 256 rules each.
    let rule = |i: u32| format!("ignored-pattern-number-{:05}-with-a-fairly-long-name-*.bak\n", i);
    let mut ignore = Vec::new();
    let mut i = 0;
    while ignore.len() + rule(i).len() <= 16 * 1024 && i < 256 {
        ignore.extend_from_slice(rule(i).as_bytes());
        i += 1;
    }
    tree.file("/r/.ignore", &ignore).file("/r/wide/.gitignore", &ignore);
    let (peak, summary) = search_peak(tree, &args_for(&["foo", "/r"]));
    assert_eq!(summary.errors, 0, "{:?}", summary);
    assert_eq!(summary.matching_lines, 4095);
    assert!(peak <= HEAP_TARGET, "peak {} bytes", peak);
}

#[test]
fn heap_peak_with_long_dense_lines_and_full_context() {
    // Lines just under the 65,536-byte line limit, and dense short matches with
    // 20 lines of context on both sides.
    let mut contents = Vec::new();
    for _ in 0..4 {
        contents.extend_from_slice(&vec![b'q'; 60_000]);
        contents.extend_from_slice(b" foo\n");
    }
    for i in 0..2000u32 {
        contents.extend_from_slice(format!("{} foo foo foo foo foo foo foo\n", i).as_bytes());
    }
    let mut tree = Tree::new();
    tree.file("/r/a", &contents);
    let args = args_for(&["-C", "20", "foo", "/r"]);
    let (peak, summary) = search_peak(tree, &args);
    // The 60,000-byte lines are over the record bound (one file error), so search
    // the dense part alone too.
    assert_eq!(summary.errors, 1, "{:?}", summary);
    assert!(peak <= HEAP_TARGET, "peak {} bytes", peak);

    let mut dense = Vec::new();
    for i in 0..4000u32 {
        dense.extend_from_slice(format!("{} {}\n", i, "foo ".repeat(200)).as_bytes());
    }
    let mut tree = Tree::new();
    tree.file("/r/a", &dense);
    let (peak, summary) = search_peak(tree, &args);
    assert_eq!(summary.errors, 0, "{:?}", summary);
    assert_eq!(summary.matching_lines, 4000);
    assert!(peak <= HEAP_TARGET, "peak {} bytes", peak);
}

// ── admission and status mapping ──

use crate::typed::{
    admit, background_refusal, observe_completion, run_rows_within, Admission, DefaultSink, Head, Redir, RedirKind,
    StageIn, TypedPlan, SEARCH_COLLECT_BOUND,
};
use text_engine::pipeline::{Category, Stage};

fn stage(words: &[&str]) -> StageIn {
    StageIn { argv: words.iter().map(|w| String::from(*w)).collect(), redirects: Vec::new() }
}

fn stage_redirect(words: &[&str], kind: RedirKind, path: &str) -> StageIn {
    StageIn { redirects: vec![Redir { kind, path: String::from(path) }], ..stage(words) }
}

fn plan(stages: &[StageIn]) -> TypedPlan {
    match admit(stages, true) {
        Admission::Typed(plan) => plan,
        other => panic!("{:?} not typed: {:?}", stages.iter().map(|s| &s.argv).collect::<Vec<_>>(), other),
    }
}

fn refused(stages: &[StageIn]) -> String {
    match admit(stages, true) {
        Admission::Reject(reason) => reason,
        other => panic!("admitted: {:?}", other),
    }
}

#[test]
fn rg_alone_is_a_producer_with_the_search_text_renderer() {
    let p = plan(&[stage(&["rg", "x"])]);
    assert_eq!(p.head, Head::Producer);
    assert!(p.search.is_some() && p.search_text && p.search_status);
    assert_eq!(p.producer_argv, strings(&["rg", "x"]));

    let p = plan(&[stage(&["rg", "x"]), stage(&["take", "1"])]);
    assert!(p.search_text && !p.search_status);
    let p = plan(&[stage(&["rg", "x"]), stage(&["where", "line_number", "==", "1"])]);
    assert!(p.search_text && !p.search_status);
    let p = plan(&[stage(&["rg", "x"]), stage(&["to", "jsonl", "--typed"])]);
    assert!(!p.search_text && p.search_status);
    let p = plan(&[stage(&["rg", "x"]), stage(&["select", "path"])]);
    assert!(!p.search_text && !p.search_status, "select reshapes rows: the table applies");
}

#[test]
fn text_display_comes_from_the_parser_and_text_words_survive() {
    let p = plan(&[stage(&["rg", "--text", "x"])]);
    assert!(p.text_display);
    let p = plan(&[stage(&["rg", "-e", "--text", "x"])]);
    assert!(!p.text_display);
    assert_eq!(p.producer_argv, strings(&["rg", "-e", "--text", "x"]));
    assert_eq!(p.search.unwrap().patterns, strings(&["--text"]));
    let p = plan(&[stage(&["rg", "x", "--", "--text"])]);
    assert!(!p.text_display);
    assert_eq!(p.search.unwrap().paths, strings(&["--text"]));
}

#[test]
fn byte_heads_feed_rg_stdin() {
    let p = plan(&[stage(&["cat", "/f"]), stage(&["rg", "-a", "x", "-"])]);
    assert_eq!(p.head, Head::Cat(String::from("/f")));
    assert_eq!(p.search_index, 1);
    assert!(p.search.as_ref().unwrap().stdin && p.stages.is_empty());
    let p = plan(&[stage(&["echo", "hi"]), stage(&["rg", "-a", "x", "-"]), stage(&["take", "1"])]);
    assert_eq!(p.head, Head::Capture);
    assert_eq!(p.stages, [Stage::Take { n: 1, frame: false }]);
    let p = plan(&[stage_redirect(&["rg", "-a", "x", "-"], RedirKind::In, "/f")]);
    assert_eq!(p.head, Head::FromFile(String::from("/f")));
    assert_eq!(p.search_index, 0);
    let p = plan(&[stage(&["/bin/x"]), stage(&["rg", "-a", "x", "-"])]);
    assert_eq!(p.head, Head::External { count: 1 });
    assert_eq!(p.search_index, 1);
}

#[test]
fn rg_rows_may_feed_byte_consumers_without_to() {
    let p = plan(&[stage(&["rg", "x"]), stage(&["wc", "-l"])]);
    assert_eq!(p.typed_cmds, 1);
    let p = plan(&[stage(&["rg", "--text", "x"]), stage(&["/bin/cat"])]);
    assert_eq!(p.tail_externals, 1);
    assert!(p.text_display && p.search_text);
    // Other producers still need an explicit `to`.
    refused(&[stage(&["ps", "--records"]), stage(&["wc"])]);
}

#[test]
fn rg_admission_refusals_happen_before_any_effect() {
    for words in [
        &["rg", "-P", "x", "/tmp"][..],
        &["rg", "-iw", "x"],
        &["rg", "-q", "-C1", "x"],
        &["rg", "x", "-"],
        &["rg"],
    ] {
        let reason = refused(&[stage(words)]);
        assert!(reason.starts_with("rg: "), "{:?}: {}", words, reason);
    }
    refused(&[stage(&["echo", "hi"]), stage(&["rg", "hi", "-"])]);
    refused(&[stage(&["rg", "-a", "x", "-"])]);
    refused(&[stage(&["ps", "--records"]), stage(&["rg", "x"])]);
    refused(&[stage(&["ps", "--records"]), stage(&["rg", "-a", "x", "-"])]);
    refused(&[stage(&["rg", "x"]), stage(&["from", "json"])]);
    refused(&[stage(&["echo", "x"]), stage(&["rg", "x"])]);
    assert!(background_refusal(&stage(&["rg", "x"])).is_some());
    assert!(background_refusal(&stage(&["rg", "-a", "x", "-"])).is_some());
}

/// A producer that yields `rows` rows then ends with `status`; `close` before the
/// end is the `take` short circuit (status 0).
struct Fixed {
    rows: usize,
    status: Option<i32>,
    end: i32,
}

impl RowSource for Fixed {
    fn next_item(&mut self) -> Result<Option<Item>, text_engine::pipeline::PipelineError> {
        if self.rows == 0 {
            self.status = Some(self.end);
            return Ok(None);
        }
        self.rows -= 1;
        Ok(Some(Item::Row(Value::Record(vec![(String::from("n"), Value::UInt(self.rows as u64))]))))
    }

    fn close(&mut self) {
        if self.status.is_none() {
            self.status = Some(0);
        }
    }

    fn completion(&self) -> Option<i32> {
        self.status
    }
}

#[test]
fn the_producer_completion_survives_the_chain() {
    for (stages, rows, end, want) in [
        (vec![], 3, 1, Some(1)),
        (vec![Stage::To(text_engine::pipeline::Format::JsonlTyped)], 3, 2, Some(2)),
        (vec![Stage::Take { n: 1, frame: false }], 3, 2, Some(0)),
        (vec![Stage::SortBy { field: String::from("n"), reverse: false, frame: false }], 3, 1, Some(1)),
    ] {
        let (source, slot) = observe_completion(Box::new(Fixed { rows, status: None, end }));
        run_rows_within(source, Category::Rows, 0, &stages, DefaultSink::TypedJsonl, SEARCH_COLLECT_BOUND, &mut || false)
            .unwrap();
        assert_eq!(slot.get(), want, "{:?}", stages);
    }
}

#[test]
fn an_rg_collected_run_over_its_bound_fails_with_two() {
    let (source, _) = observe_completion(Box::new(Fixed { rows: 100_000, status: None, end: 0 }));
    let failure = run_rows_within(source, Category::Rows, 0, &[], DefaultSink::TypedJsonl, SEARCH_COLLECT_BOUND, &mut || false)
        .unwrap_err();
    assert_eq!(failure.status, 2);
    assert_eq!(failure.stage, 0);
}

#[test]
fn search_text_collects_rendered_lines() {
    let mut tree = Tree::new();
    tree.file("/r/a", b"x foo\n");
    let (source, _) = tree_source(tree, &args_for(&["foo", "/r"]), &InterruptLatch::inert());
    let (source, slot) = observe_completion(Box::new(source));
    let out =
        run_rows_within(source, Category::Rows, 0, &[], DefaultSink::SearchText, SEARCH_COLLECT_BOUND, &mut || false).unwrap();
    assert_eq!(out, b"/r/a:1:x foo\n");
    assert_eq!(slot.get(), Some(0));
}
