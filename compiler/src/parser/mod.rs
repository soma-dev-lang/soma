use crate::ast::*;
use crate::lexer::{DurationUnitTok, SpannedToken, Token};
use thiserror::Error;

fn display_token(token: &Token) -> String {
    match token {
        Token::Eq => "'='".to_string(),
        Token::EqEq => "'=='".to_string(),
        Token::Ne => "'!='".to_string(),
        Token::Lt => "'<'".to_string(),
        Token::Gt => "'>'".to_string(),
        Token::Le => "'<='".to_string(),
        Token::Ge => "'>='".to_string(),
        Token::Plus => "'+'".to_string(),
        Token::Minus => "'-'".to_string(),
        Token::Star => "'*'".to_string(),
        Token::Slash => "'/'".to_string(),
        Token::Percent => "'%'".to_string(),
        Token::Pipe => "'|>'".to_string(),
        Token::Arrow => "'->'".to_string(),
        Token::FatArrow => "'=>'".to_string(),
        Token::LParen => "'('".to_string(),
        Token::RParen => "')'".to_string(),
        Token::LBrace => "'{'".to_string(),
        Token::RBrace => "'}'".to_string(),
        Token::LBracket => "'['".to_string(),
        Token::RBracket => "']'".to_string(),
        Token::Comma => "','".to_string(),
        Token::Dot => "'.'".to_string(),
        Token::Colon => "':'".to_string(),
        Token::Bang => "'!'".to_string(),
        Token::AndAnd => "'&&'".to_string(),
        Token::OrOr => "'||'".to_string(),
        Token::PlusEq => "'+='".to_string(),
        Token::MinusEq => "'-='".to_string(),
        Token::StarEq => "'*='".to_string(),
        Token::SlashEq => "'/='".to_string(),
        Token::NullCoal => "'??'".to_string(),
        Token::Question => "'?'".to_string(),
        Token::IntLit(n) => format!("number '{}'", n),
        Token::BigIntLit(ref s) => format!("number '{}'", s),
        Token::FloatLit(n) => format!("number '{}'", n),
        Token::StringLit(s) => format!("string \"{}\"", s),
        Token::Ident(s) => format!("'{}'", s),
        Token::TypeIdent(s) => format!("'{}'", s),
        Token::Cell => "'cell'".to_string(),
        Token::Let => "'let'".to_string(),
        Token::If => "'if'".to_string(),
        Token::Else => "'else'".to_string(),
        Token::Return => "'return'".to_string(),
        Token::For => "'for'".to_string(),
        Token::While => "'while'".to_string(),
        Token::Match => "'match'".to_string(),
        Token::Break => "'break'".to_string(),
        Token::Continue => "'continue'".to_string(),
        Token::Face => "'face'".to_string(),
        Token::Memory => "'memory'".to_string(),
        Token::Interior => "'interior'".to_string(),
        Token::On => "'on'".to_string(),
        Token::Use => "'use'".to_string(),
        Token::True => "'true'".to_string(),
        Token::False => "'false'".to_string(),
        Token::In => "'in'".to_string(),
        Token::Try => "'try'".to_string(),
        Token::Catch => "'catch'".to_string(),
        Token::Every => "'every'".to_string(),
        Token::After => "'after'".to_string(),
        Token::Ensure => "'ensure'".to_string(),
        Token::Tool => "'tool'".to_string(),
        Token::AgentKw => "'agent'".to_string(),
        Token::Scale => "'scale'".to_string(),
        Token::Eof => "end of file".to_string(),
        _ => format!("{:?}", token),
    }
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("expected {expected}, found {}", display_token(found))]
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

        // V1: collect free-standing protocol/adversary/prove blocks at the
        // top level and stash them in a synthetic cell so the checker finds
        // them. Cell name `__top__` is reserved.
        let mut top_sections: Vec<Spanned<Section>> = Vec::new();

        while !self.is_at_end() {
            match self.peek() {
                Token::Protocol => {
                    let start = self.peek_span();
                    let p = self.parse_protocol_section()?;
                    top_sections.push(Spanned::new(Section::Protocol(p), start.merge(self.prev_span())));
                }
                Token::Adversary => {
                    let start = self.peek_span();
                    let a = self.parse_adversary_section()?;
                    top_sections.push(Spanned::new(Section::Adversary(a), start.merge(self.prev_span())));
                }
                Token::Prove => {
                    let start = self.peek_span();
                    let p = self.parse_prove_section()?;
                    top_sections.push(Spanned::new(Section::Prove(p), start.merge(self.prev_span())));
                }
                _ => cells.push(self.parse_cell_def()?),
            }
        }
        if !top_sections.is_empty() {
            // Wrap into a hidden cell so downstream passes find these sections
            // without any AST refactor.
            let span = top_sections.first().map(|s| s.span).unwrap_or(Span::new(0, 0));
            cells.push(Spanned::new(
                CellDef {
                    kind: CellKind::Cell,
                    name: "__top__".to_string(),
                    type_params: Vec::new(),
                    sections: top_sections,
                    agent_model: None,
                    agent_skill: None,
                },
                span,
            ));
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
                expected: display_token(&expected).to_string(),
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
            // Keywords that can also be used as identifiers in certain positions
            // Keywords that can also be used as identifiers
            Token::State => { let span = tok.span; self.advance(); Ok(("state".to_string(), span)) }
            Token::Type => { let span = tok.span; self.advance(); Ok(("type".to_string(), span)) }
            Token::Effect => { let span = tok.span; self.advance(); Ok(("effect".to_string(), span)) }
            Token::Guard => { let span = tok.span; self.advance(); Ok(("guard".to_string(), span)) }
            Token::Initial => { let span = tok.span; self.advance(); Ok(("initial".to_string(), span)) }
            Token::Start => { let span = tok.span; self.advance(); Ok(("start".to_string(), span)) }
            Token::Test => { let span = tok.span; self.advance(); Ok(("test".to_string(), span)) }
            Token::Backend => { let span = tok.span; self.advance(); Ok(("backend".to_string(), span)) }
            Token::Builtin => { let span = tok.span; self.advance(); Ok(("builtin".to_string(), span)) }
            Token::Signal => { let span = tok.span; self.advance(); Ok(("signal".to_string(), span)) }
            Token::Emit => { let span = tok.span; self.advance(); Ok(("emit".to_string(), span)) }
            Token::Check => { let span = tok.span; self.advance(); Ok(("check".to_string(), span)) }
            Token::Memory => { let span = tok.span; self.advance(); Ok(("memory".to_string(), span)) }
            Token::Face => { let span = tok.span; self.advance(); Ok(("face".to_string(), span)) }
            Token::Property => { let span = tok.span; self.advance(); Ok(("property".to_string(), span)) }
            Token::Rules => { let span = tok.span; self.advance(); Ok(("rules".to_string(), span)) }
            Token::Runtime => { let span = tok.span; self.advance(); Ok(("runtime".to_string(), span)) }
            Token::Matches => { let span = tok.span; self.advance(); Ok(("matches".to_string(), span)) }
            Token::Native => { let span = tok.span; self.advance(); Ok(("native".to_string(), span)) }
            Token::Checker => { let span = tok.span; self.advance(); Ok(("checker".to_string(), span)) }
            Token::Scale => { let span = tok.span; self.advance(); Ok(("scale".to_string(), span)) }
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
            Token::Emit => "emit".to_string(),
            Token::Check => "check".to_string(),
            Token::Type => "type".to_string(),
            Token::Property => "property".to_string(),
            Token::State => "state".to_string(),
            Token::Start => "start".to_string(),
            Token::Guard => "guard".to_string(),
            Token::Effect => "effect".to_string(),
            Token::Initial => "initial".to_string(),
            Token::Test => "test".to_string(),
            Token::Backend => "backend".to_string(),
            Token::Builtin => "builtin".to_string(),
            Token::Runtime => "runtime".to_string(),
            Token::Scale => "scale".to_string(),
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
            Token::AgentKw => {
                self.advance();
                let (name, _) = self.expect_ident()?;
                (CellKind::Agent, name, vec![])
            }
            _ => {
                let (name, _) = self.expect_ident()?;
                (CellKind::Cell, name, vec![])
            }
        };

        // Parse agent annotations: [model: name, skill: "path"]
        let mut agent_model = None;
        let mut agent_skill = None;
        if kind == CellKind::Agent && self.check(&Token::LBracket) {
            self.advance();
            while !self.check(&Token::RBracket) && !self.is_at_end() {
                if let Token::Ident(ref attr) = self.peek().clone() {
                    let attr = attr.clone();
                    self.advance();
                    self.expect(Token::Colon)?;
                    match attr.as_str() {
                        "model" => {
                            let (model_name, _) = self.expect_ident()?;
                            agent_model = Some(model_name);
                        }
                        "skill" => {
                            if let Token::StringLit(ref path) = self.peek().clone() {
                                agent_skill = Some(path.clone());
                                self.advance();
                            }
                        }
                        _ => { self.advance(); } // skip unknown attrs
                    }
                    if self.check(&Token::Comma) { self.advance(); }
                } else {
                    self.advance();
                }
            }
            self.expect(Token::RBracket)?;
        }

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
                agent_model,
                agent_skill,
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
            Token::State => {
                let sm = self.parse_state_machine()?;
                Ok(Spanned::new(Section::State(sm), start.merge(self.prev_span())))
            }
            Token::Every => {
                let ev = self.parse_every_section()?;
                Ok(Spanned::new(Section::Every(ev), start.merge(self.prev_span())))
            }
            Token::After => {
                let ev = self.parse_after_section()?;
                Ok(Spanned::new(Section::After(ev), start.merge(self.prev_span())))
            }
            Token::Scale => {
                let sc = self.parse_scale_section()?;
                Ok(Spanned::new(Section::Scale(sc), start.merge(self.prev_span())))
            }
            Token::Protocol => {
                let p = self.parse_protocol_section()?;
                Ok(Spanned::new(Section::Protocol(p), start.merge(self.prev_span())))
            }
            Token::Adversary => {
                let a = self.parse_adversary_section()?;
                Ok(Spanned::new(Section::Adversary(a), start.merge(self.prev_span())))
            }
            Token::Prove => {
                let p = self.parse_prove_section()?;
                Ok(Spanned::new(Section::Prove(p), start.merge(self.prev_span())))
            }
            Token::Assert => {
                // In test cells, `assert expr` is syntactic sugar for a Rules section with Assert rules
                let mut rules = Vec::new();
                while self.check(&Token::Assert) {
                    let rule_start = self.peek_span();
                    self.advance();
                    let expr = self.parse_expr()?;
                    rules.push(Spanned::new(Rule::Assert(expr), rule_start.merge(self.prev_span())));
                }
                Ok(Spanned::new(
                    Section::Rules(RulesSection { rules }),
                    start.merge(self.prev_span()),
                ))
            }
            _ => Err(ParseError::Expected {
                expected: "face, memory, interior, on, rules, runtime, state, every, scale, protocol, adversary, or prove".to_string(),
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
            Token::Tool => {
                let decl = self.parse_tool_decl()?;
                Ok(Spanned::new(FaceDecl::Tool(decl), start.merge(self.prev_span())))
            }
            _ => Err(ParseError::Expected {
                expected: "given, promise, signal, await, or tool".to_string(),
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

    fn parse_tool_decl(&mut self) -> Result<ToolDecl, ParseError> {
        self.expect(Token::Tool)?;
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

        // Optional description string
        let description = if let Token::StringLit(ref s) = self.peek().clone() {
            let d = s.clone();
            self.advance();
            Some(d)
        } else {
            None
        };

        Ok(ToolDecl {
            name,
            description,
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
        let mut invariants = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Check for `invariant expr` declaration
            if let Token::Ident(ref name) = self.peek().clone() {
                if name == "invariant" {
                    let start = self.peek_span();
                    self.advance();
                    let expr = self.parse_expr()?;
                    invariants.push(Spanned::new(expr.node, start.merge(expr.span)));
                    continue;
                }
            }
            slots.push(self.parse_memory_slot()?);
        }

        self.expect(Token::RBrace)?;
        Ok(MemorySection { slots, invariants })
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
            Token::Signal | Token::Emit => {
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

    // ── State Machine ─────────────────────────────────────────────────

    fn parse_state_machine(&mut self) -> Result<StateMachineSection, ParseError> {
        self.expect(Token::State)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;

        let mut initial = String::new();
        let mut transitions = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Check for `initial: state_name`
            if self.check(&Token::Initial) {
                self.advance();
                self.expect(Token::Colon)?;
                let (state_name, _) = self.expect_ident()?;
                initial = state_name;
                continue;
            }

            // Parse transition: from -> to { guard { ... } effect { ... } }
            // or: * -> to { ... }
            let start = self.peek_span();
            let from = if self.check(&Token::Star) {
                self.advance();
                "*".to_string()
            } else {
                let (name, _) = self.expect_ident()?;
                name
            };

            self.expect(Token::Arrow)?;

            let (to, _) = self.expect_ident()?;

            let mut guard = None;
            let mut effect = Vec::new();

            // Optional block with guard/effect
            if self.check(&Token::LBrace) {
                self.advance();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    if self.check(&Token::Guard) {
                        self.advance();
                        self.expect(Token::LBrace)?;
                        let expr = self.parse_expr()?;
                        self.expect(Token::RBrace)?;
                        guard = Some(expr);
                    } else if self.check(&Token::Effect) {
                        self.advance();
                        self.expect(Token::LBrace)?;
                        while !self.check(&Token::RBrace) && !self.is_at_end() {
                            effect.push(self.parse_statement()?);
                        }
                        self.expect(Token::RBrace)?;
                    } else {
                        self.advance();
                    }
                }
                self.expect(Token::RBrace)?;
            }

            transitions.push(Spanned::new(
                Transition { from: from.clone(), to: to.clone(), guard, effect },
                start.merge(self.prev_span()),
            ));

            // Support chained transitions: a -> b -> c -> d
            // Desugars to: a -> b, b -> c, c -> d
            let mut prev = to;
            while self.check(&Token::Arrow) {
                self.advance();
                let (next, _) = self.expect_ident()?;
                transitions.push(Spanned::new(
                    Transition { from: prev.clone(), to: next.clone(), guard: None, effect: vec![] },
                    start.merge(self.prev_span()),
                ));
                prev = next;
            }
        }

        self.expect(Token::RBrace)?;

        // Default initial to first "from" state if not specified
        if initial.is_empty() {
            if let Some(first) = transitions.first() {
                if first.node.from != "*" {
                    initial = first.node.from.clone();
                }
            }
        }

        Ok(StateMachineSection { name, initial, transitions })
    }

    // ── Every (scheduler) ─────────────────────────────────────────────

    fn parse_every_section(&mut self) -> Result<EverySection, ParseError> {
        self.expect(Token::Every)?;
        // Parse interval: 30s, 5min, 1h, 500ms, or bare number (seconds)
        let tok = &self.tokens[self.pos].clone();
        let interval_ms = match &tok.token {
            Token::DurationLit(val, unit) => {
                self.advance();
                let multiplier = match unit {
                    crate::lexer::DurationUnitTok::Ms => 1.0,
                    crate::lexer::DurationUnitTok::S => 1000.0,
                    crate::lexer::DurationUnitTok::Min => 60_000.0,
                    crate::lexer::DurationUnitTok::H => 3_600_000.0,
                    crate::lexer::DurationUnitTok::D => 86_400_000.0,
                    crate::lexer::DurationUnitTok::Years => 365.25 * 86_400_000.0,
                };
                (val * multiplier) as u64
            }
            Token::IntLit(n) => {
                self.advance();
                (*n as u64) * 1000 // bare number = seconds
            }
            _ => {
                return Err(ParseError::Expected {
                    expected: "interval (e.g., 30s, 5min, 1h)".to_string(),
                    found: tok.token.clone(),
                    span: tok.span,
                });
            }
        };

        self.expect(Token::LBrace)?;
        let mut body = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }
        self.expect(Token::RBrace)?;

        Ok(EverySection { interval_ms, body })
    }

    // ── After (one-shot delayed) ──────────────────────────────────

    fn parse_after_section(&mut self) -> Result<EverySection, ParseError> {
        self.expect(Token::After)?;
        let tok = &self.tokens[self.pos].clone();
        let interval_ms = match &tok.token {
            Token::DurationLit(val, unit) => {
                self.advance();
                let multiplier = match unit {
                    crate::lexer::DurationUnitTok::Ms => 1.0,
                    crate::lexer::DurationUnitTok::S => 1000.0,
                    crate::lexer::DurationUnitTok::Min => 60_000.0,
                    crate::lexer::DurationUnitTok::H => 3_600_000.0,
                    crate::lexer::DurationUnitTok::D => 86_400_000.0,
                    crate::lexer::DurationUnitTok::Years => 365.25 * 86_400_000.0,
                };
                (val * multiplier) as u64
            }
            Token::IntLit(n) => {
                self.advance();
                (*n as u64) * 1000
            }
            _ => {
                return Err(ParseError::Expected {
                    expected: "delay (e.g., 5s, 1min, 500ms)".to_string(),
                    found: tok.token.clone(),
                    span: tok.span,
                });
            }
        };

        self.expect(Token::LBrace)?;
        let mut body = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            body.push(self.parse_statement()?);
        }
        self.expect(Token::RBrace)?;

        Ok(EverySection { interval_ms, body })
    }

    // ── Scale (Orchestration) ───────────────────────────────────────

    fn parse_scale_section(&mut self) -> Result<ScaleSection, ParseError> {
        self.expect(Token::Scale)?;
        self.expect(Token::LBrace)?;

        let mut replicas: u64 = 1;
        let mut shard: Option<String> = None;
        let mut consistency = ScaleConsistency::Strong;
        let mut tolerance: u64 = 0;
        let mut cpu: Option<u64> = None;
        let mut memory_res: Option<String> = None;
        let mut disk: Option<String> = None;
        let mut survives: Vec<String> = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let (key, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            match key.as_str() {
                "replicas" | "tolerance" | "cpu" => {
                    if let Token::IntLit(n) = self.peek().clone() {
                        match key.as_str() {
                            "replicas" => replicas = n as u64,
                            "tolerance" => tolerance = n as u64,
                            "cpu" => cpu = Some(n as u64),
                            _ => {}
                        }
                        self.advance();
                    } else {
                        return Err(ParseError::Expected {
                            expected: "integer".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        });
                    }
                }
                "shard" => {
                    let (name, _) = self.expect_ident()?;
                    shard = Some(name);
                }
                "consistency" => {
                    let (val, _) = self.expect_ident()?;
                    consistency = match val.as_str() {
                        "strong" => ScaleConsistency::Strong,
                        "causal" => ScaleConsistency::Causal,
                        "eventual" => ScaleConsistency::Eventual,
                        _ => {
                            return Err(ParseError::Expected {
                                expected: "strong, causal, or eventual".to_string(),
                                found: Token::Ident(val),
                                span: self.prev_span(),
                            });
                        }
                    };
                }
                "memory" | "disk" => {
                    // Parse size literal: "8Gi", "512Mi", "1Ti" or just a string
                    let val = match self.peek().clone() {
                        Token::StringLit(s) => { self.advance(); s }
                        Token::Ident(s) => { self.advance(); s }
                        // Handle patterns like 8Gi parsed as IntLit + Ident
                        Token::IntLit(n) => {
                            self.advance();
                            // Check for unit suffix
                            if let Token::Ident(unit) = self.peek().clone() {
                                if unit == "Gi" || unit == "Mi" || unit == "Ti" || unit == "Ki" {
                                    self.advance();
                                    format!("{}{}", n, unit)
                                } else {
                                    format!("{}", n)
                                }
                            } else {
                                format!("{}", n)
                            }
                        }
                        _ => {
                            // Try DurationLit-style: might be parsed as something else
                            let (name, _) = self.expect_ident()?;
                            name
                        }
                    };
                    match key.as_str() {
                        "memory" => memory_res = Some(val),
                        "disk" => disk = Some(val),
                        _ => {}
                    }
                }
                "survives" => {
                    // Accept either: `survives: [name1, name2]` or
                    // `survives: name1 ∧ name2` (we tokenize ∧ as Ident with
                    // unicode, so just accept idents joined by AndAnd or `∧` ident).
                    if self.check(&Token::LBracket) {
                        survives = self.parse_name_list()?;
                    } else {
                        loop {
                            let (name, _) = self.expect_ident()?;
                            survives.push(name);
                            // Accept `&&`, `,`, or stop at next key
                            if self.check(&Token::AndAnd) || self.check(&Token::Comma) {
                                self.advance();
                                continue;
                            }
                            break;
                        }
                    }
                }
                other => {
                    return Err(ParseError::Expected {
                        expected: "replicas, shard, consistency, tolerance, cpu, memory, disk, or survives".to_string(),
                        found: Token::Ident(other.to_string()),
                        span: self.prev_span(),
                    });
                }
            }
        }
        self.expect(Token::RBrace)?;

        Ok(ScaleSection { replicas, shard, consistency, tolerance, cpu, memory: memory_res, disk, survives })
    }

    // ── V1: Protocol (session types) ──────────────────────────────────

    fn parse_protocol_section(&mut self) -> Result<ProtocolSection, ParseError> {
        self.expect(Token::Protocol)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;

        let mut roles: Vec<String> = Vec::new();
        let steps = self.parse_protocol_steps(&mut roles)?;

        self.expect(Token::RBrace)?;
        Ok(ProtocolSection { name, roles, steps })
    }

    /// Parse a sequence of protocol steps until `}` (recursive for loop/choice).
    fn parse_protocol_steps(
        &mut self,
        roles: &mut Vec<String>,
    ) -> Result<Vec<Spanned<ProtocolStep>>, ParseError> {
        let mut steps = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            // `loop { ... }` — `loop` is parsed as Ident
            if let Token::Ident(kw) = self.peek().clone() {
                if kw == "loop" {
                    self.advance();
                    self.expect(Token::LBrace)?;
                    let body = self.parse_protocol_steps(roles)?;
                    self.expect(Token::RBrace)?;
                    steps.push(Spanned::new(
                        ProtocolStep::Loop(body),
                        start.merge(self.prev_span()),
                    ));
                    continue;
                }
                if kw == "choice" {
                    self.advance();
                    self.expect(Token::LBrace)?;
                    let mut branches: Vec<Vec<Spanned<ProtocolStep>>> = Vec::new();
                    while !self.check(&Token::RBrace) && !self.is_at_end() {
                        // Each branch is a `{ ... }` block, optionally separated by `or`/`|`
                        if self.check(&Token::LBrace) {
                            self.advance();
                            let b = self.parse_protocol_steps(roles)?;
                            self.expect(Token::RBrace)?;
                            branches.push(b);
                        } else if let Token::Ident(s) = self.peek().clone() {
                            if s == "or" {
                                self.advance();
                                continue;
                            }
                            // Bare branch — parse one step inline
                            let mut tmp_roles = std::mem::take(roles);
                            let one = self.parse_one_protocol_step(&mut tmp_roles)?;
                            *roles = tmp_roles;
                            branches.push(vec![one]);
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    steps.push(Spanned::new(
                        ProtocolStep::Choice(branches),
                        start.merge(self.prev_span()),
                    ));
                    continue;
                }
            }
            // Otherwise: a `from -> to : Msg(...)` send
            let one = self.parse_one_protocol_step(roles)?;
            steps.push(one);
        }
        Ok(steps)
    }

    fn parse_one_protocol_step(
        &mut self,
        roles: &mut Vec<String>,
    ) -> Result<Spanned<ProtocolStep>, ParseError> {
        let start = self.peek_span();
        let (from, _) = self.expect_ident()?;
        self.expect(Token::Arrow)?;
        let (to, _) = self.expect_ident()?;
        self.expect(Token::Colon)?;
        // Message name is a TypeIdent (capitalized) or ident
        let message = match self.peek().clone() {
            Token::TypeIdent(s) => { self.advance(); s }
            Token::Ident(s) => { self.advance(); s }
            other => {
                return Err(ParseError::Expected {
                    expected: "message name".to_string(),
                    found: other,
                    span: self.peek_span(),
                });
            }
        };
        // Optional `(name: Type, ...)`
        let mut params = Vec::new();
        if self.check(&Token::LParen) {
            self.advance();
            params = self.parse_param_list()?;
            self.expect(Token::RParen)?;
        }
        if !roles.contains(&from) { roles.push(from.clone()); }
        if !roles.contains(&to) { roles.push(to.clone()); }
        Ok(Spanned::new(
            ProtocolStep::Send { from, to, message, params },
            start.merge(self.prev_span()),
        ))
    }

    // ── V1: Adversary (declarative threat model) ──────────────────────

    fn parse_adversary_section(&mut self) -> Result<AdversarySection, ParseError> {
        self.expect(Token::Adversary)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            let (key, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            // Value is a free-form line until newline / next key.
            // We accumulate token text until we hit an ident followed by ':'
            // (the next key) or the closing brace.
            let value = self.parse_adversary_value()?;
            fields.push(Spanned::new(
                AdversaryField { key, value },
                start.merge(self.prev_span()),
            ));
        }
        self.expect(Token::RBrace)?;
        Ok(AdversarySection { name, fields })
    }

    fn parse_adversary_value(&mut self) -> Result<String, ParseError> {
        let mut parts: Vec<String> = Vec::new();
        let mut paren_depth: i32 = 0;
        loop {
            // Stop if we hit the closing `}` of the parent block (only when not nested in parens).
            if paren_depth == 0 && self.check(&Token::RBrace) { break; }
            // Lookahead: ident followed by `:` means a new key starts here.
            if paren_depth == 0 {
                if let Token::Ident(_) = self.peek().clone() {
                    if self.pos + 1 < self.tokens.len()
                        && matches!(self.tokens[self.pos + 1].token, Token::Colon)
                    {
                        break;
                    }
                }
            }
            let tok = self.peek().clone();
            self.advance();
            // Render any token as text so invariant formulas survive intact.
            let rendered = match tok {
                Token::Ident(s) | Token::TypeIdent(s) | Token::StringLit(s) => s,
                Token::IntLit(n) => n.to_string(),
                Token::BigIntLit(s) => s,
                Token::FloatLit(f) => f.to_string(),
                Token::PercentLit(p) => format!("{}%", p),
                Token::Percent => "%".to_string(),
                Token::DurationLit(v, u) => {
                    let unit = match u {
                        crate::lexer::DurationUnitTok::Ms => "ms",
                        crate::lexer::DurationUnitTok::S => "s",
                        crate::lexer::DurationUnitTok::Min => "min",
                        crate::lexer::DurationUnitTok::H => "h",
                        crate::lexer::DurationUnitTok::D => "d",
                        crate::lexer::DurationUnitTok::Years => "y",
                    };
                    format!("{}{}", v, unit)
                }
                Token::LParen => { paren_depth += 1; "(".to_string() }
                Token::RParen => { paren_depth -= 1; ")".to_string() }
                Token::Comma => ",".to_string(),
                Token::Lt => "<".to_string(),
                Token::Gt => ">".to_string(),
                Token::Le => "≤".to_string(),
                Token::Ge => "≥".to_string(),
                Token::EqEq => "==".to_string(),
                Token::Ne => "≠".to_string(),
                Token::Eq => "=".to_string(),
                Token::Plus => "+".to_string(),
                Token::Minus => "-".to_string(),
                Token::Star => "*".to_string(),
                Token::Slash => "/".to_string(),
                Token::AndAnd => "∧".to_string(),
                Token::OrOr => "∨".to_string(),
                Token::Bang => "¬".to_string(),
                Token::Arrow => "→".to_string(),
                Token::FatArrow => "⇒".to_string(),
                Token::Implies => "implies".to_string(),
                Token::Requires => "requires".to_string(),
                Token::Contradicts => "contradicts".to_string(),
                Token::True => "true".to_string(),
                Token::False => "false".to_string(),
                Token::If => "if".to_string(),
                Token::Else => "else".to_string(),
                Token::Where => "where".to_string(),
                Token::In => "in".to_string(),
                Token::Eof => break,
                _ => continue,
            };
            parts.push(rendered);
        }
        Ok(parts.join(" "))
    }

    // ── V1: Prove (exportable proof witness) ──────────────────────────

    fn parse_prove_section(&mut self) -> Result<ProveSection, ParseError> {
        self.expect(Token::Prove)?;
        let (target, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut invariants = Vec::new();
        let mut export: Option<ProveExport> = None;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            let (key, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            match key.as_str() {
                "invariant" | "safety" | "liveness" => {
                    let label = if key == "invariant" { String::new() } else { key.clone() };
                    let formula = self.parse_adversary_value()?;
                    invariants.push(Spanned::new(
                        ProveInvariant { label, formula },
                        start.merge(self.prev_span()),
                    ));
                }
                "export" => {
                    // export: lean4 -> "proofs/payment.lean"
                    let (backend, _) = self.expect_ident()?;
                    self.expect(Token::Arrow)?;
                    let path = match self.peek().clone() {
                        Token::StringLit(s) => { self.advance(); s }
                        other => {
                            return Err(ParseError::Expected {
                                expected: "string path for export".to_string(),
                                found: other,
                                span: self.peek_span(),
                            });
                        }
                    };
                    export = Some(ProveExport { backend, path });
                }
                other => {
                    return Err(ParseError::Expected {
                        expected: "invariant, safety, liveness, or export".to_string(),
                        found: Token::Ident(other.to_string()),
                        span: self.prev_span(),
                    });
                }
            }
        }
        self.expect(Token::RBrace)?;
        Ok(ProveSection { target, invariants, export })
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

        // Parse optional handler properties: [native], [native, pure], etc.
        let mut properties = Vec::new();
        if self.check(&Token::LBracket) {
            self.advance(); // consume [
            loop {
                if self.check(&Token::RBracket) {
                    self.advance();
                    break;
                }
                let (prop_name, _) = self.expect_ident()?;
                properties.push(prop_name);
                if self.check(&Token::Comma) {
                    self.advance();
                } else {
                    self.expect(Token::RBracket)?;
                    break;
                }
            }
        }

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
            properties,
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
            Token::Ensure => {
                // ensure condition — postcondition checked on handler exit
                self.advance();
                let condition = self.parse_expr()?;
                Ok(Spanned::new(
                    Statement::Ensure { condition },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Break => {
                self.advance();
                Ok(Spanned::new(Statement::Break, start.merge(self.prev_span())))
            }
            Token::Continue => {
                self.advance();
                Ok(Spanned::new(Statement::Continue, start.merge(self.prev_span())))
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
            Token::Signal | Token::Emit => {
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
                // Accept either an identifier or a string literal as the else target
                let else_signal = match self.peek() {
                    Token::StringLit(s) => {
                        let s = s.clone();
                        self.advance();
                        s
                    }
                    _ => {
                        let (name, _) = self.expect_ident()?;
                        name
                    }
                };
                Ok(Spanned::new(
                    Statement::Require {
                        constraint,
                        else_signal,
                    },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Ident(_) | Token::State | Token::Type | Token::Effect | Token::Guard | Token::Initial |
            Token::Start | Token::Test | Token::Backend | Token::Builtin |
            Token::Check | Token::Memory | Token::Face |
            Token::Property | Token::Rules | Token::Runtime | Token::Matches |
            Token::Native | Token::Checker | Token::Scale => {
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
                } else if self.check(&Token::PlusEq)
                    || self.check(&Token::MinusEq)
                    || self.check(&Token::StarEq)
                    || self.check(&Token::SlashEq)
                {
                    // name +=/-=/*=//= expr → name = name op expr
                    let op = match self.peek() {
                        Token::PlusEq => BinOp::Add,
                        Token::MinusEq => BinOp::Sub,
                        Token::StarEq => BinOp::Mul,
                        Token::SlashEq => BinOp::Div,
                        _ => unreachable!(),
                    };
                    self.advance();
                    let rhs = self.parse_expr()?;
                    let rhs_span = rhs.span;
                    let value = Spanned::new(
                        Expr::BinaryOp {
                            left: Box::new(Spanned::new(Expr::Ident(name.clone()), name_span)),
                            op,
                            right: Box::new(rhs),
                        },
                        name_span.merge(rhs_span),
                    );
                    Ok(Spanned::new(
                        Statement::Assign { name, value },
                        start.merge(self.prev_span()),
                    ))
                } else if self.check(&Token::Dot) {
                    // target.method(args) — parse as full expression to allow pipes
                    self.pos = save_pos;
                    let expr = self.parse_expr()?;
                    Ok(Spanned::new(
                        Statement::ExprStmt { expr },
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
            // Tokens that can start an expression: match, literals, parenthesized, etc.
            Token::Match | Token::Try |
            Token::IntLit(_) | Token::BigIntLit(_) | Token::FloatLit(_) | Token::StringLit(_) |
            Token::True | Token::False | Token::LParen |
            Token::Bang | Token::Minus | Token::LBracket => {
                let expr = self.parse_expr()?;
                Ok(Spanned::new(
                    Statement::ExprStmt { expr },
                    start.merge(self.prev_span()),
                ))
            }
            _ => Err(ParseError::Expected {
                expected: "statement (let, return, if, match, signal, require, or expression)".to_string(),
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
            _ => {
                // Treat any non-ident expression as a boolean comparison: expr == true
                let right_span = left.span;
                Ok(Spanned::new(
                    Constraint::Comparison {
                        left,
                        op: CmpOp::Eq,
                        right: Spanned::new(Expr::Literal(Literal::Bool(true)), right_span),
                    },
                    right_span,
                ))
            }
        }
    }

    // ── Expressions ──────────────────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_logical_or()?;
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_logical_or()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(Expr::Pipe {
                left: Box::new(left),
                right: Box::new(right),
            }, span);
        }
        Ok(left)
    }

    fn parse_logical_or(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_logical_and()?;
        while self.check(&Token::OrOr) {
            self.advance();
            let right = self.parse_logical_and()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Or,
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_null_coalesce()?;
        while self.check(&Token::AndAnd) {
            self.advance();
            let right = self.parse_null_coalesce()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::And,
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_null_coalesce(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_comparison()?;
        while self.check(&Token::NullCoal) {
            self.advance();
            let right = self.parse_comparison()?;
            let span = left.span.merge(right.span);
            // a ?? b �� if a == () then b else a
            // Desugar to FnCall("_coalesce", [a, b])
            left = Spanned::new(Expr::FnCall {
                name: "_coalesce".to_string(),
                args: vec![left, right],
            }, span);
        }
        Ok(left)
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
        if self.check(&Token::Minus) {
            let start = self.peek_span();
            self.advance();
            // Optimise: if the next token is an integer or float literal,
            // fold the negation directly so that i64::MIN (-9223372036854775808)
            // can be written as a literal without overflow.
            match self.peek().clone() {
                Token::IntLit(n) => {
                    let end = self.peek_span();
                    self.advance();
                    let span = start.merge(end);
                    return Ok(Spanned::new(
                        Expr::Literal(Literal::Int(n.wrapping_neg())),
                        span,
                    ));
                }
                Token::BigIntLit(ref s) => {
                    let neg = format!("-{}", s);
                    let end = self.peek_span();
                    self.advance();
                    let span = start.merge(end);
                    return Ok(Spanned::new(
                        Expr::Literal(Literal::BigInt(neg)),
                        span,
                    ));
                }
                Token::FloatLit(n) => {
                    let end = self.peek_span();
                    self.advance();
                    let span = start.merge(end);
                    return Ok(Spanned::new(
                        Expr::Literal(Literal::Float(-n)),
                        span,
                    ));
                }
                _ => {}
            }
            let expr = self.parse_unary()?;
            let span = start.merge(expr.span);
            return Ok(Spanned::new(
                Expr::BinaryOp {
                    left: Box::new(Spanned::new(Expr::Literal(Literal::Int(0)), start)),
                    op: BinOp::Sub,
                    right: Box::new(expr),
                },
                span,
            ));
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
            } else if self.check(&Token::Question) {
                // Postfix ? operator: expr? — propagate error
                self.advance();
                let span = expr.span.merge(self.prev_span());
                expr = Spanned::new(Expr::TryPropagate(Box::new(expr)), span);
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
            Token::BigIntLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::BigInt(s)), start))
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
                // Check for lambda: name => expr  OR  name => { stmts; expr }
                if self.check(&Token::FatArrow) {
                    self.advance();
                    if self.check(&Token::LBrace) {
                        // Block lambda: s => { let x = ...; expr }
                        self.advance();
                        let mut stmts = Vec::new();
                        while !self.check(&Token::RBrace) && !self.is_at_end() {
                            stmts.push(self.parse_statement()?);
                        }
                        self.expect(Token::RBrace)?;
                        // Extract last statement as result expression
                        let result = if let Some(last) = stmts.last() {
                            if let Statement::ExprStmt { ref expr } = last.node {
                                expr.clone()
                            } else if let Statement::Return { ref value } = last.node {
                                value.clone()
                            } else {
                                Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                            }
                        } else {
                            Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                        };
                        // Wrap: body is a block that runs stmts (minus last) then evaluates result
                        // For now, encode as nested Let bindings wrapping the result
                        // Actually, we need a Block expression. Let's reuse Match arms approach:
                        // Store stmts in lambda env by creating a special FnCall
                        // Simpler: just use the last expr as body, with preceding stmts as a separate list
                        // We need to extend Lambda to support a body with statements.
                        // For now, wrap in a special form: if there are preceding stmts,
                        // we create nested lambdas. Actually simplest: extend Expr::Lambda with optional stmts.

                        // Quick approach: generate a synthetic expression that evaluates stmts then result
                        // by wrapping in FnCall to a synthetic block handler
                        // ACTUALLY: let's just put all stmts except last into the body, and the last expr is result.
                        // But Lambda only has body: Expr. We need to change Lambda to support statements.
                        // Let's do it properly:
                        if stmts.len() <= 1 {
                            return Ok(Spanned::new(
                                Expr::Lambda { param: name, body: Box::new(result) },
                                start.merge(self.prev_span()),
                            ));
                        }
                        // Multiple statements: we need LambdaBlock. Add it to AST.
                        // For now, chain lets: let a = x; let b = y; expr → expr (with env setup)
                        // We'll extend Lambda to include statements.
                        let body_stmts: Vec<Spanned<Statement>> = stmts[..stmts.len()-1].to_vec();
                        return Ok(Spanned::new(
                            Expr::LambdaBlock { param: name, stmts: body_stmts, result: Box::new(result) },
                            start.merge(self.prev_span()),
                        ));
                    } else {
                        let body = self.parse_pipe()?;
                        return Ok(Spanned::new(
                            Expr::Lambda { param: name, body: Box::new(body) },
                            start.merge(self.prev_span()),
                        ));
                    }
                }
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
                // Check for record literal: User { name: "Alice", age: 30 }
                if self.check(&Token::LBrace) {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&Token::RBrace) && !self.is_at_end() {
                        let (field_name, _) = self.expect_ident()?;
                        self.expect(Token::Colon)?;
                        let value = self.parse_expr()?;
                        fields.push((field_name, value));
                        if self.check(&Token::Comma) { self.advance(); }
                    }
                    self.expect(Token::RBrace)?;
                    Ok(Spanned::new(Expr::Record { type_name: name, fields }, start.merge(self.prev_span())))
                } else {
                    Ok(Spanned::new(Expr::Ident(name), start))
                }
            }
            // Keywords that can be used as variable/function names in expressions
            Token::State | Token::Type | Token::Effect | Token::Guard | Token::Initial |
            Token::Start | Token::Test | Token::Backend | Token::Builtin |
            Token::Signal | Token::Emit | Token::Check | Token::Memory | Token::Face |
            Token::Property | Token::Rules | Token::Runtime | Token::Matches |
            Token::Native | Token::Checker | Token::Scale => {
                let name = format!("{:?}", self.peek()).to_lowercase();
                // Get the keyword as a string name
                let name = match self.peek() {
                    Token::State => "state", Token::Type => "type", Token::Effect => "effect",
                    Token::Guard => "guard", Token::Initial => "initial", Token::Start => "start",
                    Token::Test => "test", Token::Backend => "backend", Token::Builtin => "builtin",
                    Token::Signal => "signal", Token::Emit => "emit", Token::Check => "check", Token::Memory => "memory",
                    Token::Face => "face", Token::Property => "property", Token::Rules => "rules",
                    Token::Runtime => "runtime", Token::Matches => "matches", Token::Native => "native",
                    Token::Checker => "checker", Token::Scale => "scale",
                    _ => unreachable!(),
                }.to_string();
                self.advance();
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
            Token::If => {
                // If expression: if cond { stmts; expr } else { stmts; expr }
                self.advance();
                let condition = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut then_stmts = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    then_stmts.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                // Extract last statement as result expression
                let then_result = if let Some(last) = then_stmts.last() {
                    if let Statement::ExprStmt { ref expr } = last.node {
                        expr.clone()
                    } else if let Statement::Return { ref value } = last.node {
                        value.clone()
                    } else {
                        Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                    }
                } else {
                    Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                };
                if then_stmts.len() > 0 {
                    if let Some(last) = then_stmts.last() {
                        if matches!(last.node, Statement::ExprStmt { .. } | Statement::Return { .. }) {
                            then_stmts.pop();
                        }
                    }
                }
                self.expect(Token::Else)?;
                // Handle else if by recursing
                let (else_stmts, else_result) = if self.check(&Token::If) {
                    // else if → treat as nested if expression
                    let nested = self.parse_primary_expr()?;
                    (Vec::new(), nested)
                } else {
                    self.expect(Token::LBrace)?;
                    let mut else_stmts = Vec::new();
                    while !self.check(&Token::RBrace) && !self.is_at_end() {
                        else_stmts.push(self.parse_statement()?);
                    }
                    self.expect(Token::RBrace)?;
                    let else_result = if let Some(last) = else_stmts.last() {
                        if let Statement::ExprStmt { ref expr } = last.node {
                            expr.clone()
                        } else if let Statement::Return { ref value } = last.node {
                            value.clone()
                        } else {
                            Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                        }
                    } else {
                        Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                    };
                    if else_stmts.len() > 0 {
                        if let Some(last) = else_stmts.last() {
                            if matches!(last.node, Statement::ExprStmt { .. } | Statement::Return { .. }) {
                                else_stmts.pop();
                            }
                        }
                    }
                    (else_stmts, else_result)
                };
                Ok(Spanned::new(
                    Expr::IfExpr {
                        condition: Box::new(condition),
                        then_body: then_stmts,
                        then_result: Box::new(then_result),
                        else_body: else_stmts,
                        else_result: Box::new(else_result),
                    },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Match => {
                self.advance();
                let subject = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut arms = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    // Parse pattern: literal, _ (wildcard), variable, () (unit), negative number
                    // Supports or-patterns: "a" | "b" | "c"
                    let pattern = self.parse_match_pattern()?;

                    // Check for or-pattern: pat || pat || ...
                    let pattern = if self.check(&Token::OrOr) {
                        let mut alternatives = vec![pattern];
                        while self.check(&Token::OrOr) {
                            self.advance();
                            alternatives.push(self.parse_match_pattern()?);
                        }
                        MatchPattern::Or(alternatives)
                    } else {
                        pattern
                    };

                    // Optional guard clause: pattern if condition -> result
                    let guard = if self.check(&Token::If) {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };

                    self.expect(Token::Arrow)?;

                    // Parse body: either a block { stmts; result_expr } or single expression
                    if self.check(&Token::LBrace) {
                        self.advance();
                        let mut stmts = Vec::new();
                        while !self.check(&Token::RBrace) && !self.is_at_end() {
                            stmts.push(self.parse_statement()?);
                        }
                        self.expect(Token::RBrace)?;
                        let result = if let Some(last) = stmts.pop() {
                            match last.node {
                                Statement::ExprStmt { expr } => expr,
                                _ => {
                                    stmts.push(last);
                                    Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                                }
                            }
                        } else {
                            Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                        };
                        arms.push(MatchArm { pattern, guard, body: stmts, result });
                    } else {
                        let result = self.parse_expr()?;
                        arms.push(MatchArm { pattern, guard, body: vec![], result });
                    }
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(
                    Expr::Match { subject: Box::new(subject), arms },
                    start.merge(self.prev_span()),
                ))
            }
            Token::Try => {
                // try { expr } → returns map("value", result) or map("error", message)
                self.advance();
                self.expect(Token::LBrace)?;
                let expr = self.parse_expr()?;
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(Expr::Try(Box::new(expr)), start.merge(self.prev_span())))
            }
            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(&Token::RBracket) && !self.is_at_end() {
                    elements.push(self.parse_expr()?);
                    if self.check(&Token::Comma) { self.advance(); }
                }
                self.expect(Token::RBracket)?;
                Ok(Spanned::new(Expr::ListLiteral(elements), start.merge(self.prev_span())))
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

    /// Parse a single match pattern (without or-patterns, which are handled by the caller)
    fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        if self.check(&Token::LParen) {
            // () — unit/null pattern
            self.advance();
            self.expect(Token::RParen)?;
            Ok(MatchPattern::Literal(Literal::Unit))
        } else if self.check(&Token::LBrace) {
            // Map destructuring: {method: "GET", path, body}
            self.advance();
            let mut fields = Vec::new();
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let (field_name, _) = self.expect_ident()?;
                if self.check(&Token::Colon) {
                    // {field: pattern} — match field against a sub-pattern
                    self.advance();
                    let sub = self.parse_match_pattern()?;
                    fields.push((field_name, sub));
                } else {
                    // {field} — shorthand for binding: bind field value to variable `field`
                    fields.push((field_name.clone(), MatchPattern::Variable(field_name)));
                }
                if self.check(&Token::Comma) { self.advance(); }
            }
            self.expect(Token::RBrace)?;
            Ok(MatchPattern::MapDestructure(fields))
        } else if let Token::Ident(ref id) = self.peek().clone() {
            if id == "_" {
                self.advance();
                Ok(MatchPattern::Wildcard)
            } else {
                // Variable binding: capture the matched value
                let name = id.clone();
                self.advance();
                Ok(MatchPattern::Variable(name))
            }
        } else if self.check(&Token::Minus) {
            // Negative number pattern: -5, -3.14
            self.advance();
            let lit = self.parse_literal()?;
            match lit.node {
                Literal::Int(n) => Ok(MatchPattern::Literal(Literal::Int(-n))),
                Literal::BigInt(s) => Ok(MatchPattern::Literal(Literal::BigInt(format!("-{}", s)))),
                Literal::Float(n) => Ok(MatchPattern::Literal(Literal::Float(-n))),
                _ => Err(ParseError::Expected {
                    expected: "number after '-' in match pattern".to_string(),
                    found: self.peek().clone(),
                    span: self.peek_span(),
                }),
            }
        } else {
            // Literal pattern — check for range (1..10) or string prefix ("prefix" + rest)
            let lit = self.parse_literal()?;
            // Range pattern: 1..10
            if let Literal::Int(from) = lit.node {
                if self.check(&Token::DotDot) {
                    self.advance();
                    if let Token::IntLit(to) = self.peek().clone() {
                        self.advance();
                        return Ok(MatchPattern::Range { from, to });
                    }
                }
            }
            // String prefix pattern: "prefix" + rest
            if let Literal::String(ref prefix) = lit.node {
                if self.check(&Token::Plus) {
                    self.advance();
                    if let Token::Ident(ref rest_name) = self.peek().clone() {
                        let rest = rest_name.clone();
                        self.advance();
                        return Ok(MatchPattern::StringPrefix { prefix: prefix.clone(), rest });
                    }
                }
            }
            Ok(MatchPattern::Literal(lit.node))
        }
    }

    fn parse_literal(&mut self) -> Result<Spanned<Literal>, ParseError> {
        let start = self.peek_span();
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Spanned::new(Literal::Int(n), start))
            }
            Token::BigIntLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Spanned::new(Literal::BigInt(s), start))
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
