use crate::token::Span;
use std::prelude::v1::*;

// ─── Top-level ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ViFile {
    pub imports: Vec<Import>,
    pub components: Vec<Component>,
}

#[derive(Debug)]
pub struct Import {
    pub path: String,
    pub span: Span,
}

// ─── Component ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Component {
    pub name: String,
    pub properties: Vec<PropertyDecl>,
    pub callbacks: Vec<CallbackDecl>,
    pub children: Vec<Child>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    In,
    Out,
    InOut,
    Private,
}

#[derive(Debug)]
pub struct PropertyDecl {
    pub visibility: Option<Visibility>,
    pub ty: String,
    pub name: String,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct CallbackDecl {
    pub name: String,
    pub params: Vec<(String, String)>, // (param_name, type_name)
    pub span: Span,
}

// ─── Element ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Element {
    pub name: String,
    pub bindings: Vec<Binding>,
    pub callbacks: Vec<CallbackBinding>,
    pub children: Vec<Child>,
    pub span: Span,
}

/// A child within a component or element body — either a concrete element
/// or a control-flow construct (`if` / `for`).
#[derive(Debug)]
pub enum Child {
    /// A concrete widget element with bindings and children.
    Element(Element),
    /// Conditional rendering: `if condition { ... }`
    If {
        /// Raw condition expression (may contain `self.X` property refs).
        cond: String,
        body: Vec<Child>,
        span: Span,
    },
    /// Loop rendering: `for var in iter { ... }`
    For {
        var: String,
        iter: String,
        body: Vec<Child>,
        span: Span,
    },
}

/// Binding operator mode — controls how a widget property is wired to a value.
#[derive(Debug, Clone, PartialEq)]
pub enum BindingMode {
    /// `property: expr` — one-way read (default).
    OneWay,
    /// `property @= expr` — two-way: widget writes back to the source signal.
    TwoWay,
    /// `property #= expr` — computed/derived: auto-recomputes when dependencies change.
    Computed,
}

#[derive(Debug)]
pub struct Binding {
    pub property: String,
    pub mode: BindingMode,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug)]
pub struct CallbackBinding {
    pub name: String,
    pub body: String, // raw source text between '{ ' and ' }'
    pub span: Span,
}

// ─── Expressions ─────────────────────────────────────────────────────────────

/// Raw source text fallback — used when the expression is too complex for typed parsing.
#[derive(Debug)]
pub struct RawExpr {
    pub text: String,
    pub span: Span,
}

/// Typed boolean / integer / float / string literal.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

/// Binary operator kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum BinOpKind {
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

impl BinOpKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinOpKind::Add => "+",
            BinOpKind::Sub => "-",
            BinOpKind::Mul => "*",
            BinOpKind::Div => "/",
            BinOpKind::Rem => "%",
            BinOpKind::Eq => "==",
            BinOpKind::Ne => "!=",
            BinOpKind::Lt => "<",
            BinOpKind::Le => "<=",
            BinOpKind::Gt => ">",
            BinOpKind::Ge => ">=",
            BinOpKind::And => "&&",
            BinOpKind::Or => "||",
        }
    }
}

/// Unary operator kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

/// Part of a string interpolation like `"Speed: \{speed} rpm"`.
#[derive(Debug)]
pub enum InterpPart {
    /// Plain text segment.
    Lit(String),
    /// Interpolated expression `\{expr}`.
    Expr(Box<Expr>),
}

/// Typed expression AST node.
///
/// `Raw` is kept as a fallback for complex expressions the parser does not fully
/// model. All new patterns produce typed variants so codegen can reason about them.
#[derive(Debug)]
pub enum Expr {
    /// Untyped fallback — raw source tokens joined by spaces.
    Raw(RawExpr),
    /// A literal value: `true`, `42`, `3.14`, `"text"`.
    Literal(Literal),
    /// Bare identifier: `count`, `items`.
    Ident(String),
    /// Property reference on `self`: `self.count` → `SelfProp("count")`.
    SelfProp(String),
    /// Binary expression: `a + b`, `x == y`.
    BinOp(Box<Expr>, BinOpKind, Box<Expr>),
    /// Unary expression: `!flag`, `-n`.
    Unary(UnaryOp, Box<Expr>),
    /// Ternary expression: `cond ? then : else`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// String with `\{...}` interpolation segments: `"Count: \{count}"`.
    Interpolated(Vec<InterpPart>),
    /// Function call: `min(a, b)`.
    FnCall(String, Vec<Expr>),
}
