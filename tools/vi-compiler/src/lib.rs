//! vi-compiler — ViUI `.vi` DSL compiler.
//!
//! Pipeline: source text → [`lexer::tokenize`] → [`parser::parse`] → [`ast::ViFile`]
//!           → [`codegen::CodeGen::generate`] → Rust source string.
//!
//! P03: lexer + parser producing a structural AST.
//! P04: expression evaluator + Rust codegen from AST.

pub mod ast;
pub mod codegen;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod token;

/// One-shot: tokenize + parse a `.vi` source string → AST.
pub fn compile_str(src: &str) -> Result<ast::ViFile, error::ParseError> {
    let tokens = lexer::tokenize(src)?;
    parser::parse(tokens)
}

/// Compile a `.vi` source string all the way to Rust source code.
///
/// Pipeline: source → tokenize → parse → codegen → Rust string.
///
/// Codegen is infallible once parsing succeeds; the only failure modes are
/// lex errors and parse errors, both reported as [`error::ParseError`].
///
/// # Errors
///
/// Returns [`error::ParseError`] with line/column info if tokenization or
/// parsing fails.
pub fn compile(src: &str) -> Result<String, error::ParseError> {
    let file = compile_str(src)?;
    Ok(codegen::CodeGen::new().generate(&file))
}
