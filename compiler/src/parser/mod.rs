use crate::ast::*;
use crate::lexer::{DurationUnitTok, SpannedToken, Token};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("expected {expected}, found {found:?} at {span:?}")]
    Expected {
        expected: String,
        found: Token,
        span: Span,
    },
    #[error("unexpected end of input")]
    UnexpectedEof,
}

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut imports = Vec::new();
        let mut cells = Vec::new();

        // Parse use statements at the top
        // Syntax: use pkg::math       → package import
        //         use lib::helpers     → local directory
        //         use std::builtins    → stdlib
        //         use math             → shorthand for pkg::math
        //         use "path/file.cell" → legacy quoted path (still supported)
        while self.check(&Token::Use) {
            self.advance();
            let tok = &self.tokens[self.pos];
            if let Token::StringLit(path) = &tok.token {
                // Legacy: quoted string path
                imports.push(path.clone());
                self.advance();
            } else {
                // New: bare identifier path with :: separator
                let mut parts = Vec::new();
                let (first, _) = self.expect_any_name()?;
                parts.push(first);

                // Parse ::segment or /segment
                loop {
                    if self.pos + 1 < self.tokens.len() {
                        let t = &self.tokens[self.pos].token;
                        if matches!(t, Token::Colon) {
                            // Check for ::
                            if let Token::Colon = &self.tokens[self.pos + 1].token {
                                self.advance(); // first :
                                self.advance(); // second :
                                let (seg, _) = self.expect_any_name()?;
                                parts.push(seg);
                                continue;
                            }
                        }
                        if matches!(t, Token::Dot) {
                            self.advance();
                            let (seg, _) = self.expect_any_name()?;
                            parts.push(seg);
                            continue;
                        }
                        if matches!(t, Token::Slash) {
                            self.advance();
                            let (seg, _) = self.expect_any_name()?;
                            parts.push(seg);
                            continue;
                        }
                    }
                    break;
                }

                // Convert to import path
                let import = if parts.len() == 1 {
                    // use math → pkg:math
                    format!("pkg:{}", parts[0])
                } else {
                    let prefix = &parts[0];
                    let rest = parts[1..].join("/");
                    match prefix.as_str() {
                        "pkg" => format!("pkg:{}", rest),
                        "std" => format!("std:{}", rest),
                        "lib" => format!("lib:{}", rest),
                        _ => {
                            // Treat as path: use foo::bar → foo/bar
                            parts.join("/")
                        }
                    }
                };
                imports.push(import);
            }
        }

        while !self.is_at_end() {
            cells.push(self.parse_cell_def()?);
        }
        Ok(Program { imports, cells })
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn advance(&mut self) -> &SpannedToken {
        let tok = &self.tokens[self.pos];
        if !self.is_at_end() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<Span, ParseError> {
        let tok = &self.tokens[self.pos];
        if std::mem::discriminant(&tok.token) == std::mem::discriminant(&expected) {
            let span = tok.span;
            self.advance();
            Ok(span)
        } else {
            Err(ParseError::Expected {
                expected: format!("{:?}", expected),
                found: tok.token.clone(),
                span: tok.span,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), ParseError> {
        let tok = &self.tokens[self.pos];
        match &tok.token {
            Token::Ident(name) | Token::TypeIdent(name) => {
                let name = name.clone();
                let span = tok.span;
                self.advance();
                Ok((name, span))
            }
            _ => Err(ParseError::Expected {
                expected: "identifier".to_string(),
                found: tok.token.clone(),
                span: tok.span,
            }),
        }
    }

    fn expect_type_ident(&mut self) -> Result<(String, Span), ParseError> {
        let tok = &self.tokens[self.pos];
        match &tok.token {
            Token::TypeIdent(name) => {
                let name = name.clone();
                let span = tok.span;
                self.advance();
                Ok((name, span))
            }
            _ => Err(ParseError::Expected {
                expected: "type name".to_string(),
                found: tok.token.clone(),
                span: tok.span,
            }),
        }
    }

    /// Accept any token as a name (for backends/builtins where names like "memory" or "file" are keywords)
    fn expect_any_name(&mut self) -> Result<(String, Span), ParseError> {
        let tok = &self.tokens[self.pos];
        let span = tok.span;
        let name = match &tok.token {
            Token::Ident(n) | Token::TypeIdent(n) => n.clone(),
            // Accept keywords as names for backend/builtin definitions
            Token::Memory => "memory".to_string(),
            Token::Face => "face".to_string(),
            Token::Cell => "cell".to_string(),
            Token::Signal => "signal".to_string(),
            Token::Check => "check".to_string(),
            Token::Type => "type".to_string(),
            Token::Property => "property".to_string(),
            _ => {
                return Err(ParseError::Expected {
                    expected: "name".to_string(),
                    found: tok.token.clone(),
                    span,
                });
            }
        };
        self.advance();
        Ok((name, span))
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(token)
    }

    fn prev_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1)].span
    }

    // ── Cell ─────────────────────────────────────────────────────────

    fn parse_cell_def(&mut self) -> Result<Spanned<CellDef>, ParseError> {
        let start = self.peek_span();
        self.expect(Token::Cell)?;

        // Check for meta-cell kind: cell property, cell type, cell checker
        let (kind, name, type_params) = match self.peek() {
            Token::Property => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                (CellKind::Property, name, vec![])
            }
            Token::Type => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                // Parse optional type parameters: <T, U>
                let type_params = if self.check(&Token::Lt) {
                    self.advance();
                    let mut params = Vec::new();
                    let (p, _) = self.expect_ident()?;
                    params.push(p);
                    while self.check(&Token::Comma) {
                        self.advance();
                        let (p, _) = self.expect_ident()?;
                        params.push(p);
                    }
                    self.expect(Token::Gt)?;
                    params
                } else {
                    vec![]
                };
                (CellKind::Type, name, type_params)
            }
            Token::Checker => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                (CellKind::Checker, name, vec![])
            }
            Token::Backend => {
                self.advance();
                let (name, _) = self.expect_any_name()?;
                (CellKind::Backend, name, vec![])
            }
            Token::Builtin => {
                self.advance();
                let (name, _) = self.expect_any_name()?;
                (CellKind::Builtin, name, vec![])
            }
            Token::Test => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                (CellKind::Test, name, vec![])
            }
            _ => {
                let (name, _) = self.expect_ident()?;
                (CellKind::Cell, name, vec![])
            }
        };

        self.expect(Token::LBrace)?;

        let mut sections = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            sections.push(self.parse_section()?);
        }

        let end = self.peek_span();
        self.expect(Token::RBrace)?;

        Ok(Spanned::new(
            CellDef {
                kind,
                name,
                type_params,
                sections,
            },
            start.merge(end),
        ))
    }

    fn parse_section(&mut self) -> Result<Spanned<Section>, ParseError> {
        let start = self.peek_span();
        match self.peek() {
            Token::Face => {
                let face = self.parse_face_section()?;
                Ok(Spanned::new(Section::Face(face), start.merge(self.prev_span())))
            }
            Token::Memory => {
                let mem = self.parse_memory_section()?;
                Ok(Spanned::new(Section::Memory(mem), start.merge(self.prev_span())))
            }
            Token::Interior => {
                let interior = self.parse_interior_section()?;
                Ok(Spanned::new(Section::Interior(interior), start.merge(self.prev_span())))
            }
            Token::On => {
                let on = self.parse_on_section()?;
                Ok(Spanned::new(Section::OnSignal(on), start.merge(self.prev_span())))
            }
            Token::Rules => {
                let rules = self.parse_rules_section()?;
                Ok(Spanned::new(Section::Rules(rules), start.merge(self.prev_span())))
            }
            Token::Runtime => {
                let rt = self.parse_runtime_section()?;
                Ok(Spanned::new(Section::Runtime(rt), start.merge(self.prev_span())))
            }
            _ => Err(ParseError::Expected {
                expected: "face, memory, interior, on, rules, or runtime".to_string(),
                found: self.peek().clone(),
                span: self.peek_span(),
            }),
        }
    }

    // ── Face ─────────────────────────────────────────────────────────

    fn parse_face_section(&mut self) -> Result<FaceSection, ParseError> {
        self.expect(Token::Face)?;
        self.expect(Token::LBrace)?;

        let mut declarations = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            declarations.push(self.parse_face_decl()?);
        }

        self.expect(Token::RBrace)?;
        Ok(FaceSection { declarations })
    }

    fn parse_face_decl(&mut self) -> Result<Spanned<FaceDecl>, ParseError> {
        let start = self.peek_span();
        match self.peek() {
            Token::Given => {
                let decl = self.parse_given()?;
                Ok(Spanned::new(FaceDecl::Given(decl), start.merge(self.prev_span())))
            }
            Token::Promise => {
                let decl = self.parse_promise()?;
                Ok(Spanned::new(FaceDecl::Promise(decl), start.merge(self.prev_span())))
            }
            Token::Signal => {
                let decl = self.parse_signal_decl()?;
                Ok(Spanned::new(FaceDecl::Signal(decl), start.merge(self.prev_span())))
            }
            Token::Await => {
                let decl = self.parse_await_decl()?;
                Ok(Spanned::new(FaceDecl::Await(decl), start.merge(self.prev_span())))
            }
            _ => Err(ParseError::Expected {
                expected: "given, promise, signal, or await".to_string(),
                found: self.peek().clone(),
                span: self.peek_span(),
            }),
        }
    }

    fn parse_given(&mut self) -> Result<GivenDecl, ParseError> {
        self.expect(Token::Given)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let ty = self.parse_type_expr()?;

        let where_clause = if self.check(&Token::Where) {
            self.advance();
            self.expect(Token::LBrace)?;
            let mut constraints = Vec::new();
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                constraints.push(self.parse_constraint()?);
                if self.check(&Token::Comma) {
                    self.advance();
                }
            }
            self.expect(Token::RBrace)?;
            Some(constraints)
        } else {
            None
        };

        Ok(GivenDecl {
            name,
            ty,
            where_clause,
        })
    }

    fn parse_promise(&mut self) -> Result<PromiseDecl, ParseError> {
        self.expect(Token::Promise)?;

        if let Token::StringLit(_) = self.peek() {
            let tok = self.advance().clone();
            if let Token::StringLit(s) = &tok.token {
                return Ok(PromiseDecl {
                    constraint: Spanned::new(Constraint::Descriptive(s.clone()), tok.span),
                });
            }
        }

        let constraint = self.parse_constraint()?;
        Ok(PromiseDecl { constraint })
    }

    fn parse_signal_decl(&mut self) -> Result<SignalDecl, ParseError> {
        self.expect(Token::Signal)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(Token::RParen)?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        Ok(SignalDecl {
            name,
            params,
            return_type,
        })
    }

    fn parse_await_decl(&mut self) -> Result<AwaitDecl, ParseError> {
        self.expect(Token::Await)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(Token::RParen)?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        Ok(AwaitDecl {
            name,
            params,
            return_type,
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.check(&Token::RParen) {
            return Ok(params);
        }

        params.push(self.parse_param()?);
        while self.check(&Token::Comma) {
            self.advance();
            if self.check(&Token::RParen) {
                break;
            }
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param, ParseError> {
        let (name, _) = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let ty = self.parse_type_expr()?;
        Ok(Param { name, ty })
    }

    // ── Memory ───────────────────────────────────────────────────────

    fn parse_memory_section(&mut self) -> Result<MemorySection, ParseError> {
        self.expect(Token::Memory)?;
        self.expect(Token::LBrace)?;

        let mut slots = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            slots.push(self.parse_memory_slot()?);
        }

        self.expect(Token::RBrace)?;
        Ok(MemorySection { slots })
    }

    fn parse_memory_slot(&mut self) -> Result<Spanned<MemorySlot>, ParseError> {
        let start = self.peek_span();
        let (name, _) = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let ty = self.parse_type_expr()?;
        self.expect(Token::LBracket)?;

        let mut properties = Vec::new();
        while !self.check(&Token::RBracket) && !self.is_at_end() {
            properties.push(self.parse_memory_property()?);
            if self.check(&Token::Comma) {
                self.advance();
            }
        }

        let end = self.peek_span();
        self.expect(Token::RBracket)?;

        Ok(Spanned::new(
            MemorySlot {
                name,
                ty,
                properties,
            },
            start.merge(end),
        ))
    }

    fn parse_memory_property(&mut self) -> Result<Spanned<MemoryProperty>, ParseError> {
        let (name, span) = self.expect_ident()?;

        // Check if it's a parameterized property: name(value, value, ...)
        if self.check(&Token::LParen) {
            self.advance();
            let mut values = Vec::new();
            if !self.check(&Token::RParen) {
                values.push(self.parse_property_value()?);
                while self.check(&Token::Comma) {
                    self.advance();
                    if self.check(&Token::RParen) {
                        break;
                    }
                    values.push(self.parse_property_value()?);
                }
            }
            let end = self.peek_span();
            self.expect(Token::RParen)?;
            return Ok(Spanned::new(
                MemoryProperty::Param(PropertyParam { name, values }),
                span.merge(end),
            ));
        }

        // Flag property — ANY identifier is valid, checked against registry later
        Ok(Spanned::new(MemoryProperty::Flag(name), span))
    }

    /// Parse a property parameter value: literal or identifier (for enum values like `lru`)
    fn parse_property_value(&mut self) -> Result<Spanned<Literal>, ParseError> {
        if let Token::Ident(name) = self.peek().clone() {
            let span = self.peek_span();
            self.advance();
            return Ok(Spanned::new(Literal::String(name), span));
        }
        self.parse_literal()
    }

    // ── Rules Section (meta-cells) ───────────────────────────────────

    fn parse_rules_section(&mut self) -> Result<RulesSection, ParseError> {
        self.expect(Token::Rules)?;
        self.expect(Token::LBrace)?;

        let mut rules = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            rules.push(self.parse_rule()?);
        }

        self.expect(Token::RBrace)?;
        Ok(RulesSection { rules })
    }

    fn parse_rule(&mut self) -> Result<Spanned<Rule>, ParseError> {
        let start = self.peek_span();
        match self.peek() {
            Token::Contradicts => {
                self.advance();
                let names = self.parse_name_list()?;
                Ok(Spanned::new(Rule::Contradicts(names), start.merge(self.prev_span())))
            }
            Token::Implies => {
                self.advance();
                let names = self.parse_name_list()?;
                Ok(Spanned::new(Rule::Implies(names), start.merge(self.prev_span())))
            }
            Token::Requires => {
                self.advance();
                let names = self.parse_name_list()?;
                Ok(Spanned::new(Rule::Requires(names), start.merge(self.prev_span())))
            }
            Token::MutexGroup => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                Ok(Spanned::new(Rule::MutexGroup(name), start.merge(self.prev_span())))
            }
            Token::Check => {
                self.advance();
                self.expect(Token::LBrace)?;
                let mut body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(Rule::Check(body), start.merge(self.prev_span())))
            }
            Token::Matches => {
                self.advance();
                let names = self.parse_name_list()?;
                Ok(Spanned::new(Rule::Matches(names), start.merge(self.prev_span())))
            }
            Token::Native => {
                self.advance();
                let tok = &self.tokens[self.pos];
                if let Token::StringLit(name) = &tok.token {
                    let name = name.clone();
                    self.advance();
                    Ok(Spanned::new(Rule::Native(name), start.merge(self.prev_span())))
                } else {
                    Err(ParseError::Expected {
                        expected: "native function name string".to_string(),
                        found: tok.token.clone(),
                        span: tok.span,
                    })
                }
            }
            Token::Assert => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Spanned::new(Rule::Assert(expr), start.merge(self.prev_span())))
            }
            _ => Err(ParseError::Expected {
                expected: "contradicts, implies, requires, mutex_group, check, matches, native, or assert".to_string(),
                found: self.peek().clone(),
                span: self.peek_span(),
            }),
        }
    }

    /// Parse `[name1, name2, name3]`
    fn parse_name_list(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(Token::LBracket)?;
        let mut names = Vec::new();
        if !self.check(&Token::RBracket) {
            let (name, _) = self.expect_ident()?;
            names.push(name);
            while self.check(&Token::Comma) {
                self.advance();
                if self.check(&Token::RBracket) {
                    break;
                }
                let (name, _) = self.expect_ident()?;
                names.push(name);
            }
        }
        self.expect(Token::RBracket)?;
        Ok(names)
    }

    // ── Runtime Section ───────────────────────────────────────────────

    fn parse_runtime_section(&mut self) -> Result<RuntimeSection, ParseError> {
        self.expect(Token::Runtime)?;
        self.expect(Token::LBrace)?;

        let mut entries = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            entries.push(self.parse_runtime_entry()?);
        }

        self.expect(Token::RBrace)?;
        Ok(RuntimeSection { entries })
    }

    fn parse_runtime_entry(&mut self) -> Result<Spanned<RuntimeEntry>, ParseError> {
        let start = self.peek_span();
        match self.peek() {
            Token::Signal => {
                // emit signal_name(args)
                self.advance();
                let (name, _) = self.expect_ident()?;
                self.expect(Token::LParen)?;
                let args = self.parse_arg_list()?;
                self.expect(Token::RParen)?;
                Ok(Spanned::new(
                    RuntimeEntry::Emit { signal_name: name, args },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Connect => {
                // connect cell.signal -> cell
                self.advance();
                let (from_cell, _) = self.expect_ident()?;
                self.expect(Token::Dot)?;
                let (signal, _) = self.expect_ident()?;
                self.expect(Token::Arrow)?;
                let (to_cell, _) = self.expect_ident()?;
                Ok(Spanned::new(
                    RuntimeEntry::Connect { from_cell, signal, to_cell },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Start => {
                // start cell_name
                self.advance();
                let (cell_name, _) = self.expect_ident()?;
                Ok(Spanned::new(
                    RuntimeEntry::Start { cell_name },
                    start.merge(self.prev_span()),
                ))
            }
            _ => {
                // Fall through to statement parsing
                let stmt = self.parse_statement()?;
                Ok(Spanned::new(
                    RuntimeEntry::Stmt(stmt.node),
                    start.merge(self.prev_span()),
                ))
            }
        }
    }

    // ── Interior ─────────────────────────────────────────────────────

    fn parse_interior_section(&mut self) -> Result<InteriorSection, ParseError> {
        self.expect(Token::Interior)?;
        self.expect(Token::LBrace)?;

        let mut cells = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            cells.push(self.parse_cell_def()?);
        }

        self.expect(Token::RBrace)?;
        Ok(InteriorSection { cells })
    }

    // ── On (signal handler) ──────────────────────────────────────────

    fn parse_on_section(&mut self) -> Result<OnSection, ParseError> {
        self.expect(Token::On)?;
        let (signal_name, _) = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(Token::RParen)?;
        self.expect(Token::LBrace)?;

        let mut body = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }

        self.expect(Token::RBrace)?;
        Ok(OnSection {
            signal_name,
            params,
            body,
        })
    }

    fn parse_statement(&mut self) -> Result<Spanned<Statement>, ParseError> {
        let start = self.peek_span();
        match self.peek() {
            Token::Let => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Ok(Spanned::new(Statement::Let { name, value }, start.merge(self.prev_span())))
            }
            Token::Return => {
                self.advance();
                let value = self.parse_expr()?;
                Ok(Spanned::new(Statement::Return { value }, start.merge(self.prev_span())))
            }
            Token::If => {
                self.advance();
                let condition = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut then_body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    then_body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;

                let mut else_body = Vec::new();
                if self.check(&Token::Else) {
                    self.advance();
                    if self.check(&Token::If) {
                        // else if — wrap in a single-statement else body
                        let elif = self.parse_statement()?;
                        else_body.push(elif);
                    } else {
                        self.expect(Token::LBrace)?;
                        while !self.check(&Token::RBrace) && !self.is_at_end() {
                            else_body.push(self.parse_statement()?);
                        }
                        self.expect(Token::RBrace)?;
                    }
                }

                Ok(Spanned::new(
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    },
                    start.merge(self.prev_span()),
                ))
            }
            Token::For => {
                // for var in expr { body }
                self.advance();
                let (var, _) = self.expect_ident()?;
                self.expect(Token::In)?;
                let iter = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(
                    Statement::For { var, iter, body },
                    start.merge(self.prev_span()),
                ))
            }
            Token::While => {
                self.advance();
                let condition = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(
                    Statement::While { condition, body },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Signal => {
                self.advance();
                let (signal_name, _) = self.expect_ident()?;
                self.expect(Token::LParen)?;
                let args = self.parse_arg_list()?;
                self.expect(Token::RParen)?;
                Ok(Spanned::new(
                    Statement::Emit { signal_name, args },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Require => {
                self.advance();
                let constraint = self.parse_constraint()?;
                self.expect(Token::Else)?;
                let (else_signal, _) = self.expect_ident()?;
                Ok(Spanned::new(
                    Statement::Require {
                        constraint,
                        else_signal,
                    },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Ident(_) => {
                // Could be: assignment, target.method(args), fn_call(args), or bare expr
                let save_pos = self.pos;
                let (name, name_span) = self.expect_ident()?;

                if self.check(&Token::Eq) {
                    // Assignment: name = expr
                    self.advance();
                    let value = self.parse_expr()?;
                    Ok(Spanned::new(
                        Statement::Assign { name, value },
                        start.merge(self.prev_span()),
                    ))
                } else if self.check(&Token::Dot) {
                    // target.method(args)
                    self.advance();
                    let (method, _) = self.expect_ident()?;
                    self.expect(Token::LParen)?;
                    let args = self.parse_arg_list()?;
                    self.expect(Token::RParen)?;
                    Ok(Spanned::new(
                        Statement::MethodCall {
                            target: name,
                            method,
                            args,
                        },
                        start.merge(self.prev_span()),
                    ))
                } else if self.check(&Token::LParen) {
                    // fn_call(args) — treat as expression statement
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(Token::RParen)?;
                    Ok(Spanned::new(
                        Statement::ExprStmt {
                            expr: Spanned::new(
                                Expr::FnCall { name, args },
                                start.merge(self.prev_span()),
                            ),
                        },
                        start.merge(self.prev_span()),
                    ))
                } else {
                    // Bare identifier — back up and parse as expression statement
                    self.pos = save_pos;
                    let expr = self.parse_expr()?;
                    Ok(Spanned::new(
                        Statement::ExprStmt { expr },
                        start.merge(self.prev_span()),
                    ))
                }
            }
            _ => Err(ParseError::Expected {
                expected: "statement (let, return, if, signal, require, or expression)".to_string(),
                found: self.peek().clone(),
                span: self.peek_span(),
            }),
        }
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Spanned<Expr>>, ParseError> {
        let mut args = Vec::new();
        if self.check(&Token::RParen) {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while self.check(&Token::Comma) {
            self.advance();
            if self.check(&Token::RParen) {
                break;
            }
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }

    // ── Types ────────────────────────────────────────────────────────

    fn parse_type_expr(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let start = self.peek_span();

        // Check for lowercase ident (could be cell ref: orders.OrderId)
        if let Token::Ident(name) = self.peek().clone() {
            self.advance();
            if self.check(&Token::Dot) {
                self.advance();
                let (member, end_span) = self.expect_ident()?;
                return Ok(Spanned::new(
                    TypeExpr::CellRef {
                        cell: name,
                        member,
                    },
                    start.merge(end_span),
                ));
            }
            return Ok(Spanned::new(TypeExpr::Simple(name), start));
        }

        let (name, _) = self.expect_type_ident()?;

        if self.check(&Token::Lt) {
            self.advance();
            let mut args = Vec::new();
            args.push(self.parse_type_expr()?);
            while self.check(&Token::Comma) {
                self.advance();
                args.push(self.parse_type_expr()?);
            }
            let end = self.peek_span();
            self.expect(Token::Gt)?;
            Ok(Spanned::new(
                TypeExpr::Generic { name, args },
                start.merge(end),
            ))
        } else {
            Ok(Spanned::new(TypeExpr::Simple(name), start))
        }
    }

    // ── Constraints ──────────────────────────────────────────────────

    fn parse_constraint(&mut self) -> Result<Spanned<Constraint>, ParseError> {
        self.parse_or_constraint()
    }

    fn parse_or_constraint(&mut self) -> Result<Spanned<Constraint>, ParseError> {
        let mut left = self.parse_and_constraint()?;
        while self.check(&Token::OrOr) {
            self.advance();
            let right = self.parse_and_constraint()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(Constraint::Or(Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn parse_and_constraint(&mut self) -> Result<Spanned<Constraint>, ParseError> {
        let mut left = self.parse_primary_constraint()?;
        while self.check(&Token::AndAnd) {
            self.advance();
            let right = self.parse_primary_constraint()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(Constraint::And(Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn parse_primary_constraint(&mut self) -> Result<Spanned<Constraint>, ParseError> {
        let start = self.peek_span();

        if self.check(&Token::Bang) {
            self.advance();
            let inner = self.parse_primary_constraint()?;
            let span = start.merge(inner.span);
            return Ok(Spanned::new(Constraint::Not(Box::new(inner)), span));
        }

        let left = self.parse_expr()?;

        let op = match self.peek() {
            Token::Lt => Some(CmpOp::Lt),
            Token::Gt => Some(CmpOp::Gt),
            Token::Le => Some(CmpOp::Le),
            Token::Ge => Some(CmpOp::Ge),
            Token::EqEq => Some(CmpOp::Eq),
            Token::Ne => Some(CmpOp::Ne),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_expr()?;
            let span = left.span.merge(right.span);
            return Ok(Spanned::new(Constraint::Comparison { left, op, right }, span));
        }

        match &left.node {
            Expr::Ident(name) => {
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    let end = self.peek_span();
                    self.expect(Token::RParen)?;
                    Ok(Spanned::new(
                        Constraint::Predicate {
                            name: name.clone(),
                            args,
                        },
                        start.merge(end),
                    ))
                } else {
                    Ok(Spanned::new(
                        Constraint::Predicate {
                            name: name.clone(),
                            args: vec![],
                        },
                        left.span,
                    ))
                }
            }
            _ => Ok(Spanned::new(
                Constraint::Predicate {
                    name: format!("{:?}", left.node),
                    args: vec![],
                },
                left.span,
            )),
        }
    }

    // ── Expressions ──────────────────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let left = self.parse_additive()?;

        let op = match self.peek() {
            Token::Lt => Some(CmpOp::Lt),
            Token::Gt => Some(CmpOp::Gt),
            Token::Le => Some(CmpOp::Le),
            Token::Ge => Some(CmpOp::Ge),
            Token::EqEq => Some(CmpOp::Eq),
            Token::Ne => Some(CmpOp::Ne),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_additive()?;
            let span = left.span.merge(right.span);
            Ok(Spanned::new(
                Expr::CmpOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span,
            ))
        } else {
            Ok(left)
        }
    }

    fn parse_additive(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_multiplicative()?;
        while matches!(self.peek(), Token::Plus | Token::Minus) {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => unreachable!(),
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek(), Token::Star | Token::Slash | Token::Percent) {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => unreachable!(),
            };
            self.advance();
            let right = self.parse_unary()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Spanned<Expr>, ParseError> {
        if self.check(&Token::Bang) {
            let start = self.peek_span();
            self.advance();
            let expr = self.parse_unary()?;
            let span = start.merge(expr.span);
            return Ok(Spanned::new(Expr::Not(Box::new(expr)), span));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            if self.check(&Token::Dot) {
                self.advance();
                let (field, field_span) = self.expect_ident()?;

                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    let end = self.peek_span();
                    self.expect(Token::RParen)?;
                    let span = expr.span.merge(end);
                    expr = Spanned::new(
                        Expr::MethodCall {
                            target: Box::new(expr),
                            method: field,
                            args,
                        },
                        span,
                    );
                } else {
                    let span = expr.span.merge(field_span);
                    expr = Spanned::new(
                        Expr::FieldAccess {
                            target: Box::new(expr),
                            field,
                        },
                        span,
                    );
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span();
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Int(n)), start))
            }
            Token::FloatLit(n) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Float(n)), start))
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::String(s)), start))
            }
            Token::True => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(true)), start))
            }
            Token::False => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(false)), start))
            }
            Token::DurationLit(val, unit) => {
                self.advance();
                let du = match unit {
                    DurationUnitTok::Ms => DurationUnit::Milliseconds,
                    DurationUnitTok::S => DurationUnit::Seconds,
                    DurationUnitTok::Min => DurationUnit::Minutes,
                    DurationUnitTok::H => DurationUnit::Hours,
                    DurationUnitTok::D => DurationUnit::Days,
                    DurationUnitTok::Years => DurationUnit::Years,
                };
                Ok(Spanned::new(
                    Expr::Literal(Literal::Duration(Duration { value: val, unit: du })),
                    start,
                ))
            }
            Token::PercentLit(val) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Percentage(val)), start))
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                // Check for function call: name(args)
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    let end = self.peek_span();
                    self.expect(Token::RParen)?;
                    Ok(Spanned::new(Expr::FnCall { name, args }, start.merge(end)))
                } else {
                    Ok(Spanned::new(Expr::Ident(name), start))
                }
            }
            Token::TypeIdent(name) => {
                let name = name.clone();
                self.advance();
                Ok(Spanned::new(Expr::Ident(name), start))
            }
            Token::LParen => {
                self.advance();
                // Check for Unit literal: ()
                if self.check(&Token::RParen) {
                    self.advance();
                    return Ok(Spanned::new(Expr::Literal(Literal::Unit), start));
                }
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            _ => Err(ParseError::Expected {
                expected: "expression".to_string(),
                found: self.peek().clone(),
                span: start,
            }),
        }
    }

    fn parse_literal(&mut self) -> Result<Spanned<Literal>, ParseError> {
        let start = self.peek_span();
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Spanned::new(Literal::Int(n), start))
            }
            Token::FloatLit(n) => {
                self.advance();
                Ok(Spanned::new(Literal::Float(n), start))
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Spanned::new(Literal::String(s), start))
            }
            Token::DurationLit(val, unit) => {
                self.advance();
                let du = match unit {
                    DurationUnitTok::Ms => DurationUnit::Milliseconds,
                    DurationUnitTok::S => DurationUnit::Seconds,
                    DurationUnitTok::Min => DurationUnit::Minutes,
                    DurationUnitTok::H => DurationUnit::Hours,
                    DurationUnitTok::D => DurationUnit::Days,
                    DurationUnitTok::Years => DurationUnit::Years,
                };
                Ok(Spanned::new(
                    Literal::Duration(Duration { value: val, unit: du }),
                    start,
                ))
            }
            Token::PercentLit(val) => {
                self.advance();
                Ok(Spanned::new(Literal::Percentage(val), start))
            }
            Token::True => {
                self.advance();
                Ok(Spanned::new(Literal::Bool(true), start))
            }
            Token::False => {
                self.advance();
                Ok(Spanned::new(Literal::Bool(false), start))
            }
            _ => Err(ParseError::Expected {
                expected: "literal".to_string(),
                found: self.peek().clone(),
                span: start,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(input: &str) -> Result<Program, ParseError> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().expect("lexer error");
        let mut parser = Parser::new(tokens);
        parser.parse_program()
    }

    #[test]
    fn test_empty_cell() {
        let program = parse("cell Counter {}").unwrap();
        assert_eq!(program.cells.len(), 1);
        assert_eq!(program.cells[0].node.name, "Counter");
        assert_eq!(program.cells[0].node.kind, CellKind::Cell);
    }

    #[test]
    fn test_cell_property() {
        let input = r#"
            cell property persistent {
                face {
                    promise "data survives cell restart"
                }
                rules {
                    contradicts [ephemeral]
                    mutex_group durability
                }
            }
        "#;
        let program = parse(input).unwrap();
        let cell = &program.cells[0].node;
        assert_eq!(cell.kind, CellKind::Property);
        assert_eq!(cell.name, "persistent");
        assert_eq!(cell.sections.len(), 2);

        // Check rules section
        if let Section::Rules(ref rules) = cell.sections[1].node {
            assert_eq!(rules.rules.len(), 2);
            if let Rule::Contradicts(ref names) = rules.rules[0].node {
                assert_eq!(names, &["ephemeral"]);
            } else {
                panic!("expected Contradicts rule");
            }
            if let Rule::MutexGroup(ref name) = rules.rules[1].node {
                assert_eq!(name, "durability");
            } else {
                panic!("expected MutexGroup rule");
            }
        } else {
            panic!("expected rules section");
        }
    }

    #[test]
    fn test_cell_type_with_generics() {
        let input = r#"
            cell type TimeSeries<T> {
                face {
                    signal record(timestamp: Int, value: T)
                    signal latest() -> T
                }
            }
        "#;
        let program = parse(input).unwrap();
        let cell = &program.cells[0].node;
        assert_eq!(cell.kind, CellKind::Type);
        assert_eq!(cell.name, "TimeSeries");
        assert_eq!(cell.type_params, vec!["T"]);
    }

    #[test]
    fn test_cell_checker() {
        let input = r#"
            cell checker auth_required {
                face {
                    promise "every network cell validates auth"
                }
                rules {
                    check {
                        require has_auth else MissingAuth
                    }
                }
            }
        "#;
        let program = parse(input).unwrap();
        let cell = &program.cells[0].node;
        assert_eq!(cell.kind, CellKind::Checker);
        assert_eq!(cell.name, "auth_required");
    }

    #[test]
    fn test_string_based_properties() {
        let input = r#"
            cell Store {
                memory {
                    data: Map<String, Int> [persistent, consistent, my_custom_prop]
                    cache: Map<String, Int> [ephemeral, local, capacity(1000)]
                }
            }
        "#;
        let program = parse(input).unwrap();
        let cell = &program.cells[0].node;
        if let Section::Memory(ref mem) = cell.sections[0].node {
            assert_eq!(mem.slots[0].node.properties.len(), 3);
            // Third property is custom — not a compile error anymore
            if let MemoryProperty::Flag(ref name) = mem.slots[0].node.properties[2].node {
                assert_eq!(name, "my_custom_prop");
            } else {
                panic!("expected flag property");
            }
        } else {
            panic!("expected memory section");
        }
    }

    #[test]
    fn test_cell_with_face() {
        let input = r#"
            cell Counter {
                face {
                    signal increment(amount: Int)
                    signal get() -> Int
                    promise value >= 0
                }
            }
        "#;
        let program = parse(input).unwrap();
        assert_eq!(program.cells[0].node.name, "Counter");
    }

    #[test]
    fn test_cell_with_memory() {
        let input = r#"
            cell Store {
                memory {
                    data: Map<String, Int> [persistent, consistent]
                    cache: Map<String, Int> [ephemeral, local, capacity(1000)]
                }
            }
        "#;
        let program = parse(input).unwrap();
        if let Section::Memory(ref mem) = program.cells[0].node.sections[0].node {
            assert_eq!(mem.slots.len(), 2);
        }
    }

    #[test]
    fn test_cell_with_interior() {
        let input = r#"
            cell Parent {
                interior {
                    cell Child1 {
                        face {
                            signal ping()
                        }
                    }
                    cell Child2 {
                        face {
                            await pong()
                        }
                    }
                }
            }
        "#;
        let program = parse(input).unwrap();
        if let Section::Interior(ref interior) = program.cells[0].node.sections[0].node {
            assert_eq!(interior.cells.len(), 2);
        }
    }

    #[test]
    fn test_promise_descriptive() {
        let input = r#"
            cell Service {
                face {
                    promise "all payments settle within 24h"
                }
            }
        "#;
        let program = parse(input).unwrap();
        if let Section::Face(ref face) = program.cells[0].node.sections[0].node {
            if let FaceDecl::Promise(ref p) = face.declarations[0].node {
                assert!(matches!(p.constraint.node, Constraint::Descriptive(_)));
            }
        }
    }

    #[test]
    fn test_given_with_where() {
        let input = r#"
            cell Transfer {
                face {
                    given amount: Int where { amount > 0 }
                }
            }
        "#;
        let program = parse(input).unwrap();
        if let Section::Face(ref face) = program.cells[0].node.sections[0].node {
            if let FaceDecl::Given(ref g) = face.declarations[0].node {
                assert!(g.where_clause.is_some());
            }
        }
    }

    #[test]
    fn test_rules_implies_and_requires() {
        let input = r#"
            cell property replicated {
                rules {
                    implies [persistent]
                    requires [consistent]
                }
            }
        "#;
        let program = parse(input).unwrap();
        if let Section::Rules(ref rules) = program.cells[0].node.sections[0].node {
            assert_eq!(rules.rules.len(), 2);
            if let Rule::Implies(ref names) = rules.rules[0].node {
                assert_eq!(names, &["persistent"]);
            }
            if let Rule::Requires(ref names) = rules.rules[1].node {
                assert_eq!(names, &["consistent"]);
            }
        }
    }
}
