use crate::matcher::{PatternKind, MAX_PATTERN_BYTES};
use crate::records::{RecordReader, MAX_FILE_OPERANDS};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const MAX_PATTERNS: usize = 32;
const MAX_PATTERN_FILES: usize = 8;

/// Reads a `-f PATTERNFILE` operand into memory.
///
/// Injected by the caller so option parsing stays free of VFS/syscall access:
/// the shell passes its VFS reader, host tests pass an in-memory table. The
/// `Err(String)` payload is a human-readable reason (e.g. "cannot read").
pub type PatternFileReader<'a> = &'a mut dyn FnMut(&str) -> Result<String, String>;

#[derive(Debug)]
pub struct Config {
    pub kind: PatternKind,
    pub patterns: Vec<String>,
    pub files: Vec<String>,
    pub case_insensitive: bool,
    pub invert: bool,
    pub line_numbers: bool,
    pub count_only: bool,
    pub quiet: bool,
    pub exact_line: bool,
    pub recursive: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Message(&'static str),
    Owned(String),
}

impl ParseError {
    pub fn message(&self) -> &str {
        match self {
            Self::Message(message) => message,
            Self::Owned(message) => message,
        }
    }
}

/// Parse grep operands into a [`Config`].
///
/// # Errors
/// Returns [`ParseError`] on an unknown flag, a missing flag value, a pattern
/// or operand count past its limit, or an unreadable/empty `-f` pattern file.
pub fn parse(
    args: &[String],
    read_pattern_file: PatternFileReader<'_>,
) -> Result<Config, ParseError> {
    let mut cfg = Config {
        kind: PatternKind::Fixed,
        patterns: Vec::new(),
        files: Vec::new(),
        case_insensitive: false,
        invert: false,
        line_numbers: false,
        count_only: false,
        quiet: false,
        exact_line: false,
        recursive: false,
    };
    let mut index = 0usize;
    let mut pattern_files = 0usize;
    let mut stop_flags = false;
    while let Some(arg) = args.get(index).map(String::as_str) {
        index += 1;
        if !stop_flags && arg == "--" {
            stop_flags = true;
            continue;
        }
        if !stop_flags && arg.starts_with("-e") && arg.len() > 2 {
            push_pattern(&mut cfg.patterns, &arg[2..])?;
            continue;
        }
        if !stop_flags && arg.starts_with("-f") && arg.len() > 2 {
            pattern_files += 1;
            load_pattern_file(
                &mut cfg.patterns,
                &arg[2..],
                pattern_files,
                read_pattern_file,
            )?;
            continue;
        }
        if !stop_flags && arg.starts_with('-') && arg.len() > 1 {
            match arg {
                "-E" => cfg.kind = PatternKind::ERELite,
                "-F" => cfg.kind = PatternKind::Fixed,
                "-e" => {
                    let value = args
                        .get(index)
                        .ok_or(ParseError::Message("missing value for -e"))?;
                    index += 1;
                    push_pattern(&mut cfg.patterns, value)?;
                }
                "-f" => {
                    let value = args
                        .get(index)
                        .ok_or(ParseError::Message("missing value for -f"))?;
                    index += 1;
                    pattern_files += 1;
                    load_pattern_file(&mut cfg.patterns, value, pattern_files, read_pattern_file)?;
                }
                _ => parse_cluster(&mut cfg, arg)?,
            }
            continue;
        }
        if cfg.patterns.is_empty() {
            push_pattern(&mut cfg.patterns, arg)?;
        } else {
            if cfg.files.len() >= MAX_FILE_OPERANDS {
                return Err(ParseError::Owned(format!(
                    "too many file operands (max {MAX_FILE_OPERANDS})"
                )));
            }
            cfg.files.push(String::from(arg));
        }
    }
    if cfg.patterns.is_empty() {
        return Err(ParseError::Message("missing pattern"));
    }
    Ok(cfg)
}

fn parse_cluster(cfg: &mut Config, arg: &str) -> Result<(), ParseError> {
    for flag in arg[1..].chars() {
        match flag {
            'E' => cfg.kind = PatternKind::ERELite,
            'F' => cfg.kind = PatternKind::Fixed,
            'i' => cfg.case_insensitive = true,
            'v' => cfg.invert = true,
            'n' => cfg.line_numbers = true,
            'c' => cfg.count_only = true,
            'q' => cfg.quiet = true,
            'x' => cfg.exact_line = true,
            'r' => cfg.recursive = true,
            'e' | 'f' => {
                return Err(ParseError::Message(
                    "clustered -e/-f requires a separate value",
                ))
            }
            _ => return Err(ParseError::Owned(format!("unknown flag '-{flag}'"))),
        }
    }
    Ok(())
}

fn push_pattern(patterns: &mut Vec<String>, value: &str) -> Result<(), ParseError> {
    if patterns.len() >= MAX_PATTERNS {
        return Err(ParseError::Owned(format!(
            "too many patterns (max {MAX_PATTERNS})"
        )));
    }
    if value.len() > MAX_PATTERN_BYTES {
        return Err(ParseError::Owned(format!(
            "pattern exceeds {MAX_PATTERN_BYTES}-byte limit"
        )));
    }
    patterns.push(String::from(value));
    Ok(())
}

fn load_pattern_file(
    patterns: &mut Vec<String>,
    path: &str,
    pattern_files: usize,
    read_pattern_file: PatternFileReader<'_>,
) -> Result<(), ParseError> {
    if pattern_files > MAX_PATTERN_FILES {
        return Err(ParseError::Owned(format!(
            "too many pattern files (max {MAX_PATTERN_FILES})"
        )));
    }
    let text = read_pattern_file(path)
        .map_err(|err| ParseError::Owned(format!("pattern file '{path}': {err}")))?;
    let mut records = RecordReader::new(&text).map_err(|_| {
        ParseError::Owned(format!("pattern file '{path}' exceeds configured limits"))
    })?;
    let mut added = 0usize;
    while let Some(pattern) = records.next_record().map_err(|_| {
        ParseError::Owned(format!("pattern file '{path}' exceeds configured limits"))
    })? {
        push_pattern(patterns, pattern)?;
        added += 1;
    }
    if added == 0 {
        return Err(ParseError::Owned(format!("pattern file '{path}' is empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse, push_pattern, ParseError};
    use crate::matcher::MAX_PATTERN_BYTES;
    use alloc::{string::String, vec, vec::Vec};

    #[test]
    fn rejects_pattern_before_copying_beyond_limit() {
        let mut patterns = Vec::new();
        let oversized = String::from_utf8(vec![b'x'; MAX_PATTERN_BYTES + 1]).unwrap();
        assert!(matches!(
            push_pattern(&mut patterns, &oversized),
            Err(ParseError::Owned(_))
        ));
        assert!(patterns.is_empty());
    }

    #[test]
    fn pattern_file_reader_supplies_one_pattern_per_record() {
        let mut reader = |path: &str| {
            assert_eq!(path, "/tmp/pats");
            Ok(String::from("alpha\nbeta\n"))
        };
        let cfg = parse(
            &[String::from("-f"), String::from("/tmp/pats")],
            &mut reader,
        )
        .expect("pattern file should load");
        assert_eq!(cfg.patterns, ["alpha", "beta"]);
    }

    #[test]
    fn pattern_file_read_failure_surfaces_the_reader_reason() {
        let mut reader = |_: &str| Err(String::from("cannot read"));
        let err = parse(&[String::from("-f/missing")], &mut reader).expect_err("must fail");
        assert_eq!(err.message(), "pattern file '/missing': cannot read");
    }

    #[test]
    fn empty_pattern_file_is_rejected() {
        let mut reader = |_: &str| Ok(String::new());
        let err = parse(&[String::from("-f/empty")], &mut reader).expect_err("must fail");
        assert_eq!(err.message(), "pattern file '/empty' is empty");
    }
}
