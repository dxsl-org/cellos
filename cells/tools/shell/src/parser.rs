//! Shell command parser — tokenizes a line and builds an `Ast`.
//!
//! Supported syntax (v1.0):
//!   - Simple command: `ls /bin`
//!   - Pipeline: `cat /etc/hosts | grep 127`
//!   - Output redirect: `echo hello > /tmp/a.txt`
//!   - Input redirect: `cat < /tmp/a.txt`
//!   - Append redirect: `echo hi >> /tmp/log.txt`
//!   - Background: `sleep 10 &`
//!   - Sequence: `echo a ; echo b`
//!
//! Intentionally simple: no subshells, no quoting beyond `"..."`, no globs.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    None,
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub quote: QuoteStyle,
    pub segments: Vec<WordSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSegment {
    pub text: String,
    pub quote: QuoteStyle,
}

impl Word {
    #[cfg(test)]
    fn new(text: String, quote: QuoteStyle) -> Self {
        Self {
            segments: alloc::vec![WordSegment {
                text: text.clone(),
                quote,
            }],
            text,
            quote,
        }
    }

    fn from_segments(segments: Vec<WordSegment>) -> Self {
        let mut text = String::new();
        for segment in &segments {
            text.push_str(&segment.text);
        }
        let quote = if segments.len() == 1 {
            segments[0].quote
        } else {
            QuoteStyle::None
        };
        Self {
            text,
            quote,
            segments,
        }
    }

    fn is_unquoted(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.quote == QuoteStyle::None)
    }
}

/// One redirect target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redirect {
    /// `> path`
    StdoutTo(Word),
    /// `>> path`
    StdoutAppend(Word),
    /// `< path`
    StdinFrom(Word),
    /// `2> path`
    StderrTo(Word),
}

/// A single command with its arguments and any redirects.
#[derive(Debug, Clone)]
pub struct Cmd {
    /// `argv[0]` and arguments.
    pub argv: Vec<Word>,
    /// Redirects attached to this command.
    pub redirects: Vec<Redirect>,
}

impl Cmd {
    fn new() -> Self {
        Cmd {
            argv: Vec::new(),
            redirects: Vec::new(),
        }
    }

    /// True if the command has no name (empty line or whitespace-only).
    pub fn is_empty(&self) -> bool {
        self.argv.is_empty()
    }
}

/// Top-level abstract syntax tree for one shell line.
#[derive(Debug, Clone)]
pub enum Ast {
    /// Empty input.
    Empty,
    /// A single simple command.
    Simple(Cmd),
    /// `cmd1 | cmd2 | …` — pipeline of commands.
    Pipeline(Vec<Cmd>),
    /// `cmd &` — run in background.
    Background(Cmd),
    /// `cmd1 ; cmd2` — sequential execution.
    Sequence(Vec<Ast>),
    /// `cmd1 && cmd2` — run cmd2 only if cmd1 exits 0 (success).
    And(alloc::boxed::Box<Ast>, alloc::boxed::Box<Ast>),
    /// `cmd1 || cmd2` — run cmd2 only if cmd1 exits non-zero (failure).
    Or(alloc::boxed::Box<Ast>, alloc::boxed::Box<Ast>),
    /// `while COND; do BODY; done` — loop while COND exits 0.
    While {
        cond: alloc::boxed::Box<Ast>,
        body: alloc::boxed::Box<Ast>,
    },
    /// `name() { body; }` — define a shell function.
    ///
    /// The body is stored as a reconstructed string; the executor registers it
    /// in the function table so later invocations of `name` run the body.
    FuncDef {
        name: alloc::string::String,
        body: alloc::string::String,
    },
    /// `case EXPR in pattern) BODY ;; … esac` — pattern-match dispatch.
    ///
    /// `expr` is the unexpanded word after `case`; expansion happens at
    /// execute time.  Patterns are exact strings or `*` (wildcard).
    /// Note: the whole statement must be on a single line or in a single
    /// `source`-script line; mixing with `;`-sequences on the same line is
    /// not supported (the outer `;` split would fragment the `;;` markers).
    Case {
        expr: alloc::string::String,
        arms: alloc::vec::Vec<(alloc::string::String, alloc::boxed::Box<Ast>)>,
    },
    /// `for VAR in word1 word2 …; do BODY; done` — iterate over a word list.
    ///
    /// Sets `$VAR` to each word in order, runs BODY, then advances. `$VAR`
    /// expansion in BODY uses the same static var store as `VAR=value`.
    For {
        var: alloc::string::String,
        words: alloc::vec::Vec<alloc::string::String>,
        body: alloc::boxed::Box<Ast>,
    },
    /// `if COND; then BODY; fi` — conditional execution.
    ///
    /// `cond` exit-code 0 → run `then_b`; non-zero → run `else_b` if present.
    If {
        cond: alloc::boxed::Box<Ast>,
        then_b: alloc::boxed::Box<Ast>,
        else_b: Option<alloc::boxed::Box<Ast>>,
    },
}

// ─── Tokenizer ────────────────────────────────────────────────────────────────

/// Raw token before AST construction.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(Word),
    Pipe,           // |
    RedirectOut,    // >
    RedirectAppend, // >>
    RedirectIn,     // <
    RedirectErr,    // 2>
    Ampersand,      // &   (background marker — single &)
    And,            // &&  (short-circuit AND)
    Or,             // ||  (short-circuit OR)
    Semicolon,      // ;
    LBrace,         // {  (function body open)
    RBrace,         // }  (function body close)
    // ── Conditional keywords ─────────────────────────────────────────────────
    // These variants are NEVER emitted by the tokenizer — `if`/`then`/`else`/`fi`
    // always remain as Word tokens.  parse_if_stmt detects them by string
    // comparison so they never silently disappear from external command arguments
    // (e.g. `lua -e "if x then ... end"` must reach Lua intact).
    #[allow(dead_code)] // reserved — kept for exhaustive match arms in parse_cmd
    If,
    #[allow(dead_code)] // reserved
    Then,
    #[allow(dead_code)] // reserved
    Else,
    #[allow(dead_code)] // reserved
    Fi,
}

/// Tokenize a shell input line.
///
/// Handles:
/// - Whitespace separation
/// - Simple `"..."` double-quoted strings (no escape sequences)
/// - Single-character operators: `|`, `<`, `>`, `&`, `;`
/// - Two-character operators: `>>`, `2>`
fn tokenize(line: &str) -> Vec<Tok> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current: Vec<WordSegment> = Vec::new();

    fn append_segment(current: &mut Vec<WordSegment>, text: String, quote: QuoteStyle) {
        if let Some(last) = current.last_mut() {
            if last.quote == quote {
                last.text.push_str(&text);
                return;
            }
        }
        current.push(WordSegment { text, quote });
    }

    fn push_word(tokens: &mut Vec<Tok>, current: &mut Vec<WordSegment>) {
        if !current.is_empty() {
            tokens.push(Tok::Word(Word::from_segments(core::mem::take(current))));
        }
    }

    fn read_quoted(chars: &mut core::iter::Peekable<core::str::Chars<'_>>, end: char) -> String {
        let mut text = String::new();
        loop {
            match chars.next() {
                Some(ch) if ch == end => break,
                Some(ch) => text.push(ch),
                None => break,
            }
        }
        text
    }

    macro_rules! flush {
        () => {
            push_word(&mut tokens, &mut current);
        };
    }

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                flush!();
            }
            '"' | '\'' => {
                let quote = if c == '"' {
                    QuoteStyle::Double
                } else {
                    QuoteStyle::Single
                };
                let quoted = read_quoted(&mut chars, c);
                append_segment(&mut current, quoted, quote);
            }
            '|' => {
                flush!();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Tok::Or);
                } else {
                    tokens.push(Tok::Pipe);
                }
            }
            '&' => {
                flush!();
                if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Tok::And);
                } else {
                    tokens.push(Tok::Ampersand);
                }
            }
            ';' => {
                flush!();
                tokens.push(Tok::Semicolon);
            }
            '{' => {
                flush!();
                tokens.push(Tok::LBrace);
            }
            '}' => {
                flush!();
                tokens.push(Tok::RBrace);
            }
            '<' => {
                flush!();
                tokens.push(Tok::RedirectIn);
            }
            '>' => {
                flush!();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Tok::RedirectAppend);
                } else {
                    tokens.push(Tok::RedirectOut);
                }
            }
            '2' if chars.peek() == Some(&'>') => {
                // "2>" — only if current buffer is empty (i.e. not part of a word).
                if current.is_empty() {
                    chars.next();
                    tokens.push(Tok::RedirectErr);
                } else {
                    append_segment(&mut current, String::from(c), QuoteStyle::None);
                }
            }
            // `$(...)` — command substitution.  Consume everything up to the
            // matching `)` into the current word so spaces inside don't split
            // the token.  Nested `$( $() )` increments depth correctly.
            '$' if chars.peek() == Some(&'(') => {
                chars.next(); // consume '('
                let mut substitution = String::from("$(");
                let mut depth = 1usize;
                loop {
                    match chars.next() {
                        None => break, // unclosed: pass through
                        Some('(') => {
                            depth += 1;
                            substitution.push('(');
                        }
                        Some(')') => {
                            depth -= 1;
                            substitution.push(')');
                            if depth == 0 {
                                break;
                            }
                        }
                        Some(ch) => {
                            substitution.push(ch);
                        }
                    }
                }
                append_segment(&mut current, substitution, QuoteStyle::None);
            }
            other => {
                append_segment(&mut current, String::from(other), QuoteStyle::None);
            }
        }
    }
    flush!();
    // All tokens remain as their natural type — no keyword conversion.
    // The if-statement parser detects `if`/`then`/`else`/`fi` by string
    // comparison on Word tokens so they are never eaten from command arguments.
    tokens
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parse a shell line into an `Ast`.
///
/// Returns `Ast::Empty` for blank input.
pub fn parse(line: &str) -> Ast {
    let tokens = tokenize(line.trim());
    if tokens.is_empty() {
        return Ast::Empty;
    }

    // `if...then...fi` must be parsed BEFORE semicolon splitting, because the
    // semicolons inside an if-statement are structural (not sequence separators).
    // Keywords remain as Word tokens (not converted) so they survive in external
    // command argument strings (e.g. `lua -e "if x then ... end"`).
    // `name() { body; }` — function definition.
    // name() is a single Word token ending with "()" (no spaces inside).
    if let Some(Tok::Word(w)) = tokens.first() {
        if w.is_unquoted()
            && w.text.ends_with("()")
            && tokens.get(1) == Some(&Tok::LBrace)
            && tokens.last() == Some(&Tok::RBrace)
        {
            let name = String::from(&w.text[..w.text.len() - 2]);
            let body = tokens_to_string(&tokens[2..tokens.len() - 1]);
            return Ast::FuncDef { name, body };
        }
    }

    if is_kw_token(tokens.first(), "if") {
        return parse_if_stmt(&tokens);
    }
    if is_kw_token(tokens.first(), "while") {
        return parse_while_stmt(&tokens);
    }
    if is_kw_token(tokens.first(), "for") {
        return parse_for_stmt(&tokens);
    }
    // `case` must be parsed before semicolon splitting because `;;` arm
    // separators are also Semicolon tokens and would fragment the statement.
    if is_kw_token(tokens.first(), "case") {
        return parse_case_stmt(&tokens);
    }

    // Split on `;` into sub-sequences first.
    let segments: Vec<&[Tok]> = split_on(&tokens, |t| t == &Tok::Semicolon);
    if segments.len() > 1 {
        let seq: Vec<Ast> = segments.iter().map(|seg| parse_pipeline(seg)).collect();
        return Ast::Sequence(seq);
    }

    parse_pipeline(&tokens)
}

/// Parse a token sub-slice that may contain `;`-separated commands.
///
/// Equivalent to the main `parse()` body but operating on a pre-tokenized
/// slice — used by `parse_if_stmt` to parse condition and body sections.
fn parse_tokens(tokens: &[Tok]) -> Ast {
    // Strip leading/trailing semicolons that linger from the structural split.
    let start = tokens
        .iter()
        .position(|t| t != &Tok::Semicolon)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| t != &Tok::Semicolon)
        .map(|i| i + 1)
        .unwrap_or(0);
    let tokens = &tokens[start..end];
    if tokens.is_empty() {
        return Ast::Empty;
    }
    let segments: Vec<&[Tok]> = split_on(tokens, |t| t == &Tok::Semicolon);
    if segments.len() > 1 {
        let seq: Vec<Ast> = segments.iter().map(|seg| parse_pipeline(seg)).collect();
        return Ast::Sequence(seq);
    }
    parse_pipeline(tokens)
}

/// Parse `if COND; then BODY; fi` or `if COND; then BODY; else BODY; fi`.
///
/// Handles any number of semicolons around the keywords — the structure is
/// determined by the `Then`, `Else`, and `Fi` token positions.
/// Serialize a token slice back to a space-separated shell line.
///
/// Used by function definition to store the body as a re-parseable string.
fn tokens_to_string(tokens: &[Tok]) -> String {
    let parts: alloc::vec::Vec<&str> = tokens
        .iter()
        .map(|t| match t {
            Tok::Word(w) => w.text.as_str(),
            Tok::Pipe => "|",
            Tok::And => "&&",
            Tok::Or => "||",
            Tok::Semicolon => ";",
            Tok::Ampersand => "&",
            Tok::RedirectOut => ">",
            Tok::RedirectAppend => ">>",
            Tok::RedirectIn => "<",
            Tok::RedirectErr => "2>",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            _ => "",
        })
        .collect();
    parts.join(" ")
}

/// Helper: returns true when a token is the keyword word `w`.
fn is_kw(tok: &Tok, w: &str) -> bool {
    matches!(tok, Tok::Word(word) if word.is_unquoted() && word.text == w)
}

fn is_kw_token(tok: Option<&Tok>, w: &str) -> bool {
    matches!(tok, Some(tok) if is_kw(tok, w))
}

/// Parse `while COND; do BODY; done`.
///
/// Keywords stay as `Word` tokens (no Tok variants) so `while`/`do`/`done`
/// survive intact when used as external command arguments.  Malformed input
/// (missing `do` or `done`) calls `parse_tokens` — NOT `parse()` — to avoid
/// re-dispatching on the leading `while` and recursing infinitely.
/// Parse `for VAR in word1 word2 …; do BODY; done`.
///
/// Keywords stay as `Word` tokens (same Phase N/O rule) so `for`/`in`/`do`/`done`
/// survive as external command arguments.  Malformed input falls back to
/// `parse_tokens` (not `parse()`) to prevent infinite recursion.
fn parse_for_stmt(tokens: &[Tok]) -> Ast {
    // tokens[1] = variable name; tokens[2] should be Word("in").
    let var = match tokens.get(1) {
        Some(Tok::Word(w)) if w.is_unquoted() && w.text != "in" => w.text.clone(),
        _ => return parse_tokens(tokens),
    };
    let in_pos = match tokens.iter().position(|t| is_kw(t, "in")) {
        Some(p) => p,
        None => return parse_tokens(tokens),
    };
    let do_pos = tokens.iter().position(|t| is_kw(t, "do"));
    let done_pos = tokens.iter().rposition(|t| is_kw(t, "done"));
    let (dp, np) = match (do_pos, done_pos) {
        (Some(d), Some(n)) if n > d => (d, n),
        _ => return parse_tokens(tokens),
    };
    // Word list: tokens between `in` and `do`, stripping Semicolons.
    let words: alloc::vec::Vec<alloc::string::String> = tokens[in_pos + 1..dp]
        .iter()
        .filter_map(|t| {
            if let Tok::Word(w) = t {
                Some(w.text.clone())
            } else {
                None
            }
        })
        .collect();
    let body = parse_tokens(&tokens[dp + 1..np]);
    Ast::For {
        var,
        words,
        body: alloc::boxed::Box::new(body),
    }
}

/// Parse `case EXPR in pattern1) BODY ;; pattern2) BODY ;; esac`.
///
/// `;;` is two consecutive `Tok::Semicolon` tokens.  Patterns are the first
/// token of each arm with a trailing `)` stripped.  `*` is a catch-all.
/// Malformed input (missing `in` or `esac`) falls back to `parse_tokens`.
fn parse_case_stmt(tokens: &[Tok]) -> Ast {
    let in_pos = tokens.iter().position(|t| is_kw(t, "in"));
    let esac_pos = tokens.iter().rposition(|t| is_kw(t, "esac"));
    let (ip, ep) = match (in_pos, esac_pos) {
        (Some(i), Some(e)) if e > i => (i, e),
        _ => return parse_tokens(tokens),
    };

    // Expression: single word between `case` and `in`.
    let expr = match tokens.get(1) {
        Some(Tok::Word(w)) => w.text.clone(),
        _ => String::new(),
    };

    // Arms: tokens[ip+1..ep] split on `;;` (two consecutive Semicolons).
    let arm_tokens = &tokens[ip + 1..ep];
    let mut arms: alloc::vec::Vec<(String, alloc::boxed::Box<Ast>)> = alloc::vec::Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < arm_tokens.len() {
        if i + 1 < arm_tokens.len()
            && arm_tokens[i] == Tok::Semicolon
            && arm_tokens[i + 1] == Tok::Semicolon
        {
            push_arm(&arm_tokens[start..i], &mut arms);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    push_arm(&arm_tokens[start..], &mut arms); // last arm (no trailing ;;)

    Ast::Case { expr, arms }
}

fn push_arm(tokens: &[Tok], arms: &mut alloc::vec::Vec<(String, alloc::boxed::Box<Ast>)>) {
    // Strip leading/trailing Semicolons left from the split.
    let start = tokens
        .iter()
        .position(|t| t != &Tok::Semicolon)
        .unwrap_or(tokens.len());
    let tokens = &tokens[start..];
    if let Some(Tok::Word(pat)) = tokens.first() {
        let pattern = String::from(pat.text.trim_end_matches(')'));
        let body = parse_tokens(&tokens[1..]);
        arms.push((pattern, alloc::boxed::Box::new(body)));
    }
}

fn parse_while_stmt(tokens: &[Tok]) -> Ast {
    let do_pos = tokens.iter().position(|t| is_kw(t, "do"));
    let done_pos = tokens.iter().rposition(|t| is_kw(t, "done"));
    let (dp, np) = match (do_pos, done_pos) {
        (Some(d), Some(n)) if n > d => (d, n),
        _ => return parse_tokens(tokens), // malformed: fall back without infinite recursion
    };
    let cond = parse_tokens(&tokens[1..dp]);
    let body = parse_tokens(&tokens[dp + 1..np]);
    Ast::While {
        cond: alloc::boxed::Box::new(cond),
        body: alloc::boxed::Box::new(body),
    }
}

fn parse_if_stmt(tokens: &[Tok]) -> Ast {
    // Locate structural keywords after the leading `if` Word.
    // Keywords are plain Word tokens — never converted — so they survive intact
    // in external command argument strings.
    let then_pos = tokens
        .iter()
        .position(|t| is_kw(t, "then"))
        .unwrap_or(tokens.len());
    let else_pos = tokens.iter().position(|t| is_kw(t, "else"));
    let fi_pos = tokens
        .iter()
        .rposition(|t| is_kw(t, "fi"))
        .unwrap_or(tokens.len());

    // Condition: tokens[1..then_pos]   (skip leading `If`)
    let cond_slice = &tokens[1..then_pos];
    let cond = parse_tokens(cond_slice);

    // Then body: tokens[then_pos+1..else_or_fi]
    let then_end = else_pos.unwrap_or(fi_pos);
    let then_slice = &tokens[then_pos + 1..then_end];
    let then_b = parse_tokens(then_slice);

    // Else body (optional): tokens[else_pos+1..fi_pos]
    let else_b = else_pos.map(|ep| {
        let slice = &tokens[ep + 1..fi_pos];
        alloc::boxed::Box::new(parse_tokens(slice))
    });

    Ast::If {
        cond: alloc::boxed::Box::new(cond),
        then_b: alloc::boxed::Box::new(then_b),
        else_b,
    }
}

fn parse_pipeline(tokens: &[Tok]) -> Ast {
    // `&&` / `||` have lower precedence than pipelines — check first.
    // Split on the FIRST occurrence; right side is parsed recursively so
    // `A && B && C` builds `And(A, And(B, C))` with left-to-right evaluation.
    if let Some(pos) = tokens.iter().position(|t| t == &Tok::And || t == &Tok::Or) {
        let left = parse_pipeline(&tokens[..pos]);
        let right = parse_pipeline(&tokens[pos + 1..]);
        return match &tokens[pos] {
            Tok::And => Ast::And(alloc::boxed::Box::new(left), alloc::boxed::Box::new(right)),
            Tok::Or => Ast::Or(alloc::boxed::Box::new(left), alloc::boxed::Box::new(right)),
            _ => unreachable!(),
        };
    }

    let pipe_segs: Vec<&[Tok]> = split_on(tokens, |t| t == &Tok::Pipe);

    let cmds: Vec<Cmd> = pipe_segs
        .iter()
        .map(|seg| {
            // Ignore the per-segment `bg` flag here — the trailing `&` check on
            // `tokens.last()` below is the authoritative background detector.
            // Filtering out `bg=true` segments caused single-command background
            // jobs (`httpd 9091 /path &`) to be parsed as Ast::Empty.
            let (cmd, _bg) = parse_cmd(seg);
            cmd
        })
        .filter(|c| !c.is_empty())
        .collect();

    // Check for trailing `&` (background marker).
    let background = tokens.last() == Some(&Tok::Ampersand);

    if background && cmds.len() == 1 {
        return Ast::Background(cmds.into_iter().next().unwrap_or_else(Cmd::new));
    }
    match cmds.len() {
        0 => Ast::Empty,
        1 => Ast::Simple(cmds.into_iter().next().unwrap_or_else(Cmd::new)),
        _ => Ast::Pipeline(cmds),
    }
}

/// Parse one command segment (no `|`, `;`, or `&` except trailing `&`).
/// Returns (Cmd, is_background).
fn parse_cmd(tokens: &[Tok]) -> (Cmd, bool) {
    let mut cmd = Cmd::new();
    let mut background = false;
    let mut iter = tokens.iter().peekable();

    while let Some(tok) = iter.next() {
        match tok {
            Tok::Word(w) => cmd.argv.push(w.clone()),
            Tok::Ampersand => background = true,
            Tok::RedirectOut => {
                if let Some(Tok::Word(path)) = iter.next() {
                    cmd.redirects.push(Redirect::StdoutTo(path.clone()));
                }
            }
            Tok::RedirectAppend => {
                if let Some(Tok::Word(path)) = iter.next() {
                    cmd.redirects.push(Redirect::StdoutAppend(path.clone()));
                }
            }
            Tok::RedirectIn => {
                if let Some(Tok::Word(path)) = iter.next() {
                    cmd.redirects.push(Redirect::StdinFrom(path.clone()));
                }
            }
            Tok::RedirectErr => {
                if let Some(Tok::Word(path)) = iter.next() {
                    cmd.redirects.push(Redirect::StderrTo(path.clone()));
                }
            }
            _ => {}
        }
    }
    (cmd, background)
}

/// Split a token slice on positions where `pred` returns true.
fn split_on<F>(tokens: &[Tok], pred: F) -> Vec<&[Tok]>
where
    F: Fn(&Tok) -> bool,
{
    let mut result = Vec::new();
    let mut start = 0;
    for (i, tok) in tokens.iter().enumerate() {
        if pred(tok) {
            result.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    result.push(&tokens[start..]);
    result
}

// ─── Tests (host-runnable) ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        assert!(matches!(parse(""), Ast::Empty));
        assert!(matches!(parse("   "), Ast::Empty));
    }

    #[test]
    fn parse_simple() {
        if let Ast::Simple(cmd) = parse("ls /bin") {
            assert_eq!(cmd.argv[0], Word::new("ls".to_string(), QuoteStyle::None));
            assert_eq!(cmd.argv[1], Word::new("/bin".to_string(), QuoteStyle::None));
        } else {
            panic!("expected Simple");
        }
    }

    #[test]
    fn parse_pipeline() {
        if let Ast::Pipeline(cmds) = parse("cat /etc/hosts | grep 127") {
            assert_eq!(cmds.len(), 2);
            assert_eq!(cmds[0].argv[0].text, "cat");
            assert_eq!(cmds[1].argv[0].text, "grep");
        } else {
            panic!("expected Pipeline");
        }
    }

    #[test]
    fn parse_redirect_out() {
        if let Ast::Simple(cmd) = parse("echo hi > /tmp/a.txt") {
            assert_eq!(
                cmd.redirects,
                &[Redirect::StdoutTo(Word::new(
                    String::from("/tmp/a.txt"),
                    QuoteStyle::None
                ))]
            );
        } else {
            panic!("expected Simple with redirect");
        }
    }

    #[test]
    fn parse_redirect_append() {
        if let Ast::Simple(cmd) = parse("echo hi >> /tmp/log") {
            assert!(matches!(&cmd.redirects[0], Redirect::StdoutAppend(_)));
        } else {
            panic!("expected Simple with append redirect");
        }
    }

    #[test]
    fn parse_background() {
        assert!(matches!(parse("sleep 10 &"), Ast::Background(_)));
    }

    #[test]
    fn parse_sequence() {
        assert!(matches!(parse("echo a ; echo b"), Ast::Sequence(_)));
    }

    #[test]
    fn parse_quote_metadata() {
        if let Ast::Simple(cmd) = parse("grep -e 'a b' \"$HOME\"") {
            assert_eq!(
                cmd.argv[2],
                Word::new("a b".to_string(), QuoteStyle::Single)
            );
            assert_eq!(
                cmd.argv[3],
                Word::new("$HOME".to_string(), QuoteStyle::Double)
            );
        } else {
            panic!("expected Simple");
        }
    }

    #[test]
    fn parse_mixed_quote_segments() {
        if let Ast::Simple(cmd) = parse("echo pre'$HOME' '$HOME'suffix") {
            assert_eq!(
                cmd.argv[1].segments,
                alloc::vec![
                    WordSegment {
                        text: "pre".to_string(),
                        quote: QuoteStyle::None,
                    },
                    WordSegment {
                        text: "$HOME".to_string(),
                        quote: QuoteStyle::Single,
                    },
                ]
            );
            assert_eq!(
                cmd.argv[2].segments,
                alloc::vec![
                    WordSegment {
                        text: "$HOME".to_string(),
                        quote: QuoteStyle::Single,
                    },
                    WordSegment {
                        text: "suffix".to_string(),
                        quote: QuoteStyle::None,
                    },
                ]
            );
        } else {
            panic!("expected Simple");
        }
    }

    #[test]
    fn parse_empty_quoted_words() {
        if let Ast::Simple(cmd) = parse("echo \"\" ''") {
            assert_eq!(cmd.argv.len(), 3);
            assert_eq!(cmd.argv[1], Word::new(String::new(), QuoteStyle::Double));
            assert_eq!(cmd.argv[2], Word::new(String::new(), QuoteStyle::Single));
        } else {
            panic!("expected Simple");
        }
        if let Ast::Simple(cmd) = parse("grep \"\"") {
            assert_eq!(cmd.argv[1], Word::new(String::new(), QuoteStyle::Double));
        } else {
            panic!("expected Simple");
        }
    }
}
