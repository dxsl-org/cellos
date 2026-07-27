use alloc::string::String;
use alloc::vec::Vec;

use super::lexer::{tokenize, Token};
use super::{AwkError, BinaryOp, Builtin, Expr, Filter, Program, UnaryOp};

const MAX_EXPR_DEPTH: usize = 24;
const MAX_PRINT_ITEMS: usize = 16;

pub(crate) fn parse(program: &str) -> Result<Program, AwkError> {
    let (filter_src, action_src) = split_program(program)?;
    let filter = match filter_src {
        Some(source) if is_regex_filter(source) => {
            Some(Filter::Regex(String::from(&source[1..source.len() - 1])))
        }
        Some(source) => Some(Filter::Expr(parse_expr_tokens(&tokenize(source)?)?)),
        None => None,
    };
    let print = parse_print_list(&tokenize(action_src)?)?;
    Ok(Program { filter, print })
}

fn split_program(program: &str) -> Result<(Option<&str>, &str), AwkError> {
    let text = program.trim();
    if let Some(open) = text.find('{') {
        let close = text
            .rfind('}')
            .ok_or(AwkError::Parse("missing closing }"))?;
        if close <= open || !text[close + 1..].trim().is_empty() {
            return Err(AwkError::Parse(
                "awk program must contain one top-level action block",
            ));
        }
        let filter = text[..open].trim();
        let action = text[open + 1..close].trim();
        if action.contains('{') || action.contains('}') {
            return Err(AwkError::Parse("nested actions are not supported"));
        }
        return Ok((
            if filter.is_empty() {
                None
            } else {
                Some(filter)
            },
            action,
        ));
    }
    Ok((None, text))
}

fn is_regex_filter(source: &str) -> bool {
    let trimmed = source.trim();
    trimmed.len() >= 2 && trimmed.starts_with('/') && trimmed.ends_with('/')
}

fn parse_expr_tokens(tokens: &[Token]) -> Result<Expr, AwkError> {
    let mut parser = Parser::new(tokens);
    let expr = parser.expr(0, 0)?;
    parser.finish()?;
    Ok(expr)
}

fn parse_print_list(tokens: &[Token]) -> Result<Vec<Expr>, AwkError> {
    let mut parser = Parser::new(tokens);
    parser.expect_print()?;
    let mut items = Vec::new();
    if parser.done() {
        return Ok(items);
    }
    loop {
        if items.len() >= MAX_PRINT_ITEMS {
            return Err(AwkError::Parse("print supports at most 16 expressions"));
        }
        items.push(parser.expr(0, 0)?);
        if !parser.take_comma() {
            break;
        }
    }
    parser.finish()?;
    Ok(items)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, index: 0 }
    }

    fn done(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn finish(&self) -> Result<(), AwkError> {
        if self.done() {
            Ok(())
        } else {
            Err(AwkError::Parse("unexpected trailing tokens"))
        }
    }

    fn expect_print(&mut self) -> Result<(), AwkError> {
        match self.bump() {
            Some(Token::Print) => Ok(()),
            _ => Err(AwkError::Parse("only print actions are supported")),
        }
    }

    fn take_comma(&mut self) -> bool {
        matches!(self.peek(), Some(Token::Comma)) && {
            self.index += 1;
            true
        }
    }

    fn expr(&mut self, min_bp: u8, depth: usize) -> Result<Expr, AwkError> {
        if depth >= MAX_EXPR_DEPTH {
            return Err(AwkError::Parse("expression nesting exceeds limit"));
        }
        let mut lhs = match self.bump() {
            Some(Token::Number(value)) => Expr::Number(*value),
            Some(Token::Text(text)) => Expr::Text(text.clone()),
            Some(Token::Field(field)) => Expr::Field(*field),
            Some(Token::Nr) => Expr::Builtin(Builtin::Nr),
            Some(Token::Nf) => Expr::Builtin(Builtin::Nf),
            Some(Token::Minus) => Expr::Unary {
                op: UnaryOp::Neg,
                expr: alloc::boxed::Box::new(self.expr(9, depth + 1)?),
            },
            Some(Token::Bang) => Expr::Unary {
                op: UnaryOp::Not,
                expr: alloc::boxed::Box::new(self.expr(9, depth + 1)?),
            },
            Some(Token::LParen) => {
                let expr = self.expr(0, depth + 1)?;
                match self.bump() {
                    Some(Token::RParen) => expr,
                    _ => return Err(AwkError::Parse("missing closing )")),
                }
            }
            _ => return Err(AwkError::Parse("expected expression")),
        };
        while let Some((left_bp, right_bp, op)) = self.peek_op() {
            if left_bp < min_bp {
                break;
            }
            self.index += 1;
            let rhs = self.expr(right_bp, depth + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: alloc::boxed::Box::new(lhs),
                rhs: alloc::boxed::Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.index)
    }

    fn peek_op(&self) -> Option<(u8, u8, BinaryOp)> {
        match self.peek()? {
            Token::OrOr => Some((1, 2, BinaryOp::Or)),
            Token::AndAnd => Some((3, 4, BinaryOp::And)),
            Token::EqEq => Some((5, 6, BinaryOp::Eq)),
            Token::Ne => Some((5, 6, BinaryOp::Ne)),
            Token::Lt => Some((5, 6, BinaryOp::Lt)),
            Token::Le => Some((5, 6, BinaryOp::Le)),
            Token::Gt => Some((5, 6, BinaryOp::Gt)),
            Token::Ge => Some((5, 6, BinaryOp::Ge)),
            Token::Plus => Some((7, 8, BinaryOp::Add)),
            Token::Minus => Some((7, 8, BinaryOp::Sub)),
            Token::Star => Some((9, 10, BinaryOp::Mul)),
            Token::Slash => Some((9, 10, BinaryOp::Div)),
            Token::Percent => Some((9, 10, BinaryOp::Rem)),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "parser/tests.rs"]
mod tests;
