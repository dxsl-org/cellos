use alloc::string::String;
use regex_automata::{meta::Regex, nfa::thompson::NFA, util::syntax};
use regex_syntax::hir::{Hir, HirKind};

use super::replacement::Replacement;
use super::{validate_output_len, SedError};

const MAX_PATTERN_BYTES: usize = 256;
const MAX_AST_DEPTH: u32 = 32;
const MAX_REPEAT_BOUND: u32 = 256;
const MAX_COMPILED_STATES: usize = 4096;
const MAX_NFA_BYTES: usize = 256 * 1024;
const REGEX_METACHARS: &str = ".+*?()|[]{}^$\\";

pub enum SedPattern {
    Literal(String),
    Regex(Regex),
}

impl SedPattern {
    pub fn compile(raw: &str, delim: char) -> Result<Self, SedError> {
        let pattern = normalize(raw, delim);
        if pattern.is_empty() {
            return Err(SedError::Parse("empty pattern"));
        }
        if pattern.len() > MAX_PATTERN_BYTES {
            return Err(SedError::Parse("pattern exceeds 256-byte limit"));
        }
        let syntax_cfg = syntax::Config::new()
            .case_insensitive(false)
            .unicode(false)
            .utf8(false)
            .nest_limit(MAX_AST_DEPTH);
        let hir = syntax::parse_with(&pattern, &syntax_cfg)
            .map_err(|_| SedError::Parse("invalid pattern"))?;
        validate_hir(&hir)?;
        if looks_literal(&hir) && !pattern.contains('\\') {
            return Ok(Self::Literal(pattern));
        }
        let nfa = NFA::compiler()
            .configure(
                NFA::config()
                    .utf8(false)
                    .nfa_size_limit(Some(MAX_NFA_BYTES)),
            )
            .build_from_hir(&hir)
            .map_err(|_| SedError::Parse("pattern exceeds compiled size limits"))?;
        if nfa.states().len() > MAX_COMPILED_STATES {
            return Err(SedError::Parse("pattern exceeds compiled size limits"));
        }
        Regex::builder()
            .build_from_hir(&hir)
            .map(Self::Regex)
            .map_err(|_| SedError::Parse("invalid pattern"))
    }

    pub fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Literal(pattern) => line.contains(pattern),
            Self::Regex(regex) => regex.is_match(line.as_bytes()),
        }
    }

    pub fn replace(
        &self,
        line: &str,
        replacement: &Replacement,
        global: bool,
    ) -> Result<(String, bool), SedError> {
        match self {
            Self::Literal(pattern) => replace_literal(line, pattern, replacement, global),
            Self::Regex(regex) => replace_regex(line, regex, replacement, global),
        }
    }
}

fn normalize(raw: &str, delim: char) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                if next == delim {
                    if is_regex_metachar(next) {
                        out.push('\\');
                    }
                    out.push(next);
                } else {
                    out.push('\\');
                    out.push(next);
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_regex_metachar(ch: char) -> bool {
    REGEX_METACHARS.contains(ch)
}

fn replace_literal(
    line: &str,
    pattern: &str,
    replacement: &Replacement,
    global: bool,
) -> Result<(String, bool), SedError> {
    let mut out = String::new();
    let mut rest = line;
    let mut changed = false;
    while let Some(index) = rest.find(pattern) {
        changed = true;
        out.push_str(&rest[..index]);
        out.push_str(&replacement.expand(pattern));
        validate_output_len(&out)?;
        rest = &rest[index + pattern.len()..];
        if !global {
            out.push_str(rest);
            validate_output_len(&out)?;
            return Ok((out, true));
        }
    }
    if changed {
        out.push_str(rest);
        validate_output_len(&out)?;
        Ok((out, true))
    } else {
        Ok((String::from(line), false))
    }
}

fn replace_regex(
    line: &str,
    regex: &Regex,
    replacement: &Replacement,
    global: bool,
) -> Result<(String, bool), SedError> {
    let mut out = String::new();
    let mut last = 0usize;
    let mut changed = false;
    for matched in regex.find_iter(line.as_bytes()) {
        changed = true;
        let range = matched.start()..matched.end();
        out.push_str(&line[last..range.start]);
        out.push_str(&replacement.expand(&line[range.clone()]));
        validate_output_len(&out)?;
        last = range.end;
        if !global {
            break;
        }
    }
    if !changed {
        return Ok((String::from(line), false));
    }
    out.push_str(&line[last..]);
    validate_output_len(&out)?;
    Ok((out, true))
}

fn validate_hir(hir: &Hir) -> Result<(), SedError> {
    match hir.kind() {
        HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) => Ok(()),
        HirKind::Look(look) => match alloc::format!("{look:?}").contains("Word") {
            true => Err(SedError::Parse("unsupported ERE-lite construct")),
            false => Ok(()),
        },
        HirKind::Capture(capture) => validate_hir(&capture.sub),
        HirKind::Concat(items) | HirKind::Alternation(items) => {
            for item in items {
                validate_hir(item)?;
            }
            Ok(())
        }
        HirKind::Repetition(rep) => {
            if rep.min > MAX_REPEAT_BOUND || rep.max.is_some_and(|max| max > MAX_REPEAT_BOUND) {
                return Err(SedError::Parse("pattern repetition exceeds 256"));
            }
            validate_hir(&rep.sub)
        }
    }
}

fn looks_literal(hir: &Hir) -> bool {
    match hir.kind() {
        HirKind::Empty | HirKind::Literal(_) => true,
        HirKind::Concat(items) => items.iter().all(looks_literal),
        _ => false,
    }
}

#[cfg(test)]
#[path = "pattern/tests.rs"]
mod tests;
