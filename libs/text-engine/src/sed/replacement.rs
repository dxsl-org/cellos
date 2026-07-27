use alloc::string::String;
use alloc::vec::Vec;

use super::SedError;

pub struct Replacement {
    parts: Vec<ReplacementPart>,
}

enum ReplacementPart {
    Literal(String),
    Match,
}

impl Replacement {
    pub fn compile(raw: &str, delim: char) -> Result<Self, SedError> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            if ch == '&' {
                flush_literal(&mut parts, &mut literal);
                parts.push(ReplacementPart::Match);
                continue;
            }
            if ch == '\\' {
                let next = chars.next().ok_or(SedError::Parse("unterminated escape"))?;
                match next {
                    value if value == delim || value == '\\' || value == '&' => literal.push(value),
                    value => {
                        literal.push('\\');
                        literal.push(value);
                    }
                }
                continue;
            }
            literal.push(ch);
        }
        flush_literal(&mut parts, &mut literal);
        Ok(Self { parts })
    }

    pub fn expand(&self, matched: &str) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                ReplacementPart::Literal(literal) => out.push_str(literal),
                ReplacementPart::Match => out.push_str(matched),
            }
        }
        out
    }
}

fn flush_literal(parts: &mut Vec<ReplacementPart>, literal: &mut String) {
    if !literal.is_empty() {
        parts.push(ReplacementPart::Literal(core::mem::take(literal)));
    }
}

#[cfg(test)]
mod tests {
    use super::Replacement;

    #[test]
    fn decodes_escaped_delimiter_backslash_and_match() {
        let replacement = Replacement::compile(r"left\|\\\&-&", '|').expect("replacement parses");
        assert_eq!(replacement.expand("MID"), r"left|\&-MID");
    }
}
