use alloc::string::String;
use alloc::vec::Vec;

use super::AwkError;

const MAX_TOKENS: usize = 128;
const MAX_STRING_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Print,
    Nr,
    Nf,
    Field(u8),
    Number(i64),
    Text(String),
    Comma,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    AndAnd,
    OrOr,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, AwkError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if tokens.len() >= MAX_TOKENS {
            return Err(AwkError::Lex("program exceeds token limit"));
        }
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'(' => push(&mut tokens, Token::LParen, &mut index),
            b')' => push(&mut tokens, Token::RParen, &mut index),
            b',' => push(&mut tokens, Token::Comma, &mut index),
            b'+' => push(&mut tokens, Token::Plus, &mut index),
            b'-' => push(&mut tokens, Token::Minus, &mut index),
            b'*' => push(&mut tokens, Token::Star, &mut index),
            b'/' => push(&mut tokens, Token::Slash, &mut index),
            b'%' => push(&mut tokens, Token::Percent, &mut index),
            b'$' => {
                index += 1;
                let digit = *bytes
                    .get(index)
                    .ok_or(AwkError::Lex("expected field number"))?;
                if !digit.is_ascii_digit() {
                    return Err(AwkError::Lex("only $0..$9 are supported"));
                }
                tokens.push(Token::Field(digit - b'0'));
                index += 1;
            }
            b'!' => push_pair(bytes, &mut tokens, &mut index, b'=', Token::Ne, Token::Bang),
            b'=' => expect_pair(
                bytes,
                &mut tokens,
                &mut index,
                b'=',
                Token::EqEq,
                "assignment is not supported",
            )?,
            b'<' => push_pair(bytes, &mut tokens, &mut index, b'=', Token::Le, Token::Lt),
            b'>' => push_pair(bytes, &mut tokens, &mut index, b'=', Token::Ge, Token::Gt),
            b'&' => expect_pair(
                bytes,
                &mut tokens,
                &mut index,
                b'&',
                Token::AndAnd,
                "use &&",
            )?,
            b'|' => expect_pair(bytes, &mut tokens, &mut index, b'|', Token::OrOr, "use ||")?,
            b'"' | b'\'' => tokens.push(read_string(bytes, &mut index)?),
            b'[' | b']' => return Err(AwkError::Lex("arrays are not supported")),
            b'{' | b'}' => return Err(AwkError::Lex("nested actions are not supported")),
            b'0'..=b'9' => tokens.push(read_number(bytes, &mut index)?),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => tokens.push(read_ident(bytes, &mut index)?),
            _ => return Err(AwkError::Lex("unsupported character in awk program")),
        }
    }
    Ok(tokens)
}

fn push(tokens: &mut Vec<Token>, token: Token, index: &mut usize) {
    tokens.push(token);
    *index += 1;
}

fn push_pair(
    bytes: &[u8],
    tokens: &mut Vec<Token>,
    index: &mut usize,
    want: u8,
    yes: Token,
    no: Token,
) {
    *index += 1;
    if bytes.get(*index) == Some(&want) {
        tokens.push(yes);
        *index += 1;
    } else {
        tokens.push(no);
    }
}

fn expect_pair(
    bytes: &[u8],
    tokens: &mut Vec<Token>,
    index: &mut usize,
    want: u8,
    token: Token,
    message: &'static str,
) -> Result<(), AwkError> {
    *index += 1;
    if bytes.get(*index) == Some(&want) {
        tokens.push(token);
        *index += 1;
        Ok(())
    } else {
        Err(AwkError::Lex(message))
    }
}

fn read_string(bytes: &[u8], index: &mut usize) -> Result<Token, AwkError> {
    let quote = bytes[*index];
    *index += 1;
    let start = *index;
    while *index < bytes.len() && bytes[*index] != quote {
        *index += 1;
    }
    if *index >= bytes.len() {
        return Err(AwkError::Lex("unterminated string literal"));
    }
    let text =
        core::str::from_utf8(&bytes[start..*index]).map_err(|_| AwkError::Lex("invalid string"))?;
    if text.len() > MAX_STRING_BYTES {
        return Err(AwkError::Lex("string literal exceeds 128-byte limit"));
    }
    *index += 1;
    Ok(Token::Text(String::from(text)))
}

fn read_number(bytes: &[u8], index: &mut usize) -> Result<Token, AwkError> {
    let start = *index;
    while *index < bytes.len() && bytes[*index].is_ascii_digit() {
        *index += 1;
    }
    let text =
        core::str::from_utf8(&bytes[start..*index]).map_err(|_| AwkError::Lex("invalid number"))?;
    text.parse::<i64>()
        .map(Token::Number)
        .map_err(|_| AwkError::Lex("invalid number"))
}

fn read_ident(bytes: &[u8], index: &mut usize) -> Result<Token, AwkError> {
    let start = *index;
    while *index < bytes.len() && (bytes[*index].is_ascii_alphanumeric() || bytes[*index] == b'_') {
        *index += 1;
    }
    match core::str::from_utf8(&bytes[start..*index])
        .map_err(|_| AwkError::Lex("invalid identifier"))?
    {
        "print" => Ok(Token::Print),
        "NR" => Ok(Token::Nr),
        "NF" => Ok(Token::Nf),
        "BEGIN" | "END" => Err(AwkError::Lex("BEGIN/END are not supported")),
        "for" | "while" | "if" | "function" | "system" => {
            Err(AwkError::Lex("unsupported mini-awk feature"))
        }
        _ => Err(AwkError::Lex("user variables are not supported")),
    }
}

#[cfg(test)]
mod tests {
    use super::{tokenize, Token};

    #[test]
    fn tokenizes_fields_and_ops() {
        let tokens = tokenize("print NR, $1 + 2").expect("tokenize");
        assert!(tokens.contains(&Token::Print));
        assert!(tokens.contains(&Token::Nr));
        // `$1` is a field reference; the trailing `2` is a plain number.
        assert!(tokens.contains(&Token::Field(1)));
        assert!(tokens.contains(&Token::Number(2)));
        assert!(!tokens.contains(&Token::Field(2)));
    }

    #[test]
    fn rejects_field_index_past_the_supported_range() {
        assert!(tokenize("{ print $10 }").is_err());
    }
}
