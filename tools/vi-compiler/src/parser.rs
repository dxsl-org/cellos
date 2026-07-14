use crate::ast::{
    BinOpKind, Binding, BindingMode, CallbackBinding, Child, Component, Element, Expr, InterpPart,
    Literal, RawExpr, UnaryOp, ViFile,
};
use crate::error::ParseError;
use crate::token::{Span, Token, TokenKind};
use std::prelude::v1::*;

// ─── Parser ──────────────────────────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Token navigation ─────────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }
    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }
    fn peek_kind_at(&self, offset: usize) -> &TokenKind {
        &self.peek_at(offset).kind
    }

    fn at_eof(&self) -> bool {
        *self.peek_kind() == TokenKind::Eof
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: TokenKind, context: &'static str) -> Result<Token, ParseError> {
        if *self.peek_kind() == kind {
            Ok(self.advance().clone())
        } else {
            Err(self.unexpected(context))
        }
    }

    fn unexpected(&self, expected: &'static str) -> ParseError {
        let tok = self.peek();
        ParseError::UnexpectedToken {
            got: format!("{:?}({})", tok.kind, tok.text),
            expected,
            span: tok.span,
        }
    }

    fn current_span(&self) -> Span {
        self.peek().span
    }

    // ── parse_file ───────────────────────────────────────────────────────────

    pub fn parse_file(&mut self) -> Result<ViFile, ParseError> {
        let mut imports = Vec::new();
        let mut components = Vec::new();

        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::KwImport => imports.push(self.parse_import()?),
                TokenKind::KwComponent => components.push(self.parse_component()?),
                _ => return Err(self.unexpected("import or component")),
            }
        }
        Ok(ViFile {
            imports,
            components,
        })
    }

    // ── parse_import ─────────────────────────────────────────────────────────

    fn parse_import(&mut self) -> Result<Import, ParseError> {
        let span_start = self.current_span();
        self.expect(TokenKind::KwImport, "'import'")?;
        let path_tok = self.expect(TokenKind::StringLit, "file path string")?;
        self.expect(TokenKind::Semicolon, "';'")?;
        Ok(Import {
            path: path_tok.text,
            span: span_start,
        })
    }

    // ── parse_component ──────────────────────────────────────────────────────

    fn parse_component(&mut self) -> Result<Component, ParseError> {
        let span_start = self.current_span();
        self.expect(TokenKind::KwComponent, "'component'")?;
        let name = self.expect(TokenKind::Ident, "component name")?.text;
        self.expect(TokenKind::LBrace, "'{'")?;

        let mut properties = Vec::new();
        let mut callbacks = Vec::new();
        let mut children = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            // Property declaration: visibility? 'property' ...
            if self.is_visibility_or_property() {
                if *self.peek_kind() == TokenKind::KwCallback {
                    callbacks.push(self.parse_callback_decl()?);
                } else {
                    properties.push(self.parse_property_decl()?);
                }
            } else if matches!(
                self.peek_kind(),
                TokenKind::Ident | TokenKind::KwIf | TokenKind::KwFor
            ) {
                children.push(self.parse_child()?);
            } else {
                return Err(self.unexpected("property, callback, or element"));
            }
        }
        self.expect(TokenKind::RBrace, "'}'")?;
        Ok(Component {
            name,
            properties,
            callbacks,
            children,
            span: span_start,
        })
    }

    fn is_visibility_or_property(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::KwIn
                | TokenKind::KwOut
                | TokenKind::KwPrivate
                | TokenKind::KwProperty
                | TokenKind::KwCallback
        )
    }

    // ── parse_visibility ─────────────────────────────────────────────────────

    fn parse_visibility(&mut self) -> Option<Visibility> {
        match self.peek_kind() {
            TokenKind::KwIn => {
                self.advance();
                // in-out: KwIn Minus KwOut
                if *self.peek_kind() == TokenKind::Minus
                    && *self.peek_kind_at(1) == TokenKind::KwOut
                {
                    self.advance(); // consume '-'
                    self.advance(); // consume 'out'
                    Some(Visibility::InOut)
                } else {
                    Some(Visibility::In)
                }
            }
            TokenKind::KwOut => {
                self.advance();
                Some(Visibility::Out)
            }
            TokenKind::KwPrivate => {
                self.advance();
                Some(Visibility::Private)
            }
            _ => None,
        }
    }

    // ── parse_property_decl ──────────────────────────────────────────────────

    fn parse_property_decl(&mut self) -> Result<PropertyDecl, ParseError> {
        let span_start = self.current_span();
        let visibility = self.parse_visibility();
        self.expect(TokenKind::KwProperty, "'property'")?;

        // '<' type '>'
        self.expect(TokenKind::Lt, "'<'")?;
        let ty = self.expect(TokenKind::Ident, "type name")?.text;
        self.expect(TokenKind::Gt, "'>'")?;

        let name = self.expect(TokenKind::Ident, "property name")?.text;

        let default = if *self.peek_kind() == TokenKind::Colon {
            self.advance(); // consume ':'
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenKind::Semicolon, "';'")?;

        Ok(PropertyDecl {
            visibility,
            ty,
            name,
            default,
            span: span_start,
        })
    }

    // ── parse_callback_decl ──────────────────────────────────────────────────

    fn parse_callback_decl(&mut self) -> Result<CallbackDecl, ParseError> {
        let span_start = self.current_span();
        self.expect(TokenKind::KwCallback, "'callback'")?;
        let name = self.expect(TokenKind::Ident, "callback name")?.text;
        let mut params = Vec::new();

        if *self.peek_kind() == TokenKind::LParen {
            self.advance();
            while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
                let pname = self.expect(TokenKind::Ident, "param name")?.text;
                self.expect(TokenKind::Colon, "':'")?;
                let pty = self.expect(TokenKind::Ident, "param type")?.text;
                params.push((pname, pty));
                if *self.peek_kind() == TokenKind::Comma {
                    self.advance();
                }
            }
            self.expect(TokenKind::RParen, "')'")?;
        }
        self.expect(TokenKind::Semicolon, "';'")?;
        Ok(CallbackDecl {
            name,
            params,
            span: span_start,
        })
    }

    // ── parse_child ──────────────────────────────────────────────────────────

    /// Parse one child — either a concrete element or a control-flow node.
    fn parse_child(&mut self) -> Result<Child, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::KwIf => self.parse_if_child(),
            TokenKind::KwFor => self.parse_for_child(),
            _ => Ok(Child::Element(self.parse_element()?)),
        }
    }

    /// Parse `if <cond> { <children> }`.
    fn parse_if_child(&mut self) -> Result<Child, ParseError> {
        let span = self.current_span();
        self.expect(TokenKind::KwIf, "'if'")?;
        let cond = self.parse_until_lbrace()?;
        self.expect(TokenKind::LBrace, "'{'")?;
        let mut body = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            body.push(self.parse_child()?);
        }
        self.expect(TokenKind::RBrace, "'}'")?;
        Ok(Child::If { cond, body, span })
    }

    /// Parse `for <var> in <iter> { <children> }`.
    fn parse_for_child(&mut self) -> Result<Child, ParseError> {
        let span = self.current_span();
        self.expect(TokenKind::KwFor, "'for'")?;
        let var = self.expect(TokenKind::Ident, "loop variable")?.text;
        self.expect(TokenKind::KwIn, "'in'")?;
        let iter = self.parse_until_lbrace()?;
        self.expect(TokenKind::LBrace, "'{'")?;
        let mut body = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            body.push(self.parse_child()?);
        }
        self.expect(TokenKind::RBrace, "'}'")?;
        Ok(Child::For {
            var,
            iter,
            body,
            span,
        })
    }

    /// Collect token texts until `{` at paren/bracket-depth 0, returning them space-joined.
    ///
    /// Does NOT consume the `{`. Returns an error if EOF is reached first.
    fn parse_until_lbrace(&mut self) -> Result<String, ParseError> {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        loop {
            match self.peek_kind().clone() {
                TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    depth -= 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::LBrace if depth == 0 => break,
                TokenKind::LBrace => {
                    depth += 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("'{' after condition"));
                }
                _ => parts.push(self.advance().text.clone()),
            }
        }
        Ok(parts.join(" ").trim().to_string())
    }

    // ── parse_element ────────────────────────────────────────────────────────

    fn parse_element(&mut self) -> Result<Element, ParseError> {
        let span_start = self.current_span();
        let name = self.expect(TokenKind::Ident, "element name")?.text;
        self.expect(TokenKind::LBrace, "'{'")?;

        let mut bindings = Vec::new();
        let mut callbacks = Vec::new();
        let mut children = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            match (self.peek_kind().clone(), self.peek_kind_at(1).clone()) {
                // callback binding: IDENT =>
                (TokenKind::Ident, TokenKind::Arrow) => {
                    callbacks.push(self.parse_callback_binding()?);
                }
                // one-way binding: IDENT :
                (TokenKind::Ident, TokenKind::Colon) => {
                    bindings.push(self.parse_binding()?);
                }
                // two-way binding: IDENT @=
                (TokenKind::Ident, TokenKind::TwoWayBind) => {
                    bindings.push(self.parse_binding()?);
                }
                // computed binding: IDENT #=
                (TokenKind::Ident, TokenKind::ComputedBind) => {
                    bindings.push(self.parse_binding()?);
                }
                // child element or control-flow node
                (TokenKind::Ident, TokenKind::LBrace)
                | (TokenKind::KwIf, _)
                | (TokenKind::KwFor, _) => {
                    children.push(self.parse_child()?);
                }
                _ => return Err(self.unexpected("binding, callback, or child element")),
            }
        }
        self.expect(TokenKind::RBrace, "'}'")?;
        Ok(Element {
            name,
            bindings,
            callbacks,
            children,
            span: span_start,
        })
    }

    // ── parse_binding ────────────────────────────────────────────────────────

    fn parse_binding(&mut self) -> Result<Binding, ParseError> {
        let span_start = self.current_span();
        let property = self.expect(TokenKind::Ident, "property name")?.text;

        // Determine binding mode from the operator token.
        let mode = match self.peek_kind() {
            TokenKind::Colon => {
                self.advance();
                BindingMode::OneWay
            }
            TokenKind::TwoWayBind => {
                self.advance();
                BindingMode::TwoWay
            }
            TokenKind::ComputedBind => {
                self.advance();
                BindingMode::Computed
            }
            _ => return Err(self.unexpected("':', '@=', or '#='")),
        };

        let value = self.parse_expr();
        self.expect(TokenKind::Semicolon, "';'")?;
        Ok(Binding {
            property,
            mode,
            value,
            span: span_start,
        })
    }

    // ── parse_callback_binding ───────────────────────────────────────────────

    fn parse_callback_binding(&mut self) -> Result<CallbackBinding, ParseError> {
        let span_start = self.current_span();
        let name = self.expect(TokenKind::Ident, "callback name")?.text;
        self.expect(TokenKind::Arrow, "'=>'")?;
        self.expect(TokenKind::LBrace, "'{'")?;
        let body = self.parse_raw_body();
        self.expect(TokenKind::RBrace, "'}'")?;
        // Optional trailing semicolon
        if *self.peek_kind() == TokenKind::Semicolon {
            self.advance();
        }
        Ok(CallbackBinding {
            name,
            body,
            span: span_start,
        })
    }

    // ── Typed expression parser ──────────────────────────────────────────────

    /// Entry point: parse a typed expression, consuming tokens until `;` or `}` at depth 0.
    ///
    /// Returns a typed `Expr` variant for recognised patterns; falls back to
    /// `Expr::Raw` for complex / unrecognised inputs so all existing `.vi` files
    /// continue to compile.
    fn parse_expr(&mut self) -> Expr {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Expr {
        let cond = self.parse_or();
        if *self.peek_kind() == TokenKind::Question {
            self.advance(); // consume '?'
            let then_expr = self.parse_or();
            // Consume ':' separating then/else branches.
            // ':' is also used as a binding operator, but at this point we are
            // inside an expression already (past the property name + ':' consumed
            // by parse_binding), so the next ':' unambiguously belongs to ternary.
            if *self.peek_kind() == TokenKind::Colon {
                self.advance(); // consume ':'
            }
            let else_expr = self.parse_or();
            return Expr::Ternary(Box::new(cond), Box::new(then_expr), Box::new(else_expr));
        }
        cond
    }

    fn parse_or(&mut self) -> Expr {
        let mut lhs = self.parse_and();
        while *self.peek_kind() == TokenKind::Or {
            self.advance();
            let rhs = self.parse_and();
            lhs = Expr::BinOp(Box::new(lhs), BinOpKind::Or, Box::new(rhs));
        }
        lhs
    }

    fn parse_and(&mut self) -> Expr {
        let mut lhs = self.parse_eq();
        while *self.peek_kind() == TokenKind::And {
            self.advance();
            let rhs = self.parse_eq();
            lhs = Expr::BinOp(Box::new(lhs), BinOpKind::And, Box::new(rhs));
        }
        lhs
    }

    fn parse_eq(&mut self) -> Expr {
        let mut lhs = self.parse_cmp();
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqEq => BinOpKind::Eq,
                TokenKind::BangEq => BinOpKind::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_cmp();
            lhs = Expr::BinOp(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_cmp(&mut self) -> Expr {
        let mut lhs = self.parse_add();
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinOpKind::Lt,
                TokenKind::Gt => BinOpKind::Gt,
                TokenKind::LtEq => BinOpKind::Le,
                TokenKind::GtEq => BinOpKind::Ge,
                _ => break,
            };
            // Peek further: '<' and '>' are also used in property type angles.
            // Heuristic: if the token after '<'/'>' is an Ident followed by '>', it's
            // a type angle — let the caller handle it.  We already consumed past
            // `parse_primary` so this ambiguity only arises inside a nested expr.
            self.advance();
            let rhs = self.parse_add();
            lhs = Expr::BinOp(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_add(&mut self) -> Expr {
        let mut lhs = self.parse_mul();
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOpKind::Add,
                TokenKind::Minus => BinOpKind::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul();
            lhs = Expr::BinOp(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_mul(&mut self) -> Expr {
        let mut lhs = self.parse_unary_expr();
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOpKind::Mul,
                TokenKind::Slash => BinOpKind::Div,
                // '%' is not in TokenKind yet; skip Rem
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary_expr();
            lhs = Expr::BinOp(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_unary_expr(&mut self) -> Expr {
        match self.peek_kind() {
            TokenKind::Bang => {
                self.advance();
                Expr::Unary(UnaryOp::Not, Box::new(self.parse_unary_expr()))
            }
            TokenKind::Minus => {
                // Disambiguate: `-` before a digit is part of a negative literal.
                if matches!(
                    self.peek_kind_at(1),
                    TokenKind::IntLit | TokenKind::FloatLit
                ) {
                    self.advance(); // consume '-'
                    match self.peek_kind().clone() {
                        TokenKind::IntLit => {
                            let tok = self.advance();
                            let n: i64 = tok.text.parse().unwrap_or(0);
                            Expr::Literal(Literal::Int(-n))
                        }
                        TokenKind::FloatLit => {
                            let tok = self.advance();
                            let f: f64 = tok.text.parse().unwrap_or(0.0);
                            Expr::Literal(Literal::Float(-f))
                        }
                        _ => unreachable!(),
                    }
                } else {
                    self.advance();
                    Expr::Unary(UnaryOp::Neg, Box::new(self.parse_unary_expr()))
                }
            }
            _ => self.parse_primary(),
        }
    }

    /// Parse a primary expression: literal / ident / self.prop / fn-call / string / group.
    ///
    /// Falls back to raw token collection when the leading token is not recognised
    /// as a primary-expression start.
    fn parse_primary(&mut self) -> Expr {
        match self.peek_kind().clone() {
            TokenKind::KwTrue => {
                self.advance();
                Expr::Literal(Literal::Bool(true))
            }
            TokenKind::KwFalse => {
                self.advance();
                Expr::Literal(Literal::Bool(false))
            }

            TokenKind::IntLit => {
                let tok = self.advance().clone();
                let n: i64 = tok.text.parse().unwrap_or(0);
                Expr::Literal(Literal::Int(n))
            }

            TokenKind::FloatLit => {
                let tok = self.advance().clone();
                let f: f64 = tok.text.parse().unwrap_or(0.0);
                Expr::Literal(Literal::Float(f))
            }

            TokenKind::StringLit => {
                let tok = self.advance().clone();
                self.parse_string_lit(&tok.text, tok.span)
            }

            TokenKind::Ident => {
                let span = self.current_span();
                let name = self.advance().text.clone();

                // `self.prop` → SelfProp
                if name == "self" && *self.peek_kind() == TokenKind::Dot {
                    self.advance(); // consume '.'
                    if let TokenKind::Ident = self.peek_kind() {
                        let prop = self.advance().text.clone();
                        return Expr::SelfProp(prop);
                    } else {
                        // `self.` followed by non-ident — fall back to raw
                        let raw_rest = self.collect_raw_until_semi();
                        return Expr::Raw(RawExpr {
                            text: format!("self . {}", raw_rest),
                            span,
                        });
                    }
                }

                // `name(args...)` → FnCall
                if *self.peek_kind() == TokenKind::LParen {
                    self.advance(); // consume '('
                    let mut args = Vec::new();
                    while !matches!(
                        self.peek_kind(),
                        TokenKind::RParen | TokenKind::Eof | TokenKind::Semicolon
                    ) {
                        args.push(self.parse_expr());
                        if *self.peek_kind() == TokenKind::Comma {
                            self.advance();
                        }
                    }
                    if *self.peek_kind() == TokenKind::RParen {
                        self.advance();
                    }
                    return Expr::FnCall(name, args);
                }

                // `.prop` chained on bare ident — not a self-prop, fall back raw
                if *self.peek_kind() == TokenKind::Dot {
                    let raw_rest = self.collect_raw_until_semi();
                    return Expr::Raw(RawExpr {
                        text: format!("{} . {}", name, raw_rest),
                        span,
                    });
                }

                Expr::Ident(name)
            }

            // Grouped expression
            TokenKind::LParen => {
                self.advance(); // consume '('
                let inner = self.parse_expr();
                if *self.peek_kind() == TokenKind::RParen {
                    self.advance();
                }
                inner
            }

            // Anything unrecognised — collect as Raw
            _ => {
                let span = self.current_span();
                let text = self.collect_raw_until_semi();
                Expr::Raw(RawExpr { text, span })
            }
        }
    }

    /// Parse a string literal token, handling `\{expr}` interpolation.
    fn parse_string_lit(&self, content: &str, _span: Span) -> Expr {
        // Check for interpolation markers `\{`
        if !content.contains("\\{") {
            return Expr::Literal(Literal::Str(content.to_string()));
        }

        // Split into parts: text and \{...} segments
        let bytes = content.as_bytes();
        let mut parts: Vec<InterpPart> = Vec::new();
        let mut i = 0usize;
        let mut lit_start = 0usize;

        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == b'\\' && bytes[i + 1] == b'{' {
                // Flush accumulated literal
                if i > lit_start {
                    parts.push(InterpPart::Lit(content[lit_start..i].to_string()));
                }
                i += 2; // skip `\{`
                let var_start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let var_name = content[var_start..i].trim().to_string();
                if i < bytes.len() {
                    i += 1;
                } // skip `}`
                lit_start = i;
                // Emit a SelfProp or Ident depending on the var name content
                let expr = if var_name.starts_with("self.") {
                    Expr::SelfProp(var_name[5..].to_string())
                } else {
                    Expr::Ident(var_name)
                };
                parts.push(InterpPart::Expr(Box::new(expr)));
            } else {
                i += 1;
            }
        }

        // Flush trailing literal
        if lit_start < bytes.len() {
            parts.push(InterpPart::Lit(content[lit_start..].to_string()));
        }

        if parts.iter().all(|p| matches!(p, InterpPart::Lit(_))) {
            // Only literal parts — no real interpolation found
            Expr::Literal(Literal::Str(content.to_string()))
        } else {
            Expr::Interpolated(parts)
        }
    }

    /// Collect remaining tokens (until `;` or `}` at depth 0) as a raw text string.
    ///
    /// Used as fallback inside typed expression parsers when they encounter
    /// something they cannot model.
    fn collect_raw_until_semi(&mut self) -> String {
        let mut parts = Vec::<String>::new();
        let mut depth = 0i32;
        loop {
            match self.peek_kind().clone() {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::Semicolon if depth == 0 => break,
                TokenKind::Eof => break,
                TokenKind::StringLit => parts.push(format!("\"{}\"", self.advance().text)),
                _ => parts.push(self.advance().text.clone()),
            }
        }
        parts.join(" ")
    }

    // ── Expression helpers ───────────────────────────────────────────────────

    /// Collect tokens until ';' at brace-depth 0 (does NOT consume the ';').
    ///
    /// Kept for compatibility; new callers should use `parse_expr()` for typed AST nodes.
    #[allow(dead_code)]
    fn parse_expr_raw_until_semi(&mut self) -> Expr {
        let span_start = self.current_span();
        let mut parts = Vec::<String>::new();
        let mut depth = 0i32;
        loop {
            match self.peek_kind().clone() {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::Semicolon if depth == 0 => break,
                TokenKind::Eof => break,
                // Preserve quotes so eval_binding can distinguish StringLit from Ident
                TokenKind::StringLit => parts.push(format!("\"{}\"", self.advance().text)),
                _ => parts.push(self.advance().text.clone()),
            }
        }
        Expr::Raw(RawExpr {
            text: parts.join(" "),
            span: span_start,
        })
    }

    /// Collect tokens until the closing '}' at depth 0 (does NOT consume '}'.
    fn parse_raw_body(&mut self) -> String {
        let mut parts = Vec::<String>::new();
        let mut depth = 0i32;
        loop {
            match self.peek_kind().clone() {
                TokenKind::LBrace => {
                    depth += 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    parts.push(self.advance().text.clone());
                }
                TokenKind::Eof => break,
                _ => parts.push(self.advance().text.clone()),
            }
        }
        parts.join(" ").trim().to_string()
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a token list (from `tokenize()`) into a `ViFile` AST.
pub fn parse(tokens: Vec<Token>) -> Result<ViFile, ParseError> {
    Parser::new(tokens).parse_file()
}

// Re-export ast types used in parser for import hygiene
use crate::ast::{CallbackDecl, Import, PropertyDecl, Visibility};
