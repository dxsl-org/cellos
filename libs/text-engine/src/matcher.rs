use alloc::format;
use alloc::string::String;
use regex_automata::{meta::Regex, nfa::thompson::NFA, util::syntax};
use regex_syntax::hir::{Hir, HirKind};

pub const MAX_PATTERN_BYTES: usize = 256;
const MAX_AST_DEPTH: u32 = 32;
const MAX_REPEAT_BOUND: u32 = 256;
const MAX_COMPILED_STATES: usize = 4096;
const MAX_NFA_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    Fixed,
    ERELite,
}

#[derive(Debug)]
pub enum PatternError {
    TooLong,
    RepeatTooLarge,
    Unsupported,
    TooManyStates,
    Invalid,
}

impl PatternError {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooLong => "pattern exceeds 256-byte limit",
            Self::RepeatTooLarge => "pattern repetition exceeds 256",
            Self::Unsupported => "unsupported ERE-lite construct",
            Self::TooManyStates => "pattern exceeds compiled size limits",
            Self::Invalid => "invalid pattern",
        }
    }
}

pub struct CompiledPattern {
    kind: PatternKind,
    fixed: String,
    regex: Option<Regex>,
    case_insensitive: bool,
}

impl CompiledPattern {
    pub fn compile(
        kind: PatternKind,
        pattern: &str,
        case_insensitive: bool,
    ) -> Result<Self, PatternError> {
        if pattern.len() > MAX_PATTERN_BYTES {
            return Err(PatternError::TooLong);
        }
        match kind {
            PatternKind::Fixed => Ok(Self {
                kind,
                fixed: String::from(pattern),
                regex: None,
                case_insensitive,
            }),
            PatternKind::ERELite => {
                let syntax_cfg = syntax::Config::new()
                    .case_insensitive(case_insensitive)
                    .unicode(false)
                    .utf8(false)
                    .nest_limit(MAX_AST_DEPTH);
                let hir =
                    syntax::parse_with(pattern, &syntax_cfg).map_err(|_| PatternError::Invalid)?;
                validate_hir(&hir)?;
                let nfa = NFA::compiler()
                    .configure(
                        NFA::config()
                            .utf8(false)
                            .nfa_size_limit(Some(MAX_NFA_BYTES)),
                    )
                    .build_from_hir(&hir)
                    .map_err(|_| PatternError::TooManyStates)?;
                if nfa.states().len() > MAX_COMPILED_STATES {
                    return Err(PatternError::TooManyStates);
                }
                let regex = Regex::builder()
                    .build_from_hir(&hir)
                    .map_err(|_| PatternError::Invalid)?;
                Ok(Self {
                    kind,
                    fixed: String::new(),
                    regex: Some(regex),
                    case_insensitive,
                })
            }
        }
    }

    pub fn is_match(&self, haystack: &str) -> bool {
        match self.kind {
            PatternKind::Fixed => {
                if self.case_insensitive {
                    contains_insensitive(haystack, &self.fixed)
                } else {
                    haystack.contains(&self.fixed)
                }
            }
            PatternKind::ERELite => self
                .regex
                .as_ref()
                .map(|regex| regex.is_match(haystack.as_bytes()))
                .unwrap_or(false),
        }
    }

    pub fn literal(&self) -> Option<&str> {
        match self.kind {
            PatternKind::Fixed => Some(self.fixed.as_str()),
            PatternKind::ERELite => None,
        }
    }
}

fn validate_hir(hir: &Hir) -> Result<(), PatternError> {
    match hir.kind() {
        HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) => Ok(()),
        HirKind::Look(look) => {
            if format!("{look:?}").contains("Word") {
                Err(PatternError::Unsupported)
            } else {
                Ok(())
            }
        }
        HirKind::Capture(capture) => validate_hir(&capture.sub),
        HirKind::Concat(items) | HirKind::Alternation(items) => {
            for item in items {
                validate_hir(item)?;
            }
            Ok(())
        }
        HirKind::Repetition(rep) => {
            if rep.min > MAX_REPEAT_BOUND || rep.max.is_some_and(|max| max > MAX_REPEAT_BOUND) {
                return Err(PatternError::RepeatTooLarge);
            }
            if matches!(rep.sub.kind(), HirKind::Alternation(_)) {
                return Err(PatternError::Unsupported);
            }
            validate_hir(&rep.sub)
        }
    }
}

fn contains_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let last = h.len().saturating_sub(n.len());
    for start in 0..=last {
        if h[start..start + n.len()]
            .iter()
            .zip(n.iter())
            .all(|(lhs, rhs)| lhs.eq_ignore_ascii_case(rhs))
        {
            return true;
        }
    }
    false
}
