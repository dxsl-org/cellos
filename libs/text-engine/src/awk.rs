mod lexer;
mod parser;
mod runtime;

use alloc::string::String;
use alloc::vec::Vec;

use crate::records::RecordError;
use parser::parse;

pub const MAX_PROGRAM_BYTES: usize = 512;

#[derive(Debug)]
pub enum AwkError {
    ProgramTooLong,
    Lex(&'static str),
    Parse(&'static str),
    Runtime(&'static str),
    Record(RecordError),
}

impl AwkError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::ProgramTooLong => "program exceeds 512-byte limit",
            Self::Lex(message) | Self::Parse(message) | Self::Runtime(message) => message,
            Self::Record(RecordError::InputTooLarge) => "input exceeds configured limit",
            Self::Record(RecordError::RecordTooLong) => "record exceeds configured limit",
            Self::Record(RecordError::TooManyRecords) => "record count exceeds configured limit",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Separator<'a> {
    Whitespace,
    Literal(&'a str),
}

impl<'a> Separator<'a> {
    pub fn from_flag(raw: Option<&'a str>) -> Result<Self, AwkError> {
        match raw {
            None => Ok(Self::Whitespace),
            Some("") => Err(AwkError::Runtime("separator may not be empty")),
            Some(value) => Ok(Self::Literal(value)),
        }
    }
}

pub fn execute(
    program: &str,
    separator: Separator<'_>,
    input: &str,
) -> Result<Vec<String>, AwkError> {
    if program.len() > MAX_PROGRAM_BYTES {
        return Err(AwkError::ProgramTooLong);
    }
    let ast = parse(program)?;
    runtime::run(&ast, separator, input)
}

pub fn looks_like_program(arg: &str) -> bool {
    arg.contains(' ')
        || arg.contains('{')
        || arg.contains('}')
        || arg.starts_with("print")
        || arg.contains('$')
        || arg.contains("==")
        || arg.contains("!=")
        || arg.contains("<=")
        || arg.contains(">=")
        || arg.contains("&&")
        || arg.contains("||")
        || arg.contains('(')
        || arg.contains(')')
        || arg.contains('"')
        || arg == "NR"
        || arg == "NF"
}

#[derive(Clone)]
pub(crate) struct Program {
    pub filter: Option<Filter>,
    pub print: Vec<Expr>,
}

#[derive(Clone)]
pub(crate) enum Filter {
    Expr(Expr),
    Regex(String),
}

#[derive(Clone)]
pub(crate) enum Expr {
    Number(i64),
    Text(String),
    Field(u8),
    Builtin(Builtin),
    Unary {
        op: UnaryOp,
        expr: alloc::boxed::Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: alloc::boxed::Box<Expr>,
        rhs: alloc::boxed::Box<Expr>,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum Builtin {
    Nr,
    Nf,
}

#[derive(Clone, Copy)]
pub(crate) enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[cfg(test)]
mod tests {
    use super::{execute, looks_like_program, Separator};

    #[test]
    fn detects_real_programs() {
        assert!(looks_like_program("{ print $1 }"));
        assert!(!looks_like_program("1,3"));
    }

    #[test]
    fn executes_numeric_filter() {
        let output = execute(
            "$2 >= 10 { print NR, $1 }",
            Separator::Literal(","),
            "alice,12\nbob,9",
        )
        .expect("program should succeed");
        assert_eq!(output, ["1 alice"]);
    }

    #[test]
    fn reports_divide_by_zero() {
        let err = execute("{ print 4 / $1 }", Separator::Whitespace, "0").expect_err("must fail");
        assert_eq!(err.message(), "division by zero");
    }
}
