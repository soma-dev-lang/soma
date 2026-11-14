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
        // not the Rust name `PercentLit(50.0)`
        Token::PercentLit(p) => format!("percentage '{}%'", p),
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
        Token::PlusPlus => "'++'".to_string(),
        Token::Try => "'try'".to_string(),
        Token::Catch => "'catch'".to_string(),
        Token::Every => "'every'".to_string(),
        Token::After => "'after'".to_string(),
        Token::Ensure => "'ensure'".to_string(),
        Token::Tool => "'tool'".to_string(),
        Token::AgentKw => "'agent'".to_string(),
        Token::Scale => "'scale'".to_string(),
        Token::Eof => "end of file".to_string(),
        // never a Rust name (`DurationLit(5.0, S)`, `DotDot`) in a message
        Token::DurationLit(v, u, _) => format!("duration '{}{}'", v, match u {
            DurationUnitTok::Ms => "ms", DurationUnitTok::S => "s", DurationUnitTok::Min => "min",
            DurationUnitTok::H => "h", DurationUnitTok::D => "d", DurationUnitTok::Years => "years" }),
        Token::DotDot => "'..'".to_string(),
        other => {
            let name = format!("{:?}", other);
            let base = name.split('(').next().unwrap_or(&name).to_lowercase();
            format!("'{}'", base)
        }
    }
}

/// Render a TypeExpr back to source form (for fix-it messages).
/// The handler a skeleton signal has until a flesh fills it: it fails,
/// naming the signal. The property `hole` is how every tool tells it from
/// a body someone wrote.
fn hole_handler(cell: &str, signal: &str, params: Vec<Param>, span: Span) -> OnSection {
    let lit = |t: String| Spanned::new(Expr::Literal(Literal::String(t)), span);
    let call = Expr::FnCall {
        name: "fail".to_string(),
        args: vec![
            lit("not_implemented".to_string()),
            lit(format!("{}.{} is a hole of the skeleton: no flesh gives it a body yet", cell, signal)),
        ],
    };
    OnSection {
        signal_name: signal.to_string(),
        params,
        body: vec![Spanned::new(Statement::Return { value: Spanned::new(call, span) }, span)],
        properties: vec![HOLE.to_string()],
    }
}

fn type_expr_source(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Simple(name) => name.clone(),
        TypeExpr::Generic { name, args } => {
            let inner: Vec<String> = args.iter().map(|a| type_expr_source(&a.node)).collect();
            format!("{}<{}>", name, inner.join(", "))
        }
        TypeExpr::CellRef { cell, member } => format!("{}.{}", cell, member),
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
    /// A near-miss with a known correction. The message IS the fix:
    /// it tells the writer exactly what to type instead.
    #[error("{message}")]
    FixIt {
        message: String,
        span: Span,
    },
}

impl ParseError {
    pub fn span(&self) -> Option<Span> {
        match self {
            ParseError::Expected { span, .. } => Some(*span),
            ParseError::FixIt { span, .. } => Some(*span),
            ParseError::UnexpectedEof => None,
        }
    }
}

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    /// expression nesting, to refuse a 5 000-deep `((((…` with an error
    /// instead of a stack overflow abort
    depth: usize,
    /// Open `(` / `[` around the expression being parsed (reset inside each
    /// statement of a block): there no new statement can start, so a line
    /// starting with `-2` continues the expression (`(a\n -2)`).
    bracket_depth: usize,
    /// Recovery mode (`parse_program_recovering`): a syntax error is recorded
    /// and parsing resumes at the next statement / section / cell, so one
    /// pass reports every error of the file.
    recover: bool,
    /// (error, the AST is still complete) — complete: a fix-it whose intent
    /// was unambiguous (`=>` in a match arm, `-> T` on a handler), parsed as
    /// if written correctly, so the semantic checks can still run.
    recovered: Vec<(ParseError, bool)>,
    /// open `{` before each token (recovery resynchronises on it)
    depth_at: Vec<u32>,
    /// `const NAME = literal` of the cell being parsed (3.0): a constant IS
    /// its literal — every read is replaced here, so an invariant, a
    /// `require`, a guard and the prover all see the number itself.
    consts: std::collections::HashMap<String, Literal>,
    /// constants declared through a nominal type: `const MAX = Lots(50)`
    const_nominal: std::collections::HashMap<String, String>,
}

const MAX_EXPR_DEPTH: usize = 400;
/// Past this many syntax errors the rest of the file is not worth guessing.
const MAX_SYNTAX_ERRORS: usize = 25;

/// Keywords that double as field / variable names.
fn keyword_as_name(t: &Token) -> Option<String> {
    Some(match t {
        Token::State => "state", Token::Type => "type", Token::Effect => "effect",
        Token::Guard => "guard", Token::Initial => "initial", Token::Start => "start",
        Token::Test => "test", Token::Backend => "backend", Token::Builtin => "builtin",
        Token::Signal => "signal", Token::Emit => "emit", Token::Check => "check", Token::Memory => "memory",
        Token::Face => "face", Token::Property => "property", Token::Rules => "rules",
        Token::Runtime => "runtime", Token::Matches => "matches", Token::Native => "native",
        Token::Checker => "checker", Token::Scale => "scale", Token::Cost => "cost", Token::Protocol => "protocol",
        _ => return None,
    }.to_string())
}

/// A keyword that can never be a name: the error says so (it used to be a
/// bare "expected identifier, found 'agent'").
fn reserved_word(t: &Token) -> Option<&'static str> {
    Some(match t {
        Token::Cell => "cell", Token::Interior => "interior", Token::Given => "given",
        Token::Promise => "promise", Token::Tool => "tool", Token::AgentKw => "agent",
        Token::Await => "await", Token::On => "on", Token::Where => "where", Token::Else => "else",
        Token::Require => "require", Token::Let => "let", Token::If => "if", Token::Return => "return",
        Token::For => "for", Token::In => "in", Token::Use => "use", Token::While => "while",
        Token::Break => "break", Token::Continue => "continue", Token::Try => "try", Token::Catch => "catch",
        Token::Assert => "assert", Token::MutexGroup => "mutex_group",
        _ => return None,
    })
}

fn reserved_error(t: &Token, span: Span) -> Option<ParseError> {
    reserved_word(t).map(|w| ParseError::FixIt {
        // `x => require ok else Nope`: a statement where a value is expected
        // a lambda's value is its expression: `x => return x + 1` is `x => x + 1`
        // (the block form suggested before refuses `return` too)
        message: if w == "return" {
            "`return` starts a statement and cannot stand where a value is expected (a lambda `x => …`, an operand, an argument) — a lambda's value is its expression: drop the `return` (`x => x + 1`, or `x => { let y = x * 2  y + 1 }`); as a name it is reserved (`return_id`)".to_string()
        } else if matches!(w, "require" | "let" | "for" | "while" | "break" | "continue") {
            format!("`{w}` starts a statement and cannot stand where a value is expected (a lambda `x => …`, an operand, an argument) — in a lambda write a block (`x => {{ {w} …  x }}`), or move it into the handler body; as a name it is reserved (`{w}_id`)")
        } else {
            format!("`{w}` is a reserved word in Soma and cannot name a variable, parameter or field — rename it (`{w}_id`, `the_{w}`); to read a JSON field of that name write `d[\"{w}\"]`")
        },
        span,
    })
}

thread_local! {
    /// The constants of the skeletons parsed so far, by skeleton name: a
    /// `flesh Name` reads them (`if amount > LIMIT`) — in the same file, in
    /// any order, or in a file the skeleton's file imports.
    static SKELETON_CONSTS: std::cell::RefCell<std::collections::HashMap<String, std::collections::HashMap<String, Literal>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, pos: 0, depth: 0, bracket_depth: 0, recover: false, recovered: Vec::new(), depth_at: Vec::new(), consts: std::collections::HashMap::new(), const_nominal: std::collections::HashMap::new() }
    }

    /// Parse the whole file, reporting EVERY syntax error: `(program, errors,
    /// complete)`. `complete` is true when each error left its construct in
    /// the AST as the writer meant it — the program can then be checked.
    pub fn parse_program_recovering(&mut self) -> (Program, Vec<ParseError>, bool) {
        self.recover = true;
        let mut d = 0u32;
        self.depth_at = self.tokens.iter().map(|t| {
            let before = d;
            match t.token {
                Token::LBrace => d += 1,
                Token::RBrace => d = d.saturating_sub(1),
                _ => {}
            }
            before
        }).collect();
        let program = match self.parse_program() {
            Ok(p) => p,
            Err(e) => {
                self.recovered.push((e, false));
                Program { nominal_issues: Vec::new(), imports: Vec::new(), cells: Vec::new(), protocols: Vec::new() }
            }
        };
        let complete = self.recovered.iter().all(|(_, c)| *c);
        let mut errors: Vec<ParseError> = std::mem::take(&mut self.recovered).into_iter().map(|(e, _)| e).collect();
        // source order (a section error is recorded after its statements')
        errors.sort_by_key(|e| e.span().map_or(usize::MAX, |s| s.start));
        (program, errors, complete)
    }

    fn can_recover(&self) -> bool {
        self.recover && self.recovered.len() < MAX_SYNTAX_ERRORS
    }

    /// The value after `const NAME =`: a literal, or a nominal type around
    /// one (`Lots(50)`) — (the nominal type, the literal, tokens used).
    fn const_value_at(&self, at: usize) -> Option<(Option<String>, Literal, usize)> {
        if let (Some(Token::Ident(n) | Token::TypeIdent(n)), Some(Token::LParen)) =
            (self.tokens.get(at).map(|t| &t.token), self.tokens.get(at + 1).map(|t| &t.token)) {
            // a type's name (`Lots(50)`), not a call (`len("a")` stays refused)
            if !n.starts_with(|c: char| c.is_ascii_uppercase()) { return None; }
            let (lit, used) = self.const_literal_at(at + 2)?;
            if !matches!(self.tokens.get(at + 2 + used).map(|t| &t.token), Some(Token::RParen)) { return None; }
            return Some((Some(n.clone()), lit, used + 3));
        }
        self.const_literal_at(at).map(|(l, u)| (None, l, u))
    }

    /// The literal after `const NAME =`, starting at token `at`: (literal, tokens used).
    fn const_literal_at(&self, at: usize) -> Option<(Literal, usize)> {
        let tok = |i: usize| self.tokens.get(i).map(|t| &t.token);
        let (neg, at2) = if matches!(tok(at), Some(Token::Minus)) { (true, at + 1) } else { (false, at) };
        let lit = match tok(at2)? {
            Token::IntLit(n) => Literal::Int(if neg { n.checked_neg()? } else { *n }),
            Token::FloatLit(x) => Literal::Float(if neg { -*x } else { *x }),
            Token::BigIntLit(b) if !neg => Literal::BigInt(b.clone()),
            Token::StringLit(t) if !neg && !t.contains('{') => Literal::String(t.clone()),
            Token::True if !neg => Literal::Bool(true),
            Token::False if !neg => Literal::Bool(false),
            _ => return None,
        };
        Some((lit, at2 + 1 - at))
    }

    /// The constants a cell declares, read ahead of its sections (a handler
    /// above the `const` line uses it too).
    fn prescan_consts(&mut self) {
        let mut depth = 0i32;
        let mut i = self.pos;
        while let Some(t) = self.tokens.get(i) {
            match &t.token {
                Token::LBrace => depth += 1,
                Token::RBrace => { depth -= 1; if depth < 0 { break; } }
                Token::Eof => break,
                Token::Ident(w) if depth == 0 && w == "const" => {
                    if let (Some(Token::Ident(name) | Token::TypeIdent(name)), Some(Token::Eq)) =
                        (self.tokens.get(i + 1).map(|t| &t.token), self.tokens.get(i + 2).map(|t| &t.token)) {
                        if let Some((nominal, lit, _)) = self.const_value_at(i + 3) {
                            self.consts.entry(name.clone()).or_insert(lit);
                            match nominal {
                                Some(n) => { self.const_nominal.entry(name.clone()).or_insert(n); }
                                // an interior cell's plain constant of the same name
                                None => { self.const_nominal.remove(name); }
                            }
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// `const NAME = literal` inside a cell.
    fn parse_const(&mut self) -> Result<(), ParseError> {
        let kw = self.peek_span();
        self.advance(); // const
        let (name, name_span) = match self.peek().clone() {
            Token::Ident(n) | Token::TypeIdent(n) => { let sp = self.peek_span(); self.advance(); (n, sp) }
            other => return Err(ParseError::Expected { expected: "a constant name (`const LIMIT = 500`)".to_string(), found: other, span: self.peek_span() }),
        };
        if !self.check(&Token::Eq) {
            return Err(ParseError::FixIt {
                message: format!("`const {name}` needs a value — `const {name} = 500` (a number, a String or a Bool literal)"),
                span: self.peek_span(),
            });
        }
        self.advance();
        let Some((_, lit, used)) = self.const_value_at(self.pos) else {
            return Err(ParseError::FixIt {
                message: format!("`const {name}` takes a literal — a number, a String without interpolation, or a Bool (`const {name} = 500`); a computed value belongs in a handler: `on _{}() {{ return … }}`", name.to_lowercase()),
                span: self.peek_span(),
            });
        };
        for _ in 0..used { self.advance(); }
        if !self.at_line_start() && matches!(self.peek(), Token::Plus | Token::Minus | Token::Star | Token::Slash | Token::LParen | Token::Dot) {
            return Err(ParseError::FixIt {
                message: format!("`const {name}` takes a literal, not an expression — write the value itself (`const {name} = 500`); a computed value belongs in a handler"),
                span: self.peek_span(),
            });
        }
        // one definition: the first is the one every read was given
        if self.consts.get(&name).map_or(true, |first| format!("{:?}", first) != format!("{:?}", lit)) {
            return Err(ParseError::FixIt {
                message: format!("`const {name}` is declared twice in this cell with different values — keep one"),
                span: name_span,
            });
        }
        let _ = kw;
        Ok(())
    }

    /// A constant's name cannot be bound again: its reads are the literal.
    fn refuse_const(&self, name: &str, span: Span, what: &str) -> Result<(), ParseError> {
        if self.consts.contains_key(name) {
            return Err(ParseError::FixIt {
                message: format!("`{name}` is a constant of this cell (`const {name} = …`) and cannot name {what} — rename one of them"),
                span,
            });
        }
        Ok(())
    }

    /// `"up to {LIMIT} rows"`: a constant read inside an interpolation.
    fn consts_in_string(&self, text: &str) -> String {
        if self.consts.is_empty() || !text.contains('{') { return text.to_string(); }
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let (mut i, mut depth) = (0usize, 0usize);
        while i < chars.len() {
            let c = chars[i];
            // `{{` is a literal brace
            if depth == 0 && c == '{' && chars.get(i + 1) == Some(&'{') { out.push_str("{{"); i += 2; continue; }
            if c == '{' { depth += 1; out.push(c); i += 1; continue; }
            if c == '}' { depth = depth.saturating_sub(1); out.push(c); i += 1; continue; }
            let word_start = c.is_alphabetic() || c == '_';
            let prev_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '.' || chars[i - 1] == '"');
            if depth > 0 && word_start && prev_ok {
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                let word: String = chars[i..j].iter().collect();
                let called = chars.get(j) == Some(&'(');
                // `{LIMIT}` alone: the text itself replaces the braces too
                let alone = depth == 1 && i > 0 && chars[i - 1] == '{' && chars.get(j) == Some(&'}') && out.ends_with('{');
                match self.consts.get(&word) {
                    Some(lit) if !called => {
                        let (plain, source) = match lit {
                            Literal::Int(n) => (n.to_string(), n.to_string()),
                            Literal::Float(x) => (format!("{:?}", x), format!("{:?}", x)),
                            Literal::BigInt(b) => (b.clone(), b.clone()),
                            Literal::Bool(b) => (b.to_string(), b.to_string()),
                            Literal::String(t) => (t.clone(), format!("\"{}\"", t)),
                            _ => (word.clone(), word.clone()),
                        };
                        if alone {
                            out.pop();
                            out.push_str(&plain);
                            depth -= 1;
                            i = j + 1;
                            continue;
                        }
                        out.push_str(&source);
                    }
                    _ => out.push_str(&word),
                }
                i = j;
                continue;
            }
            out.push(c);
            i += 1;
        }
        out
    }

    fn cur_depth(&self) -> u32 {
        self.depth_at.get(self.pos).copied().unwrap_or(0)
    }

    /// Skip to the next place an item of brace depth `d` can start: a token
    /// `starts` accepts, or the `}` closing the enclosing block. Always moves
    /// past `item_start` (the failed item's first token).
    fn resync(&mut self, d: u32, item_start: usize, starts: impl Fn(&SpannedToken) -> bool) {
        if self.pos == item_start && !self.is_at_end() {
            self.advance();
        }
        while !self.is_at_end() {
            let dep = self.cur_depth();
            if dep < d {
                return;
            }
            if dep == d && (self.check(&Token::RBrace) || starts(&self.tokens[self.pos])) {
                return;
            }
            self.advance();
        }
    }

    /// The items of a `{ … }` block, one per line (statements, test rules):
    /// in recovery mode a failed item is recorded and the next line parsed.
    fn parse_block_items<T>(&mut self, mut item: impl FnMut(&mut Self) -> Result<T, ParseError>) -> Result<Vec<T>, ParseError> {
        let d = self.cur_depth();
        let mut out = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.pos;
            match item(self) {
                Ok(x) => out.push(x),
                Err(e) if self.can_recover() => {
                    self.recovered.push((e, false));
                    self.resync(d, start, |t| t.newline_before);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// The constants of each `skeleton` of this file, read ahead: a flesh
    /// written above its skeleton uses them too.
    fn prescan_skeleton_consts(&mut self) {
        let saved = (self.pos, std::mem::take(&mut self.consts));
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < self.tokens.len() {
            match &self.tokens[i].token {
                Token::LBrace => depth += 1,
                Token::RBrace => depth -= 1,
                Token::Ident(w) if depth == 0 && w == "skeleton" => {
                    let at = if matches!(self.tokens.get(i + 1).map(|t| &t.token), Some(Token::AgentKw)) { i + 2 } else { i + 1 };
                    let name = match self.tokens.get(at).map(|t| &t.token) { Some(Token::Ident(n) | Token::TypeIdent(n)) => Some(n.clone()), _ => None };
                    // the first `{` after the name opens the body
                    let open = (at..self.tokens.len()).find(|j| matches!(self.tokens[*j].token, Token::LBrace | Token::Eof));
                    if let (Some(name), Some(open)) = (name, open) {
                        if matches!(self.tokens[open].token, Token::LBrace) {
                            self.pos = open + 1;
                            self.consts.clear();
                            self.prescan_consts();
                            let found = std::mem::take(&mut self.consts);
                            SKELETON_CONSTS.with(|m| { m.borrow_mut().insert(name, found); });
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
        self.pos = saved.0;
        self.consts = saved.1;
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        self.prescan_skeleton_consts();
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

        let mut protocols = Vec::new();
        while !self.is_at_end() {
            let start = self.pos;
            let r = if self.check(&Token::Protocol) {
                self.parse_protocol_def().map(|p| protocols.push(p))
            } else if self.at_word("flesh") {
                self.parse_flesh_def().map(|c| cells.push(c))
            } else {
                self.parse_cell_def().map(|c| cells.push(c))
            };
            match r {
                Ok(()) => {}
                Err(e) if self.can_recover() => {
                    self.recovered.push((e, false));
                    self.resync(0, start, |t| matches!(t.token, Token::Cell | Token::Protocol)
                        || matches!(&t.token, Token::Ident(w) if w == "skeleton" || w == "flesh"));
                    // the `}` of the cell that failed is not an item start
                    while self.check(&Token::RBrace) { self.advance(); }
                }
                Err(e) => return Err(e),
            }
        }
        let mut program = Program { nominal_issues: Vec::new(), imports, cells, protocols };
        // a flesh in the same file as its skeleton (one in another file is
        // attached when the import brings it in)
        crate::checker::flesh::attach(&mut program);
        Ok(program)
    }

    /// The next token is the contextual keyword `word` (an identifier
    /// everywhere else: `skeleton`, `flesh`, `reads`, `writes`, …).
    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Token::Ident(w) if w == word)
    }

    /// `flesh Name { on f(a, b) { … } … }`: handler bodies for `skeleton
    /// Name`, and nothing else.
    fn parse_flesh_def(&mut self) -> Result<Spanned<CellDef>, ParseError> {
        let start = self.peek_span();
        self.advance(); // flesh
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut sections = Vec::new();
        let section_depth = self.cur_depth();
        // the skeleton's constants are the flesh's
        let outer_consts = std::mem::replace(&mut self.consts,
            SKELETON_CONSTS.with(|m| m.borrow().get(&name).cloned().unwrap_or_default()));
        let result = self.parse_flesh_sections(&name, section_depth, &mut sections);
        self.consts = outer_consts;
        result?;
        let end = self.peek_span();
        self.expect(Token::RBrace)?;
        Ok(Spanned::new(
            CellDef { kind: CellKind::Flesh, name, type_params: vec![], sections, agent_model: None, agent_skill: None, skeleton: false, nominal: None },
            start.merge(end),
        ))
    }

    fn parse_flesh_sections(&mut self, name: &str, section_depth: u32, sections: &mut Vec<Spanned<Section>>) -> Result<(), ParseError> {
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let item_start = self.pos;
            let sec_start = self.peek_span();
            let parsed = if self.check(&Token::On) {
                self.parse_on_section_in(true).map(|on| Spanned::new(Section::OnSignal(on), sec_start.merge(self.prev_span())))
            } else {
                Err(ParseError::FixIt {
                    message: format!("a flesh holds handler bodies only (`on name(…) {{ … }}`) — memory, state and face are the structure: they belong in `skeleton {}`", name),
                    span: self.peek_span(),
                })
            };
            match parsed {
                Ok(sec) => sections.push(sec),
                Err(e) if self.can_recover() => {
                    self.recovered.push((e, false));
                    self.resync(section_depth, item_start, |t| matches!(t.token, Token::On));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// V1.6: protocol Foo {
    ///     roles: client = Client, server = Server
    ///     client -> server: request(query: String)
    ///     server -> client: reply(answer: String)
    /// }
    fn parse_protocol_def(&mut self) -> Result<Spanned<ProtocolDef>, ParseError> {
        let start = self.peek_span();
        self.expect(Token::Protocol)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;

        let mut roles = Vec::new();
        let mut steps = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Either: "roles: a = A, b = B"  or  "from -> to: msg(args)"
            if let Token::Ident(first) = self.peek().clone() {
                if first == "roles" {
                    self.advance();
                    self.expect(Token::Colon)?;
                    loop {
                        let (role, _) = self.expect_ident()?;
                        self.expect(Token::Eq)?;
                        let (cell, _) = self.expect_ident()?;
                        roles.push((role, cell));
                        if self.check(&Token::Comma) { self.advance(); }
                        else { break; }
                    }
                    continue;
                }
            }
            // Step: from -> to: msg(args)
            let (from, _) = self.expect_ident()?;
            self.expect(Token::Arrow)?;
            let (to, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let (msg, _) = self.expect_ident()?;
            self.expect(Token::LParen)?;
            let mut args = Vec::new();
            while !self.check(&Token::RParen) {
                let (an, _) = self.expect_ident()?;
                // optional :Type — consume and discard
                if self.check(&Token::Colon) {
                    self.advance();
                    let _ = self.expect_ident()?;
                }
                args.push(an);
                if self.check(&Token::Comma) { self.advance(); }
            }
            self.expect(Token::RParen)?;
            steps.push(ProtocolStep { from, to, message: msg, args });
        }
        self.expect(Token::RBrace)?;
        Ok(Spanned::new(
            ProtocolDef { name, roles, steps },
            start.merge(self.prev_span()),
        ))
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn peek_at(&self, n: usize) -> &Token {
        let idx = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[idx].token
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
            Token::Cost => { let span = tok.span; self.advance(); Ok(("cost".to_string(), span)) }
            Token::Protocol => { let span = tok.span; self.advance(); Ok(("protocol".to_string(), span)) }
            // verify-property words are ordinary field names (`r.after`, `x.requires`)
            Token::After => { let span = tok.span; self.advance(); Ok(("after".to_string(), span)) }
            Token::Every => { let span = tok.span; self.advance(); Ok(("every".to_string(), span)) }
            Token::Ensure => { let span = tok.span; self.advance(); Ok(("ensure".to_string(), span)) }
            Token::Requires => { let span = tok.span; self.advance(); Ok(("requires".to_string(), span)) }
            Token::Connect => { let span = tok.span; self.advance(); Ok(("connect".to_string(), span)) }
            Token::Variants => { let span = tok.span; self.advance(); Ok(("variants".to_string(), span)) }
            Token::Implies => { let span = tok.span; self.advance(); Ok(("implies".to_string(), span)) }
            Token::Contradicts => { let span = tok.span; self.advance(); Ok(("contradicts".to_string(), span)) }
            Token::Match => { let span = tok.span; self.advance(); Ok(("match".to_string(), span)) }
            t if reserved_word(t).is_some() => Err(reserved_error(t, tok.span).unwrap()),
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

    /// The next token starts a line: `[` or `(` there opens a new
    /// expression, not an index or a call of the previous line's value
    /// (`let xs = [1, 2]` then `[i]` read as `[1, 2][i]`).
    fn at_line_start(&self) -> bool {
        self.tokens.get(self.pos).map_or(false, |t| t.newline_before)
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
        // `skeleton Name { … }` / `skeleton agent Name { … }`: a cell whose
        // handler bodies live in `flesh Name { … }`
        let skeleton = self.at_word("skeleton");
        if skeleton {
            self.advance();
            if !matches!(self.peek(), Token::AgentKw | Token::Ident(_) | Token::TypeIdent(_)) {
                return Err(ParseError::FixIt {
                    message: "a skeleton is `skeleton Name { … }` or `skeleton agent Name { … }` — the other kinds of cell (type, test, property…) have no flesh".to_string(),
                    span: self.peek_span(),
                });
            }
        } else {
            self.expect(Token::Cell)?;
        }

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

        // `cell type Lots = Int`: a nominal type over a base — no body
        if kind == CellKind::Type && self.check(&Token::Eq) {
            self.advance();
            let base_span = self.peek_span();
            let base = match self.peek().clone() {
                Token::Ident(b) | Token::TypeIdent(b) => { self.advance(); b }
                other => return Err(ParseError::Expected { expected: format!("the type `{name}` stands for (`cell type {name} = Int`)"), found: other, span: base_span }),
            };
            if !matches!(base.as_str(), "Int" | "Float" | "String") || !type_params.is_empty() {
                return Err(ParseError::FixIt {
                    message: format!("`cell type {name} = {base}` — a nominal type stands for Int, Float or String (`cell type {name} = Int`); a record or a sum type is `cell type {name} {{ variants {{ … }} }}`"),
                    span: base_span,
                });
            }
            return Ok(Spanned::new(
                CellDef { kind, name, type_params, sections: vec![], agent_model: None, agent_skill: None, skeleton, nominal: Some(base) },
                start.merge(self.prev_span()),
            ));
        }

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
        let section_depth = self.cur_depth();
        // an interior cell reads its parent's constants and its own
        let outer_consts = self.consts.clone();
        self.prescan_consts();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let item_start = self.pos;
            let is_const = matches!(self.peek(), Token::Ident(w) if w == "const");
            let parsed = if is_const { self.parse_const().map(|_| None) }
                else if skeleton && self.check(&Token::On) {
                    Err(ParseError::FixIt {
                        message: format!("a skeleton has no handler bodies — declare the signal in `face {{ }}` with its frame, and put the body in `flesh {} {{ on … {{ … }} }}`", name),
                        span: self.peek_span(),
                    })
                }
                else { self.parse_section().map(Some) };
            match parsed {
                Ok(Some(sec)) => sections.push(sec),
                Ok(None) => {}
                Err(e) if self.can_recover() => {
                    self.recovered.push((e, false));
                    self.resync(section_depth, item_start, |t| matches!(&t.token, Token::Ident(w) if w == "const") || matches!(t.token,
                        Token::On | Token::Face | Token::Memory | Token::State | Token::Every | Token::After
                        | Token::Cost | Token::Rules | Token::Interior | Token::Scale | Token::Runtime | Token::Variants));
                }
                Err(e) => { self.consts = outer_consts; return Err(e); }
            }
        }
        self.consts = outer_consts;

        let end = self.peek_span();
        self.expect(Token::RBrace)?;

        // 3.0: a tool bound to a server (`[mcp: …]` / `[openapi: …]`) has
        // no handler in the source; it gets the one that makes the call, so
        // that a call, a `mock` and every analysis find it like any other
        let mut bound: Vec<Spanned<Section>> = Vec::new();
        for section in &sections {
            let Section::Face(face) = &section.node else { continue };
            for decl in &face.declarations {
                let FaceDecl::Tool(tool) = &decl.node else { continue };
                if tool.binding.is_none() { continue; }
                if sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == tool.name)) { continue; }
                let mut args = vec![Spanned::new(Expr::Literal(Literal::String(tool.name.clone())), decl.span)];
                args.extend(tool.params.iter().map(|p| Spanned::new(Expr::Ident(p.name.clone()), decl.span)));
                let call = Spanned::new(Expr::FnCall { name: "_external_tool".to_string(), args }, decl.span);
                bound.push(Spanned::new(Section::OnSignal(OnSection {
                    signal_name: tool.name.clone(),
                    params: tool.params.clone(),
                    body: vec![Spanned::new(Statement::Return { value: call }, decl.span)],
                    properties: vec![],
                }), decl.span));
            }
        }
        sections.extend(bound);

        // a skeleton's frames are closed (no clause: the signal may touch
        // nothing), and every signal answers: the ones no flesh fills are
        // holes — handlers that fail by name until a `flesh` replaces them
        if skeleton {
            let mut holes: Vec<Spanned<Section>> = Vec::new();
            for section in sections.iter_mut() {
                let Section::Face(face) = &mut section.node else { continue };
                for decl in face.declarations.iter_mut() {
                    let (sig_name, params) = match &mut decl.node {
                        FaceDecl::Signal(sig) => { sig.frame.get_or_insert_with(FrameDecl::default); (sig.name.clone(), sig.params.clone()) }
                        FaceDecl::Tool(tool) if tool.binding.is_none() => { tool.frame.get_or_insert_with(FrameDecl::default); (tool.name.clone(), tool.params.clone()) }
                        _ => continue,
                    };
                    if holes.iter().any(|h| matches!(&h.node, Section::OnSignal(on) if on.signal_name == sig_name)) { continue; }
                    holes.push(Spanned::new(Section::OnSignal(hole_handler(&name, &sig_name, params, decl.span)), decl.span));
                }
            }
            sections.extend(holes);
        }

        Ok(Spanned::new(
            CellDef {
                kind,
                name,
                type_params,
                sections,
                agent_model,
                agent_skill,
                skeleton,
                nominal: None,
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
            Token::Cost => {
                let cs = self.parse_cost_section()?;
                Ok(Spanned::new(Section::Cost(cs), start.merge(self.prev_span())))
            }
            Token::Variants => {
                let v = self.parse_variants_section()?;
                Ok(Spanned::new(Section::Variants(v), start.merge(self.prev_span())))
            }
            Token::Assert | Token::Property => {
                // In test cells, bare `assert expr` / `property "name" forall ...`
                // is syntactic sugar for a Rules section with the corresponding rules.
                let mut rules = Vec::new();
                while self.check(&Token::Assert) || self.check(&Token::Property) {
                    rules.push(self.parse_rule()?);
                }
                Ok(Spanned::new(
                    Section::Rules(RulesSection { rules }),
                    start.merge(self.prev_span()),
                ))
            }
            // `const LIMIT = 500` / a cell-level `let`: there are none — say so
            Token::Let => Err(ParseError::FixIt {
                message: "a cell has no cell-level `let` — a constant is `const LIMIT = 500` (a literal, usable in invariants, guards and `require`); a value that changes lives in a `memory` slot".to_string(),
                span: self.peek_span(),
            }),
            Token::Ident(ref w) if w == "val" || w == "var" => Err(ParseError::FixIt {
                message: format!("`{}`: a cell-level constant is `const LIMIT = 500` (a literal, usable in invariants, guards and `require`); a value that changes lives in a `memory` slot", w),
                span: self.peek_span(),
            }),
            _ => Err(ParseError::Expected {
                expected: "a section: face, memory, state, on, every, after, cost, or (in a test cell) rules".to_string(),
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

        // 3.0: the contract of the signal — `requires amount > 0`,
        // `ensures result >= 0` (any number of each, in any order)
        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        // the frame clauses (contextual keywords, any order)
        let mut frame: Option<FrameDecl> = None;
        loop {
            if self.check(&Token::Requires) {
                self.advance();
                requires.push(self.parse_expr()?);
            } else if matches!(self.peek(), Token::Ident(s) if s == "ensures") {
                self.advance();
                ensures.push(self.parse_expr()?);
            } else if self.parse_frame_clause(&mut frame)? {
            } else {
                break;
            }
        }

        Ok(SignalDecl {
            name,
            params,
            return_type,
            requires,
            ensures,
            frame,
        })
    }

    /// One clause of a frame, if one starts here: `reads a, b` / `writes a`
    /// / `emits s` / `calls f, Cell.g` / `moves x -> y, …`.
    fn parse_frame_clause(&mut self, frame: &mut Option<FrameDecl>) -> Result<bool, ParseError> {
        if self.at_word("reads") || self.at_word("writes") || self.at_word("emits") {
            let which = match self.peek() { Token::Ident(w) => w.clone(), _ => unreachable!() };
            self.advance();
            let f = frame.get_or_insert_with(FrameDecl::default);
            loop {
                // `reads Cell.machine`: another cell's machine
                let (mut n, _) = self.expect_any_name()?;
                if self.check(&Token::Dot) {
                    self.advance();
                    n = format!("{}.{}", n, self.expect_any_name()?.0);
                }
                match which.as_str() { "reads" => f.reads.push(n), "writes" => f.writes.push(n), _ => f.emits.push(n) }
                if self.check(&Token::Comma) { self.advance(); } else { break; }
            }
            Ok(true)
        } else if self.at_word("calls") {
            self.advance();
            let f = frame.get_or_insert_with(FrameDecl::default);
            loop {
                let (mut n, _) = self.expect_any_name()?;
                if self.check(&Token::Dot) {
                    self.advance();
                    let (h, _) = self.expect_any_name()?;
                    n = format!("{}.{}", n, h);
                }
                f.calls.push(n);
                if self.check(&Token::Comma) { self.advance(); } else { break; }
            }
            Ok(true)
        } else if self.at_word("moves") {
            self.advance();
            let f = frame.get_or_insert_with(FrameDecl::default);
            // `moves a -> b` (the cell's machine), `moves m: a -> b`,
            // `moves Cell.m: a -> b`
            let mut machine = String::new();
            if matches!(self.peek_at(1), Token::Colon) {
                machine = self.expect_any_name()?.0;
                self.advance();
            } else if matches!(self.peek_at(1), Token::Dot) && matches!(self.peek_at(3), Token::Colon) {
                let (c, _) = self.expect_any_name()?;
                self.advance();
                let (m, _) = self.expect_any_name()?;
                self.advance();
                machine = format!("{}.{}", c, m);
            }
            loop {
                let from = if self.check(&Token::Star) { self.advance(); "*".to_string() } else { self.expect_any_name()?.0 };
                if !self.check(&Token::Arrow) {
                    return Err(ParseError::FixIt {
                        message: format!("`moves` lists edges of the state machine: `moves {} -> next_state`", from),
                        span: self.peek_span(),
                    });
                }
                self.advance();
                // `moves * -> *`: any edge (a target computed at run time)
                let to = if self.check(&Token::Star) { self.advance(); "*".to_string() } else { self.expect_any_name()?.0 };
                // `when Consult.done || Consult.closed [via consult_of]`: a rule
                // between machines stated on the edge
                if self.at_word("when") {
                    self.advance();
                    let mut states = Vec::new();
                    loop {
                        let (c, _) = self.expect_any_name()?;
                        if !self.check(&Token::Dot) {
                            return Err(ParseError::FixIt { message: format!("`when` names a state of another cell's machine: `when {}.state`", c), span: self.peek_span() });
                        }
                        self.advance();
                        let (st, _) = self.expect_any_name()?;
                        states.push(format!("{}.{}", c, st));
                        if self.check(&Token::OrOr) { self.advance(); } else { break; }
                    }
                    let via = if self.at_word("via") { self.advance(); Some(self.expect_any_name()?.0) } else { None };
                    f.conditions.push(EdgeCondition { machine: machine.clone(), from: from.clone(), to: to.clone(), states, via });
                }
                f.moves.push((machine.clone(), from, to));
                if self.check(&Token::Comma) { self.advance(); } else { break; }
            }
            Ok(true)
        } else {
            Ok(false)
        }
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

        // V1.6: optional capability declaration:
        //   tool fetch(url) -> String [capability: "net:wikipedia.org"]
        //   tool fetch(url) -> String [capabilities: "net:a.com", "net:b.com"]
        let mut capabilities = Vec::new();
        // 3.0: `[mcp: "server"]` / `[openapi: "server/operationId"]` — the
        // tool runs on a server of soma.toml's `[tools.*]`, not in a handler
        let mut binding: Option<ToolBinding> = None;
        if self.check(&Token::LBracket) {
            self.advance();
            while !self.check(&Token::RBracket) && !self.is_at_end() {
                if let Token::Ident(ref key) = self.peek().clone() {
                    let key = key.clone();
                    self.advance();
                    self.expect(Token::Colon)?;
                    if key == "capability" || key == "capabilities" {
                        loop {
                            if let Token::StringLit(ref s) = self.peek().clone() {
                                capabilities.push(s.clone());
                                self.advance();
                            } else { break; }
                            if self.check(&Token::Comma) {
                                self.advance();
                                // peek next; if it's a string keep collecting
                                if !matches!(self.peek(), Token::StringLit(_)) { break; }
                            } else { break; }
                        }
                    } else if key == "mcp" || key == "openapi" {
                        let at = self.peek_span();
                        let Token::StringLit(target) = self.peek().clone() else {
                            return Err(ParseError::FixIt {
                                message: format!("`{key}:` takes the server's name in soma.toml, as a string: `[{key}: \"docs\"]` (the tool's own name on the server) or `[{key}: \"docs/remote_name\"]`"),
                                span: at,
                            });
                        };
                        self.advance();
                        let (server, remote) = match target.split_once('/') {
                            Some((s, r)) => (s.to_string(), r.to_string()),
                            None => (target.clone(), name.clone()),
                        };
                        if binding.is_some() || server.is_empty() || remote.is_empty() {
                            return Err(ParseError::FixIt {
                                message: format!("tool '{name}' is bound once, to one server: `[mcp: \"server\"]` or `[openapi: \"server/operationId\"]`"),
                                span: at,
                            });
                        }
                        binding = Some(ToolBinding { kind: key.clone(), server, remote });
                    } else {
                        // skip unknown attribute value
                        self.advance();
                    }
                    if self.check(&Token::Comma) { self.advance(); }
                } else {
                    self.advance();
                }
            }
            self.expect(Token::RBracket)?;
        }

        // Optional description string
        let description = if let Token::StringLit(ref s) = self.peek().clone() {
            let d = s.clone();
            self.advance();
            Some(d)
        } else {
            None
        };

        let mut frame: Option<FrameDecl> = None;
        while self.parse_frame_clause(&mut frame)? {}

        Ok(ToolDecl {
            name,
            description,
            params,
            return_type,
            capabilities,
            binding,
            frame,
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
        let (name, name_span) = self.expect_ident()?;
        self.refuse_const(&name, name_span, "a parameter")?;
        if !self.check(&Token::Colon) {
            return Err(ParseError::FixIt {
                message: format!("parameter '{}' needs a type — `{}: Int` / `String` / `Float` / `Bool` / `Map` / `List`, or `{}: Any` for a value of any kind", name, name, name),
                span: self.tokens[self.pos].span,
            });
        }
        self.advance();
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

        // V1.1: the property bracket is now **optional**. Bare slots like
        // `orders: Map<String, String>` are legal; the compiler infers the
        // right backend (and adds default-on causal-clock metadata).
        let mut properties = Vec::new();
        let end_span;
        if self.check(&Token::LBracket) {
            self.advance();
            while !self.check(&Token::RBracket) && !self.is_at_end() {
                properties.push(self.parse_memory_property()?);
                if self.check(&Token::Comma) {
                    self.advance();
                }
            }
            end_span = self.peek_span();
            self.expect(Token::RBracket)?;
        } else {
            end_span = self.prev_span();
        }

        Ok(Spanned::new(
            MemorySlot {
                name,
                ty,
                properties,
            },
            start.merge(end_span),
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

        let rules = self.parse_block_items(|p| p.parse_rule())?;

        self.expect(Token::RBrace)?;
        Ok(RulesSection { rules })
    }

    fn parse_rule(&mut self) -> Result<Spanned<Rule>, ParseError> {
        let start = self.peek_span();
        // `assert_fails expr` — negative assertion (lexes as a plain
        // identifier, so handle it before the keyword match).
        if matches!(self.peek(), Token::Ident(s) if s == "assert_fails") {
            self.advance();
            let expr = self.parse_expr()?;
            if matches!(self.peek(), Token::Ident(s) if s == "matching") {
                self.advance();
                let Token::StringLit(text) = self.peek().clone() else {
                    return Err(ParseError::Expected {
                        expected: "a string after `matching` (assert_fails f(x) matching \"invalid transition\")".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    });
                };
                self.advance();
                return Ok(Spanned::new(Rule::AssertFailsMatching(expr, text), start.merge(self.prev_span())));
            }
            return Ok(Spanned::new(Rule::AssertFails(expr), start.merge(self.prev_span())));
        }
        // `mock think <expr>` / `mock think error <expr>`
        if matches!(self.peek(), Token::Ident(s) if s == "mock") {
            self.advance();
            if matches!(self.peek(), Token::Ident(s) if s == "approve") {
                self.advance();
                let reply = self.parse_expr()?;
                return Ok(Spanned::new(Rule::MockApprove { reply }, start.merge(self.prev_span())));
            }
            if !matches!(self.peek(), Token::Ident(s) if s == "think") {
                // `mock <handler> <value>` / `mock <handler> error "msg"`:
                // stub any handler of the program (a tool, an http wrapper)
                if let Token::Ident(name) | Token::TypeIdent(name) = self.peek().clone() {
                    self.advance();
                    // `mock Notifier.send …` stubs THAT cell's handler only
                    // (the cell name was ignored: `mock Email.send` caught
                    // `Sms.send`, `mock Nope.charge` caught `Pay.charge`)
                    let name = if self.check(&Token::Dot) {
                        self.advance();
                        let (h, _) = self.expect_ident()?;
                        format!("{}.{}", name, h)
                    } else { name };
                    let is_error = matches!(self.peek(), Token::Ident(s) if s == "error");
                    if is_error { self.advance(); }
                    let reply = self.parse_expr()?;
                    return Ok(Spanned::new(Rule::MockHandler { name, reply, is_error }, start.merge(self.prev_span())));
                }
                return Err(ParseError::Expected {
                    expected: "think, approve, now, a handler name or Cell.handler (mock think \"reply\" | mock think error \"msg\" | mock approve false | mock now 1700000000 | mock price_check 42 | mock Prices.fetch error \"not_found: down\")".to_string(),
                    found: self.peek().clone(),
                    span: self.peek_span(),
                });
            }
            self.advance();
            let is_error = matches!(self.peek(), Token::Ident(s) if s == "error");
            if is_error {
                self.advance();
            }
            let reply = self.parse_expr()?;
            return Ok(Spanned::new(Rule::MockThink { reply, is_error }, start.merge(self.prev_span())));
        }
        // `let name = expr` — a fixture shared by the following rules
        if matches!(self.peek(), Token::Let) {
            self.advance();
            let (name, _) = self.expect_ident()?;
            self.expect(Token::Eq)?;
            let value = self.parse_expr()?;
            return Ok(Spanned::new(Rule::Let { name, value }, start.merge(self.prev_span())));
        }
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
            Token::Property => {
                // V1.6: property "name" forall x: Int in lo..hi [count N] ensures expr
                self.advance();
                let name = if let Token::StringLit(s) = self.peek().clone() {
                    self.advance(); s
                } else { return Err(ParseError::Expected {
                    expected: "property name string".to_string(),
                    found: self.peek().clone(),
                    span: self.peek_span(),
                }); };
                // expect "forall"
                if !matches!(self.peek(), Token::Ident(s) if s == "forall") {
                    return Err(ParseError::Expected {
                        expected: "forall".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    });
                }
                self.advance();
                let (var, _) = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let (ty, _) = self.expect_ident()?;
                if !matches!(self.peek(), Token::In) {
                    return Err(ParseError::Expected {
                        expected: "in".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    });
                }
                self.advance();
                let lo = match self.peek().clone() {
                    Token::IntLit(n) => { self.advance(); n }
                    Token::Minus => {
                        self.advance();
                        if let Token::IntLit(n) = self.peek().clone() { self.advance(); -n }
                        else { return Err(ParseError::Expected {
                            expected: "integer".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        }); }
                    }
                    _ => return Err(ParseError::Expected {
                        expected: "integer".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    }),
                };
                self.expect(Token::DotDot)?;
                let hi = match self.peek().clone() {
                    Token::IntLit(n) => { self.advance(); n }
                    Token::Minus => {
                        self.advance();
                        if let Token::IntLit(n) = self.peek().clone() { self.advance(); -n }
                        else { return Err(ParseError::Expected {
                            expected: "integer".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        }); }
                    }
                    _ => return Err(ParseError::Expected {
                        expected: "integer".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    }),
                };
                // optional `count N`
                let count = if matches!(self.peek(), Token::Ident(s) if s == "count") {
                    self.advance();
                    if let Token::IntLit(n) = self.peek().clone() { self.advance(); n as u32 }
                    else { 50 }
                } else { 50 };
                if !matches!(self.peek(), Token::Ident(s) if s == "ensures") {
                    return Err(ParseError::Expected {
                        expected: "ensures".to_string(),
                        found: self.peek().clone(),
                        span: self.peek_span(),
                    });
                }
                self.advance();
                let body = self.parse_expr()?;
                Ok(Spanned::new(
                    Rule::Property { name, var, ty, lo, hi, count, body },
                    start.merge(self.prev_span()),
                ))
            }
            _ => Err(ParseError::Expected {
                expected: "assert, assert_fails [matching \"text\"], let, mock think | mock approve | mock now | mock <handler> | mock Cell.handler, or property (a test cell's rules hold assertions — put logic in a handler)".to_string(),
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

    fn parse_variants_section(&mut self) -> Result<VariantsSection, ParseError> {
        self.expect(Token::Variants)?;
        self.expect(Token::LBrace)?;
        let mut variants = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            let (name, _) = match self.peek() {
                Token::TypeIdent(t) => {
                    let t = t.clone();
                    self.advance();
                    (t, start)
                }
                _ => return Err(ParseError::Expected {
                    expected: "PascalCase variant name".to_string(),
                    found: self.peek().clone(),
                    span: self.peek_span(),
                }),
            };
            // Optional fields.
            let fields = if self.check(&Token::LBrace) {
                // Struct variant: { field: Type, field: Type, ... }
                self.advance();
                let mut fields = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    let (fname, _) = self.expect_ident()?;
                    self.expect(Token::Colon)?;
                    let ty = self.parse_type_expr()?;
                    fields.push((fname, ty));
                    if self.check(&Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(Token::RBrace)?;
                VariantFields::Struct(fields)
            } else if self.check(&Token::LParen) {
                // Tuple variant: (Type, Type, ...)
                self.advance();
                let mut tys = Vec::new();
                while !self.check(&Token::RParen) && !self.is_at_end() {
                    tys.push(self.parse_type_expr()?);
                    if self.check(&Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(Token::RParen)?;
                VariantFields::Tuple(tys)
            } else {
                VariantFields::Unit
            };
            variants.push(Spanned::new(
                VariantDecl { name, fields },
                start.merge(self.prev_span()),
            ));
            // Optional separator: newline or `;`.  We accept (and ignore)
            // a trailing comma or semicolon — but the language is
            // newline-separated, so usually nothing.
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(VariantsSection { variants })
    }

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

        // Optional type annotation: `state order: OrderState { ... }`.
        // When present, the refinement checker verifies all state names
        // are variants of the named sum type.
        let state_type = if self.check(&Token::Colon) {
            self.advance();
            if let Token::TypeIdent(ref t) = self.peek().clone() {
                let t = t.clone();
                self.advance();
                Some(t)
            } else {
                return Err(ParseError::Expected {
                    expected: "PascalCase sum-type name after ':' in state declaration".to_string(),
                    found: self.peek().clone(),
                    span: self.peek_span(),
                });
            }
        } else {
            None
        };

        // Optional bracketed annotations: `state foo [max_instances(1000)] { ... }`.
        // Used by the V1.4 budget checker (compiler/src/checker/budget.rs).
        let mut sm_properties: Vec<Spanned<MemoryProperty>> = Vec::new();
        if self.check(&Token::LBracket) {
            self.advance();
            if !self.check(&Token::RBracket) {
                sm_properties.push(self.parse_memory_property()?);
                while self.check(&Token::Comma) {
                    self.advance();
                    if self.check(&Token::RBracket) { break; }
                    sm_properties.push(self.parse_memory_property()?);
                }
            }
            self.expect(Token::RBracket)?;
        }

        self.expect(Token::LBrace)?;

        let mut initial = String::new();
        let mut transitions = Vec::new();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Check for `initial: state_name` — but `initial -> x` is a
            // transition whose SOURCE state is named "initial" (the word
            // is accepted as a state name everywhere else).
            if self.check(&Token::Initial) && !matches!(self.peek_at(1), Token::Arrow) {
                self.advance();
                // Fix-it: `initial = draft` is the assignment spelling.
                if self.check(&Token::Eq) {
                    let eq_span = self.peek_span();
                    self.advance();
                    let state_name = match self.peek() {
                        Token::Ident(s) | Token::TypeIdent(s) => s.clone(),
                        _ => "state_name".to_string(),
                    };
                    return Err(ParseError::FixIt {
                        message: format!(
                            "state machines declare the start state as 'initial: {}' (colon, not equals)",
                            state_name
                        ),
                        span: eq_span,
                    });
                }
                self.expect(Token::Colon)?;
                let (state_name, sspan) = self.expect_any_name()?;
                // two `initial:` lines: the last one silently won
                if !initial.is_empty() && initial != state_name {
                    return Err(ParseError::FixIt {
                        message: format!("a state machine has ONE initial state — it already starts in '{}'", initial),
                        span: sspan,
                    });
                }
                initial = state_name;
                continue;
            }

            // Fix-it: `final: paid` / `terminal: paid` / `states: [a, b]` —
            // states are never declared; a state with no outgoing transition
            // is final.
            if let Token::Ident(word) = self.peek().clone() {
                if matches!(word.as_str(), "final" | "terminal" | "end" | "states" | "accept")
                    && matches!(self.peek_at(1), Token::Colon | Token::Eq)
                {
                    return Err(ParseError::FixIt {
                        message: format!(
                            "state machines do not declare '{}' — states are named by transitions ('a -> b'), and a state with no outgoing transition is final; delete this line",
                            word
                        ),
                        span: self.peek_span(),
                    });
                }
            }

            // Parse transition: from -> to { guard { ... } effect { ... } }
            // or: * -> to { ... }
            let start = self.peek_span();
            let from = if self.check(&Token::Star) {
                self.advance();
                "*".to_string()
            } else {
                let (name, _) = self.expect_any_name()?;
                name
            };

            self.expect(Token::Arrow)?;

            let (to, _) = self.expect_any_name()?;

            // Fix-it: `a -> b when cond { }` — guards live inside the
            // transition block, not after the target state. But `when` is
            // a legal state name: `when -> c` (next token is '->') must
            // parse as the next transition, not trip the fix-it.
            if (matches!(self.peek(), Token::Ident(s) if s == "when") || self.check(&Token::If))
                && !matches!(self.peek_at(1), Token::Arrow) {
                return Err(ParseError::FixIt {
                    message: format!(
                        "guards are declared inside the transition block: {} -> {} {{ guard {{ cond }} }} — the condition reads the calling handler's locals",
                        from, to
                    ),
                    span: self.peek_span(),
                });
            }

            // `* -> failed except [paid, denied]` — states the wildcard does
            // not leave (they stay final)
            let mut except: Vec<String> = Vec::new();
            if matches!(self.peek(), Token::Ident(s) if s == "except") {
                if from != "*" {
                    return Err(ParseError::FixIt {
                        message: format!("`except` only applies to a wildcard transition: `* -> {} except [a, b]`", to),
                        span: self.peek_span(),
                    });
                }
                self.advance();
                let bracketed = self.check(&Token::LBracket);
                if bracketed {
                    self.advance();
                }
                loop {
                    let (name, _) = self.expect_any_name()?;
                    except.push(name);
                    if self.check(&Token::Comma) {
                        self.advance();
                        continue;
                    }
                    break;
                }
                if bracketed {
                    self.expect(Token::RBracket)?;
                }
            }

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
                        return Err(ParseError::Expected {
                            expected: "'guard', 'effect', or '}'".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        });
                    }
                }
                self.expect(Token::RBrace)?;
            }

            transitions.push(Spanned::new(
                Transition { from: from.clone(), to: to.clone(), except, guard, effect },
                start.merge(self.prev_span()),
            ));

            // Support chained transitions: a -> b -> c -> d
            // Desugars to: a -> b, b -> c, c -> d
            let mut prev = to;
            while self.check(&Token::Arrow) {
                self.advance();
                let (next, _) = self.expect_any_name()?;
                transitions.push(Spanned::new(
                    Transition { from: prev.clone(), to: next.clone(), except: vec![], guard: None, effect: vec![] },
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

        // Reject state machines with no determinable initial state
        if initial.is_empty() && !transitions.is_empty() {
            return Err(ParseError::Expected {
                expected: "an 'initial:' declaration or a non-wildcard first transition".to_string(),
                found: Token::Eof, // placeholder
                span: self.prev_span(),
            });
        }

        Ok(StateMachineSection { name, initial, transitions, properties: sm_properties, state_type })
    }

    // ── Every (scheduler) ─────────────────────────────────────────────

    /// Optional `[task]` after a tick's period: `every 5s [task] { … }`.
    fn parse_tick_task(&mut self) -> Result<bool, ParseError> {
        if !self.check(&Token::LBracket) { return Ok(false); }
        self.advance();
        let tok = self.tokens[self.pos].clone();
        let (name, _) = self.expect_ident()?;
        if name != "task" {
            return Err(ParseError::Expected { expected: "`[task]` (the only tick annotation)".to_string(), found: tok.token, span: tok.span });
        }
        self.expect(Token::RBracket)?;
        Ok(true)
    }

    fn parse_every_section(&mut self) -> Result<EverySection, ParseError> {
        self.expect(Token::Every)?;
        // Parse interval: 30s, 5min, 1h, 500ms, or bare number (seconds)
        let tok = &self.tokens[self.pos].clone();
        let interval_ms = match &tok.token {
            Token::DurationLit(_, _, ms) => {
                self.advance();
                (*ms).max(0) as u64
            }
            Token::IntLit(n) => {
                if *n < 0 {
                    return Err(ParseError::Expected {
                        expected: "positive interval (e.g., 30s, 5min, 1h)".to_string(),
                        found: tok.token.clone(),
                        span: tok.span,
                    });
                }
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

        let task = self.parse_tick_task()?;
        self.expect(Token::LBrace)?;
        let body = self.parse_block_items(|p| p.parse_statement())?;
        self.expect(Token::RBrace)?;

        Ok(EverySection { interval_ms, body, task })
    }

    // ── After (one-shot delayed) ──────────────────────────────────

    fn parse_after_section(&mut self) -> Result<EverySection, ParseError> {
        self.expect(Token::After)?;
        let tok = &self.tokens[self.pos].clone();
        let interval_ms = match &tok.token {
            Token::DurationLit(_, _, ms) => {
                self.advance();
                (*ms).max(0) as u64
            }
            Token::IntLit(n) => {
                if *n < 0 {
                    return Err(ParseError::Expected {
                        expected: "positive delay (e.g., 5s, 1min, 500ms)".to_string(),
                        found: tok.token.clone(),
                        span: tok.span,
                    });
                }
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

        let task = self.parse_tick_task()?;
        self.expect(Token::LBrace)?;
        let body = self.parse_block_items(|p| p.parse_statement())?;
        self.expect(Token::RBrace)?;

        Ok(EverySection { interval_ms, body, task })
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
                other => {
                    return Err(ParseError::Expected {
                        expected: "replicas, shard, consistency, tolerance, cpu, memory, or disk".to_string(),
                        found: Token::Ident(other.to_string()),
                        span: self.prev_span(),
                    });
                }
            }
        }
        self.expect(Token::RBrace)?;

        Ok(ScaleSection { replicas, shard, consistency, tolerance, cpu, memory: memory_res, disk })
    }

    // ── Cost (V1.6) ──────────────────────────────────────────────────

    fn parse_cost_section(&mut self) -> Result<CostSection, ParseError> {
        self.expect(Token::Cost)?;
        self.expect(Token::LBrace)?;

        let mut cs = CostSection::default();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let (key, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            match key.as_str() {
                "tokens" => {
                    if let Token::IntLit(n) = self.peek().clone() {
                        cs.tokens = Some(n);
                        self.advance();
                    } else {
                        return Err(ParseError::Expected {
                            expected: "integer".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        });
                    }
                }
                "latency" => {
                    // Accept "30s", "5000ms" — lexed as DurationLit(value, unit).
                    match self.peek().clone() {
                        Token::DurationLit(_, _, ms) => {
                            self.advance();
                            cs.latency_ms = Some(ms);
                        }
                        Token::IntLit(n) => {
                            self.advance();
                            cs.latency_ms = Some(n * 1000);  // bare int → seconds
                        }
                        _ => {
                            return Err(ParseError::Expected {
                                expected: "duration (e.g. 30s, 500ms)".to_string(),
                                found: self.peek().clone(),
                                span: self.peek_span(),
                            });
                        }
                    }
                }
                "usd" => {
                    // 0.10 → 100 milli-USD. Accept Float or Int.
                    match self.peek().clone() {
                        Token::FloatLit(f) => {
                            cs.usd_milli = Some((f * 1000.0).round() as i64);
                            self.advance();
                        }
                        Token::IntLit(n) => {
                            cs.usd_milli = Some(n * 1000);
                            self.advance();
                        }
                        _ => {
                            return Err(ParseError::Expected {
                                expected: "number".to_string(),
                                found: self.peek().clone(),
                                span: self.peek_span(),
                            });
                        }
                    }
                }
                other => {
                    return Err(ParseError::Expected {
                        expected: "tokens, latency, or usd".to_string(),
                        found: Token::Ident(other.to_string()),
                        span: self.prev_span(),
                    });
                }
            }
        }
        self.expect(Token::RBrace)?;
        Ok(cs)
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
        self.parse_on_section_in(false)
    }

    /// A flesh parameter: `id` (the skeleton's signal gives the type) or
    /// `id: String`.
    fn parse_flesh_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        while !self.check(&Token::RParen) {
            let (name, name_span) = self.expect_ident()?;
            self.refuse_const(&name, name_span, "a parameter")?;
            let ty = if self.check(&Token::Colon) {
                self.advance();
                self.parse_type_expr()?
            } else {
                Spanned::new(TypeExpr::Simple(FLESH_PARAM_TYPE.to_string()), name_span)
            };
            params.push(Param { name, ty });
            if self.check(&Token::Comma) { self.advance(); } else { break; }
        }
        Ok(params)
    }

    fn parse_on_section_in(&mut self, flesh: bool) -> Result<OnSection, ParseError> {
        self.expect(Token::On)?;
        let (signal_name, _) = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let params = if flesh { self.parse_flesh_params()? } else { self.parse_param_list()? };
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

        // Fix-it: `on add(a: Int) -> Int { ... }` — return types belong
        // on the signal declaration in face { }, not on the handler.
        if self.check(&Token::Arrow) {
            let arrow_span = self.peek_span();
            self.advance();
            // the WHOLE type (`-> Map<String, List<Int>>` said `-> Map`)
            let ret_ty = match self.peek() {
                Token::TypeIdent(_) | Token::Ident(_) => match self.parse_type_expr() {
                    Ok(t) => type_expr_source(&t.node),
                    Err(_) => "T".to_string(),
                },
                _ => "T".to_string(),
            };
            let params_str = params.iter()
                .map(|p| format!("{}: {}", p.name, type_expr_source(&p.ty.node)))
                .collect::<Vec<_>>()
                .join(", ");
            let err = ParseError::FixIt {
                message: format!(
                    "handlers do not declare return types — put '-> {}' on the signal declaration inside face {{ }}, then write: on {}({}) {{ ... }}",
                    ret_ty, signal_name, params_str
                ),
                span: arrow_span,
            };
            // the handler is read as if the type were not there
            if self.can_recover() && self.check(&Token::LBrace) {
                self.recovered.push((err, true));
            } else {
                return Err(err);
            }
        }

        self.expect(Token::LBrace)?;

        let body = self.parse_block_items(|p| p.parse_statement())?;

        self.expect(Token::RBrace)?;
        Ok(OnSection {
            signal_name,
            params,
            body,
            properties,
        })
    }

    fn parse_statement(&mut self) -> Result<Spanned<Statement>, ParseError> {
        let saved = std::mem::replace(&mut self.bracket_depth, 0);
        let r = self.parse_statement_raw();
        self.bracket_depth = saved;
        r
    }

    fn parse_statement_raw(&mut self) -> Result<Spanned<Statement>, ParseError> {
        // nested blocks share the expression depth budget: 10 000 nested
        // `if` used to overflow the compiler's own stack (SIGABRT)
        if self.depth >= MAX_EXPR_DEPTH {
            return Err(ParseError::FixIt {
                message: format!("blocks nested more than {} levels deep — move the inner part into its own handler", MAX_EXPR_DEPTH),
                span: self.peek_span(),
            });
        }
        self.depth += 1;
        let r = self.parse_statement_inner();
        self.depth -= 1;
        r
    }

    fn parse_statement_inner(&mut self) -> Result<Spanned<Statement>, ParseError> {
        let start = self.peek_span();
        // a contextual keyword used as a variable (`let variants = 1` is
        // accepted) is a name at the start of a statement too: `variants = 2`,
        // `variants[k] = v` were "expected statement, found Variants"
        let contextual = match self.peek() {
            Token::Variants => Some("variants"), Token::Requires => Some("requires"), Token::Connect => Some("connect"),
            Token::Implies => Some("implies"), Token::Contradicts => Some("contradicts"), Token::After => Some("after"),
            Token::Every => Some("every"),
            _ => None,
        };
        if let Some(name) = contextual {
            if matches!(self.peek_at(1), Token::Eq | Token::LBracket | Token::Dot | Token::PlusEq | Token::MinusEq | Token::StarEq | Token::SlashEq) {
                let pos = self.pos;
                self.tokens[pos].token = Token::Ident(name.to_string());
            }
        }
        match self.peek() {
            Token::Let => {
                self.advance();
                let (name, name_span) = self.expect_ident()?;
                // `match` reads fine as a field (`x.match`) but a variable of
                // that name can never be read back: `match + 1` starts a match
                self.refuse_const(&name, name_span, "a variable")?;
                if name == "match" {
                    return Err(ParseError::FixIt {
                        message: "`match` cannot name a variable (it starts a match expression when read) — pick another name, e.g. `m` or `found`".to_string(),
                        span: name_span,
                    });
                }
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Ok(Spanned::new(Statement::Let { name, value }, start.merge(self.prev_span())))
            }
            Token::Return => {
                self.advance();
                // a bare `return` before `}` returns () (it was "expected
                // expression, found '}'"); a newline after `return` ends it
                // too, so the next line is a statement of its own, not the
                // returned value (it was run as `return log_it(x)`)
                let value = if self.check(&Token::RBrace) || self.check(&Token::Eof) || self.at_line_start() {
                    Spanned::new(Expr::Literal(Literal::Unit), self.prev_span())
                } else {
                    self.parse_expr()?
                };
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
                // for [loop_bound(N)] var in expr { body }
                // or:    for var in expr { body }
                self.advance();
                // Optional [loop_bound(N), ...] annotations.
                let mut bound: Option<u64> = None;
                if self.check(&Token::LBracket) {
                    self.advance();
                    while !self.check(&Token::RBracket) && !self.is_at_end() {
                        let prop_span = self.peek_span();
                        let prop = self.parse_memory_property()?;
                        // `loop_bound(zb)` / `loop_bound(2.5)` were accepted and ignored:
                        // the loop then ran unbounded
                        let ok = match &prop.node {
                            MemoryProperty::Param(pp) if pp.name == "loop_bound" => match pp.values.first().map(|l| &l.node) {
                                Some(Literal::Int(n)) if *n > 0 => { bound = Some(*n as u64); true }
                                _ => false,
                            },
                            _ => false,
                        };
                        if !ok {
                            return Err(ParseError::FixIt {
                                message: "a loop annotation is `[loop_bound(N)]` with N a positive Int literal".to_string(),
                                span: prop_span,
                            });
                        }
                        if self.check(&Token::Comma) { self.advance(); }
                    }
                    self.expect(Token::RBracket)?;
                }
                let (var, var_span) = self.expect_ident()?;
                self.refuse_const(&var, var_span, "a loop variable")?;
                self.expect(Token::In)?;
                let iter = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(
                    Statement::For { var, iter, body, bound },
                    start.merge(self.prev_span()),
                ))
            }
            Token::While => {
                self.advance();
                // Optional [loop_bound(N)] annotation before condition
                let mut bound: Option<u64> = None;
                if self.check(&Token::LBracket) {
                    self.advance();
                    while !self.check(&Token::RBracket) && !self.is_at_end() {
                        let (attr_name, _) = self.expect_ident()?;
                        let good = attr_name == "loop_bound" && self.check(&Token::LParen)
                            && matches!(self.tokens.get(self.pos + 1).map(|t| &t.token), Some(Token::IntLit(n)) if *n > 0);
                        if !good {
                            return Err(ParseError::FixIt {
                                message: "a loop annotation is `[loop_bound(N)]` with N a positive Int literal".to_string(),
                                span: self.peek_span(),
                            });
                        }
                        self.advance();
                        if let Token::IntLit(n) = &self.tokens[self.pos].token { bound = Some(*n as u64); }
                        self.advance(); // consume the int
                        self.expect(Token::RParen)?;
                        if self.check(&Token::Comma) { self.advance(); }
                    }
                    self.expect(Token::RBracket)?;
                }
                let condition = self.parse_expr()?;
                self.expect(Token::LBrace)?;
                let mut body = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    body.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Spanned::new(
                    Statement::While { condition, body, bound },
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
                if !matches!(self.peek(), Token::Else) {
                    return Err(ParseError::FixIt {
                        message: "`require` needs `else Tag` — `require len(rows) < 500 else Full` (the tag is the error kind a caller sees)".to_string(),
                        span: self.tokens[self.pos].span,
                    });
                }
                self.advance();
                // Accept either an identifier or a string literal as the else target
                let else_signal = match self.peek() {
                    Token::StringLit(s) => {
                        let s = s.clone();
                        self.advance();
                        s
                    }
                    _ => {
                        let (name, _) = self.expect_ident()?;
                        // `else Tag "why {x}"`: a machine-readable kind AND
                        // a human detail (encoded Tag\u{1f}text; the
                        // interpreter splits it). The detail sits on the
                        // same line: a string on the next line is a
                        // statement of its own.
                        if let (Token::StringLit(detail), false) = (self.peek().clone(), self.at_line_start()) {
                            let detail = self.consts_in_string(&detail);
                            self.advance();
                            format!("{}\u{1f}{}", name, detail)
                        } else {
                            name
                        }
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
            Token::Native | Token::Checker | Token::Scale | Token::Cost | Token::Protocol => {
                // Could be: assignment, target.method(args), fn_call(args), or bare expr
                let save_pos = self.pos;
                let (name, name_span) = self.expect_ident()?;

                if self.check(&Token::LBracket) || self.check(&Token::Dot) {
                    // lvalue chain assignment: name (.field | [index])+ = value
                    //   g.bet = 10            → name["bet"] = 10
                    //   xs[i] = v             → name[i] = v
                    //   g.board[0] = 99       → name["board"] = with(g["board"], 0, 99)
                    // Nested paths desugar to nested with(); the root set
                    // reuses IndexSet (which handles maps, records, slots).
                    // A `.method(` is not an accessor — it falls through to
                    // expression parsing (method calls, pipes).
                    let mut accessors: Vec<Spanned<Expr>> = Vec::new();
                    let chain_ok = loop {
                        if self.check(&Token::LBracket) {
                            self.advance();
                            let idx = self.parse_expr()?;
                            if !self.check(&Token::RBracket) { break false; }
                            self.advance();
                            accessors.push(idx);
                        } else if self.check(&Token::Dot) {
                            // a keyword is a fine field name here (`j.state = …`)
                            // a capitalised field (`s.LT1 = 5`, PLC tags) too
                            let field = match self.peek_at(1).clone() {
                                Token::Ident(f) | Token::TypeIdent(f) => Some(f),
                                other => keyword_as_name(&other),
                            };
                            if let Some(f) = field {
                                if !matches!(self.peek_at(2), Token::LParen) {
                                    let fspan = self.peek_span();
                                    self.advance(); // '.'
                                    self.advance(); // field ident
                                    accessors.push(Spanned::new(Expr::Literal(Literal::String(f)), fspan));
                                    continue;
                                }
                            }
                            break true;
                        } else {
                            break true;
                        }
                    };
                    let compound = match self.peek() {
                        Token::PlusEq => Some(BinOp::Add),
                        Token::MinusEq => Some(BinOp::Sub),
                        Token::StarEq => Some(BinOp::Mul),
                        Token::SlashEq => Some(BinOp::Div),
                        _ => None,
                    };
                    if chain_ok && !accessors.is_empty() && (self.check(&Token::Eq) || compound.is_some()) {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        let sp = start;
                        // the rewrite below repeats every index expression
                        // (`g[f()][1] = v` read g[f()] and wrote g[f()]; `xs[f()]
                        // += 1` called f three times): an index that calls
                        // anything is evaluated once, into a temporary, first
                        let calls = |e: &Expr| {
                            let mut hit = false;
                            // an interpolated key may call too (`m["k{f()}"] -= 1`)
                            crate::checker::literals::for_each_in_expr(e, &mut |x| if matches!(x, Expr::FnCall { .. } | Expr::MethodCall { .. })
                                || matches!(x, Expr::Literal(Literal::String(t)) if t.contains('{')) { hit = true; });
                            hit
                        };
                        let repeated = compound.is_some() || accessors.len() > 1;
                        let mut hoisted: Vec<Spanned<Statement>> = Vec::new();
                        if repeated {
                            for acc in accessors.iter_mut() {
                                if calls(&acc.node) {
                                    let tmp = format!("__soma_ix_{}_{}", acc.span.start, hoisted.len());
                                    let aspan = acc.span;
                                    crate::ast::note_hoisted(&tmp, &acc.node);
                                    hoisted.push(Spanned::new(Statement::Let { name: tmp.clone(), value: acc.clone() }, aspan));
                                    *acc = Spanned::new(Expr::Ident(tmp), aspan);
                                }
                            }
                        }
                        // `m[k] += 1`, `acc.balance -= x`: read the same chain
                        // and write `read op rhs` back through it
                        let value = match compound {
                            None => rhs,
                            Some(op) => {
                                let mut read = Spanned::new(Expr::Ident(name.clone()), name_span);
                                for acc in &accessors {
                                    read = Spanned::new(
                                        Expr::Index { target: Box::new(read), index: Box::new(acc.clone()) },
                                        sp,
                                    );
                                }
                                let rhs_span = rhs.span;
                                Spanned::new(
                                    Expr::BinaryOp { left: Box::new(read), op, right: Box::new(rhs) },
                                    sp.merge(rhs_span),
                                )
                            }
                        };
                        let k0 = accessors[0].clone();
                        let base0 = Spanned::new(Expr::Index {
                            target: Box::new(Spanned::new(Expr::Ident(name.clone()), name_span)),
                            index: Box::new(k0.clone()),
                        }, sp);
                        let new_val = build_nested_with(base0, &accessors[1..], value, sp);
                        let set = Spanned::new(
                            Statement::IndexSet { name, index: k0, value: new_val },
                            start.merge(self.prev_span()),
                        );
                        if hoisted.is_empty() { return Ok(set); }
                        // one block: the temporaries, then the write (an
                        // assignment inside a block reaches the outer name)
                        let whole = start.merge(self.prev_span());
                        hoisted.push(set);
                        return Ok(Spanned::new(Statement::If {
                            condition: Spanned::new(Expr::Literal(Literal::Bool(true)), whole),
                            then_body: hoisted,
                            else_body: vec![],
                        }, whole));
                    }
                    // not an lvalue assignment — parse the whole thing as
                    // an expression statement (postfix handles reads/calls)
                    self.pos = save_pos;
                    let expr = self.parse_expr()?;
                    Ok(Spanned::new(Statement::ExprStmt { expr }, start.merge(self.prev_span())))
                } else if self.check(&Token::Eq) {
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
                } else if self.check(&Token::LParen) {
                    // fn_call(args) … — an expression statement; parsed as a
                    // full expression so `f() + 1` / `f() |> g()` / `f().x`
                    // as the last line of a block are its value
                    self.pos = save_pos;
                    let expr = self.parse_expr()?;
                    Ok(Spanned::new(
                        Statement::ExprStmt { expr },
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
            Token::Match | Token::Try | Token::Await |
            Token::IntLit(_) | Token::BigIntLit(_) | Token::FloatLit(_) | Token::StringLit(_) |
            Token::True | Token::False | Token::LParen |
            Token::Bang | Token::Minus | Token::LBracket | Token::PercentLit(_) |
            Token::TypeIdent(_) => {
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
        self.bracket_depth += 1;
        let r = self.parse_arg_list_inner();
        self.bracket_depth -= 1;
        r
    }

    fn parse_arg_list_inner(&mut self) -> Result<Vec<Spanned<Expr>>, ParseError> {
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

    /// A chain `a |> f |> g …` / `a + b + c …` builds a tree as deep as it
    /// is long without recursing here: 50 000 pipes made check and verify
    /// run for minutes — the same limit as nesting.
    fn chain_limit(&self, chain: usize) -> Result<(), ParseError> {
        if self.depth + chain >= MAX_EXPR_DEPTH {
            return Err(ParseError::FixIt {
                message: format!("an operator chain longer than {} steps (with its nesting) — split it into `let` bindings", MAX_EXPR_DEPTH),
                span: self.peek_span(),
            });
        }
        Ok(())
    }

    fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        if self.depth >= MAX_EXPR_DEPTH {
            return Err(ParseError::FixIt {
                message: format!("nested more than {} levels deep (expressions and blocks together) — split it into `let` bindings or a helper handler", MAX_EXPR_DEPTH),
                span: self.peek_span(),
            });
        }
        self.depth += 1;
        let r = self.parse_logical_or();
        self.depth -= 1;
        r
    }

    // `|>` binds tighter than comparisons and `??` but looser than
    // arithmetic: `xs |> pluck("k") == ["a"]` is `(xs |> pluck("k")) == …`
    // and `a + b |> f()` stays `(a + b) |> f()`.
    fn parse_pipe(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_additive()?;
        let mut chain = 0usize;
        while self.check(&Token::Pipe) {
            chain += 1;
            self.chain_limit(chain)?;
            self.advance();
            let mut right = self.parse_additive()?;
            // `x |> h` is `x |> h()` (the runtime calls h(x)): as a bare
            // name it was invisible to termination, cost, route ownership
            // and GET→405 — `n |> up` recursed with "✓ terminates"
            if let Expr::Ident(name) = &right.node {
                right = Spanned::new(Expr::FnCall { name: name.clone(), args: vec![] }, right.span);
            }
            let span = left.span.merge(right.span);
            // `"C" |> delegate("h", x)` is `delegate("C", "h", x)`: as a pipe
            // it was invisible to the call graph (a self-delegating handler
            // "structurally terminated", a size proof missed its writes)
            if let Expr::FnCall { name, args } = &right.node {
                if name == "delegate" {
                    let mut all = vec![left];
                    all.extend(args.iter().cloned());
                    left = Spanned::new(Expr::FnCall { name: name.clone(), args: all }, span);
                    continue;
                }
                // `xs |> std.map(f)` is `std.map(xs, f)`: the piped value
                // goes after the builtin's name
                if name == "_std" && !args.is_empty() {
                    let mut all = vec![args[0].clone(), left];
                    all.extend(args[1..].iter().cloned());
                    left = Spanned::new(Expr::FnCall { name: name.clone(), args: all }, span);
                    continue;
                }
            }
            left = Spanned::new(Expr::Pipe {
                left: Box::new(left),
                right: Box::new(right),
            }, span);
        }
        Ok(left)
    }

    fn parse_logical_or(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let mut left = self.parse_logical_and()?;
        let mut chain = 0usize;
        while self.check(&Token::OrOr) {
            chain += 1;
            self.chain_limit(chain)?;
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
        let mut chain = 0usize;
        while self.check(&Token::AndAnd) {
            chain += 1;
            self.chain_limit(chain)?;
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
        let mut chain = 0usize;
        while self.check(&Token::NullCoal) {
            chain += 1;
            self.chain_limit(chain)?;
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
        let left = self.parse_pipe()?;

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
            let right = self.parse_pipe()?;
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
        let mut chain = 0usize;
        while matches!(self.peek(), Token::Plus | Token::Minus | Token::PlusPlus) {
            // `a ++ b` joins two lists or two Strings: concat(a, b)
            if matches!(self.peek(), Token::PlusPlus) {
                chain += 1;
                self.chain_limit(chain)?;
                self.advance();
                let right = self.parse_multiplicative()?;
                let span = left.span.merge(right.span);
                left = Spanned::new(Expr::FnCall { name: "concat".to_string(), args: vec![left, right] }, span);
                continue;
            }
            // a line starting with `-` TOUCHING its operand (`-amount`) is a
            // new expression, the value of a block: `let fee = 2` then
            // `-amount` read as `2 - amount`. `- amount` (spaced) still
            // continues the line before, as a leading `+` does.
            if matches!(self.peek(), Token::Minus) && self.at_line_start() && self.bracket_depth == 0
                && self.tokens.get(self.pos + 1).map_or(false, |n| n.span.start == self.peek_span().end)
            {
                break;
            }
            chain += 1;
            self.chain_limit(chain)?;
            // `"neg"  -1..1 -> "small"`: the `-1` starts the NEXT match arm
            // (a negative pattern), it is not a subtraction from this result
            if matches!(self.peek(), Token::Minus)
                && matches!(self.peek_at(1), Token::IntLit(_) | Token::FloatLit(_) | Token::BigIntLit(_))
                && matches!(self.peek_at(2), Token::DotDot | Token::Arrow | Token::OrOr)
            {
                break;
            }
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
        let mut chain = 0usize;
        while matches!(self.peek(), Token::Star | Token::Slash | Token::Percent) {
            chain += 1;
            self.chain_limit(chain)?;
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
        // `- - - … 1` (50 000 deep) recursed here past the depth budget and
        // aborted `soma serve` with a native stack overflow
        if self.depth >= MAX_EXPR_DEPTH {
            return Err(ParseError::FixIt {
                message: format!("nested more than {} levels deep (expressions and blocks together) — split it into `let` bindings or a helper handler", MAX_EXPR_DEPTH),
                span: self.peek_span(),
            });
        }
        self.depth += 1;
        let r = self.parse_unary_inner();
        self.depth -= 1;
        r
    }

    fn parse_unary_inner(&mut self) -> Result<Spanned<Expr>, ParseError> {
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
            // can be written as a literal without overflow — unless a
            // suffix follows it (`-2.abs()` is -(2.abs()); the fold left
            // `.abs()` behind, a parse error)
            let suffixed = matches!(self.peek_at(1), Token::Dot | Token::LBracket);
            match if suffixed { Token::Eof } else { self.peek().clone() } {
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
                    // the `0` of a unary minus is zero-width: that marks it
                    // (a runtime error says "cannot negate", not "0 - null")
                    left: Box::new(Spanned::new(Expr::Literal(Literal::Int(0)), Span::new(start.start, start.start))),
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

                // `let k = m.len` then `(n * 2)` on the next line is not a
                // call of `len` (it answered `()`)
                if self.check(&Token::LParen) && !self.at_line_start() {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    let end = self.peek_span();
                    self.expect(Token::RParen)?;
                    let span = expr.span.merge(end);
                    // `"C".delegate("h", x)` is `delegate("C", "h", x)` (the
                    // analyses follow only the call form)
                    // 3.0: `std.len(x)` is the BUILTIN len, whatever handlers
                    // the program names `len`
                    if matches!(&expr.node, Expr::Ident(n) if n == "std") {
                        let mut all = vec![Spanned::new(Expr::Literal(Literal::String(field.clone())), field_span)];
                        all.extend(args);
                        expr = Spanned::new(Expr::FnCall { name: "_std".to_string(), args: all }, span);
                        continue;
                    }
                    if field == "delegate" && matches!(expr.node, Expr::Literal(Literal::String(_))) {
                        let mut all = vec![expr];
                        all.extend(args);
                        expr = Spanned::new(Expr::FnCall { name: field, args: all }, span);
                        continue;
                    }
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
            } else if self.check(&Token::LBracket) && !self.at_line_start() {
                // Postfix index: expr[index] — list position, map key, or
                // string char index.
                self.advance();
                let index = self.parse_expr()?;
                let end = self.peek_span();
                self.expect(Token::RBracket)?;
                let span = expr.span.merge(end);
                expr = Spanned::new(
                    Expr::Index {
                        target: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                );
            } else if self.check(&Token::LParen)
                && self.peek_span().start == self.prev_span().end
                && (matches!(expr.node, Expr::FnCall { .. } | Expr::Index { .. } | Expr::MethodCall { .. })
                    || matches!(self.tokens[self.pos.saturating_sub(1)].token, Token::RParen))
            {
                // `mk(5)(n)` / `fs[0](n)` / `(f)(n)` / `(x => x + 1)(n)`:
                // calling an expression's result — it parsed as two
                // statements and `return` gave back the lambda
                return Err(ParseError::FixIt {
                    message: "calling the result of a call, an index or a parenthesized expression directly (`f(a)(b)`, `fs[0](x)`, `(x => x + 1)(2)`) is not supported — bind it first: `let g = fs[0]` then `g(x)`".to_string(),
                    span: self.peek_span(),
                });
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

    /// The body of a lambda, after `=>`: an expression, or a `{ … }` block
    /// whose last expression is the value.
    fn finish_lambda(&mut self, name: String, start: Span) -> Result<Spanned<Expr>, ParseError> {
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
                        // the last statement is the value only when it IS an
                        // expression: an `emit` / `set` / `transition(...)`
                        // statement there was dropped (it never ran)
                        let last_is_value = stmts.last().map_or(false, |l| matches!(l.node, Statement::ExprStmt { .. } | Statement::Return { .. }));
                        if !last_is_value && !stmts.is_empty() {
                            return Ok(Spanned::new(
                                Expr::LambdaBlock { param: name, stmts, result: Box::new(result) },
                                start.merge(self.prev_span()),
                            ));
                        }
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
                        let body = self.parse_expr()?;
                        return Ok(Spanned::new(
                            Expr::Lambda { param: name, body: Box::new(body) },
                            start.merge(self.prev_span()),
                        ));
                    }
    }

    /// `(a, b) => …` (3.0): a lambda of several parameters. Called with that
    /// many arguments (`f(1, 2)`), or handed ONE composite by a builtin — a
    /// pair, `reduce`'s {acc, val}, an entry {key, value} — which it takes
    /// apart in order. Read as a block lambda over `__argsN`.
    fn parse_multi_param_lambda(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.peek_span();
        self.advance(); // (
        let mut params: Vec<(String, Span)> = Vec::new();
        loop {
            let (name, sp) = self.expect_ident()?;
            self.refuse_const(&name, sp, "a lambda parameter")?;
            if params.iter().any(|(p, _)| *p == name) {
                return Err(ParseError::FixIt { message: format!("lambda parameter `{name}` is named twice"), span: sp });
            }
            params.push((name, sp));
            if self.check(&Token::Comma) { self.advance(); } else { break; }
        }
        self.expect(Token::RParen)?;
        self.expect(Token::FatArrow)?;
        let packed = format!("__args{}", params.len());
        let lets: Vec<Spanned<Statement>> = params.iter().enumerate().map(|(i, (name, sp))| Spanned::new(
            Statement::Let {
                name: name.clone(),
                value: Spanned::new(Expr::Index {
                    target: Box::new(Spanned::new(Expr::Ident(packed.clone()), *sp)),
                    index: Box::new(Spanned::new(Expr::Literal(Literal::Int(i as i64)), *sp)),
                }, *sp),
            }, *sp)).collect();
        let lambda = self.finish_lambda(packed, start)?;
        let span = lambda.span;
        Ok(Spanned::new(match lambda.node {
            Expr::Lambda { param, body } => Expr::LambdaBlock { param, stmts: lets, result: body },
            Expr::LambdaBlock { param, stmts, result } => Expr::LambdaBlock { param, stmts: lets.into_iter().chain(stmts).collect(), result },
            other => other,
        }, span))
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
                let s = self.consts_in_string(&s);
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::String(s)), start))
            }
            // a constant of this cell IS its literal
            // (`if amount > LIMIT {` — the brace opens the block: only
            // `Name { field: …` is a record of that name)
            Token::Ident(name) | Token::TypeIdent(name) if self.consts.contains_key(&name)
                && !matches!(self.peek_at(1), Token::LParen)
                && !(matches!(self.peek_at(1), Token::LBrace) && matches!(self.peek_at(2), Token::Ident(_)) && matches!(self.peek_at(3), Token::Colon)) => {
                if matches!(self.peek_at(1), Token::FatArrow) {
                    return Err(ParseError::FixIt {
                        message: format!("`{name}` is a constant of this cell (`const {name} = …`) and cannot name a lambda parameter — rename one of them"),
                        span: start,
                    });
                }
                if matches!(self.peek_at(1), Token::Eq | Token::PlusEq | Token::MinusEq | Token::StarEq | Token::SlashEq) {
                    return Err(ParseError::FixIt {
                        message: format!("`{name}` is a constant (`const {name} = …`) and cannot be assigned — a value that changes lives in a `memory` slot or a `let`"),
                        span: start,
                    });
                }
                let lit = self.consts[&name].clone();
                self.advance();
                // `const MAX = Lots(50)`: each read is `Lots(50)`, so the
                // type pass sees what the cap counts (erased to 50 after it)
                if let Some(n) = self.const_nominal.get(&name) {
                    return Ok(Spanned::new(Expr::FnCall { name: n.clone(), args: vec![Spanned::new(Expr::Literal(lit), start)] }, start));
                }
                Ok(Spanned::new(Expr::Literal(lit), start))
            }
            // 3.0: a durable wait — `await approve(msg)`, `await signal(name,
            // key)`, `await sleep(duration)`. The call is the builtin
            // `await_<what>`; where it may stand is `check`'s rule (kind `await`).
            Token::Await => {
                self.advance();
                let what = match self.peek().clone() {
                    Token::Signal => "signal".to_string(),
                    Token::Ident(n) if n == "approve" || n == "sleep" => n,
                    _ => return Err(ParseError::FixIt {
                        message: "`await` waits for one of three things: `await approve(message)`, `await signal(name, key)`, `await sleep(duration)` — a handler call is awaited already (it returns when it is done)".to_string(),
                        span: self.peek_span(),
                    }),
                };
                self.advance();
                self.expect(Token::LParen)?;
                let args = self.parse_arg_list()?;
                self.expect(Token::RParen)?;
                Ok(Spanned::new(Expr::FnCall { name: format!("await_{}", what), args }, start.merge(self.prev_span())))
            }
            Token::True => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(true)), start))
            }
            Token::False => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(false)), start))
            }
            Token::DurationLit(val, unit, ms) => {
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
                    Expr::Literal(Literal::Duration(Duration { value: val, unit: du, ms })),
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
                    return self.finish_lambda(name, start);
                }
                // Check for function call: name(args) — on the same line
                if self.check(&Token::LParen) && !self.at_line_start() {
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
                // Check for record literal: User { name: "Alice", age: 30 }.
                // `if s == Pending { return 1 }` is NOT one: the brace opens
                // the block — a record literal starts with `field:` or is `{}`
                let record_follows = self.check(&Token::LBrace) && (
                    matches!(self.peek_at(1), Token::RBrace)
                    || ((matches!(self.peek_at(1), Token::Ident(_) | Token::TypeIdent(_)) || keyword_as_name(self.peek_at(1)).is_some())
                        && matches!(self.peek_at(2), Token::Colon))
                );
                if record_follows {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&Token::RBrace) && !self.is_at_end() {
                        // a capitalised field name (`Tank { LT1: 3 }`) too
                        let field_name = if let Token::TypeIdent(f) = self.peek().clone() {
                            self.advance();
                            f
                        } else {
                            self.expect_ident()?.0
                        };
                        self.expect(Token::Colon)?;
                        let value = self.parse_expr()?;
                        fields.push((field_name, value));
                        if self.check(&Token::Comma) { self.advance(); }
                    }
                    self.expect(Token::RBrace)?;
                    Ok(Spanned::new(Expr::Record { type_name: name, fields }, start.merge(self.prev_span())))
                } else if self.check(&Token::LParen) {
                    // Tuple-variant constructor: Up(3), Move(x, y).
                    // Parsed identically to a function call; the interpreter
                    // dispatches on variant_registry.
                    self.advance();
                    let args = self.parse_arg_list()?;
                    let end = self.peek_span();
                    self.expect(Token::RParen)?;
                    Ok(Spanned::new(
                        Expr::FnCall { name, args },
                        start.merge(end),
                    ))
                } else {
                    Ok(Spanned::new(Expr::Ident(name), start))
                }
            }
            // Keywords that can be used as variable/function names in expressions
            Token::State | Token::Type | Token::Effect | Token::Guard | Token::Initial |
            Token::Start | Token::Test | Token::Backend | Token::Builtin |
            Token::Signal | Token::Emit | Token::Check | Token::Memory | Token::Face |
            Token::Property | Token::Rules | Token::Runtime | Token::Matches |
            Token::Native | Token::Checker | Token::Scale | Token::Cost | Token::Protocol => {
                let name = format!("{:?}", self.peek()).to_lowercase();
                // Get the keyword as a string name
                let name = match self.peek() {
                    Token::State => "state", Token::Type => "type", Token::Effect => "effect",
                    Token::Guard => "guard", Token::Initial => "initial", Token::Start => "start",
                    Token::Test => "test", Token::Backend => "backend", Token::Builtin => "builtin",
                    Token::Signal => "signal", Token::Emit => "emit", Token::Check => "check", Token::Memory => "memory",
                    Token::Face => "face", Token::Property => "property", Token::Rules => "rules",
                    Token::Runtime => "runtime", Token::Matches => "matches", Token::Native => "native",
                    Token::Checker => "checker", Token::Scale => "scale", Token::Cost => "cost", Token::Protocol => "protocol",
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
                let then_result = block_result(&mut then_stmts, self.prev_span());
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
                    let else_result = block_result(&mut else_stmts, self.prev_span());
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

                    // Fix-it: `pattern => expr` is the Rust spelling.
                    if self.check(&Token::FatArrow) {
                        let err = ParseError::FixIt {
                            message: "match arms use '->', not '=>'".to_string(),
                            span: self.peek_span(),
                        };
                        // the arm is read as if `->` were written
                        if self.can_recover() {
                            self.recovered.push((err, true));
                            self.advance();
                        } else {
                            return Err(err);
                        }
                    } else {
                        self.expect(Token::Arrow)?;
                    }

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
                // try { expr } → {value, error, kind, detail}. A block of several
                // statements is allowed too: `try { _take(sku, qty)  transition(id, "held") }`
                // — it is desugared to a one-arm match (whose body every analysis
                // already walks); the value is the block's last expression.
                self.advance();
                self.expect(Token::LBrace)?;
                let mut stmts: Vec<Spanned<Statement>> = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    stmts.push(self.parse_statement()?);
                }
                self.expect(Token::RBrace)?;
                let span = start.merge(self.prev_span());
                let inner = if stmts.len() == 1 && matches!(stmts[0].node, Statement::ExprStmt { .. }) {
                    match stmts.pop().unwrap().node {
                        Statement::ExprStmt { expr } => expr,
                        _ => unreachable!(),
                    }
                } else {
                    let result = match stmts.last().map(|s| &s.node) {
                        Some(Statement::ExprStmt { .. }) => match stmts.pop().unwrap().node {
                            Statement::ExprStmt { expr } => expr,
                            _ => unreachable!(),
                        },
                        _ => Spanned::new(Expr::Literal(Literal::Unit), span),
                    };
                    Spanned::new(
                        Expr::Match {
                            subject: Box::new(Spanned::new(Expr::Literal(Literal::Unit), span)),
                            arms: vec![MatchArm { pattern: MatchPattern::Wildcard, guard: None, body: stmts, result }],
                        },
                        span,
                    )
                };
                Ok(Spanned::new(Expr::Try(Box::new(inner)), span))
            }
            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(&Token::RBracket) && !self.is_at_end() {
                    self.bracket_depth += 1;
                    let e = self.parse_expr();
                    self.bracket_depth -= 1;
                    elements.push(e?);
                    // the comma was optional: `[1 -1]` read as `[0]` and
                    // `["a" "b"]` as two elements, while `[1 2]` passed
                    if self.check(&Token::Comma) { self.advance(); } else if !self.check(&Token::RBracket) && !self.is_at_end() {
                        return Err(ParseError::FixIt {
                            message: format!("expected ',' or ']' in a list, found {} — list elements are separated by commas: `[a, b]`", display_token(&self.peek().clone())),
                            span: self.peek_span(),
                        });
                    }
                }
                self.expect(Token::RBracket)?;
                Ok(Spanned::new(Expr::ListLiteral(elements), start.merge(self.prev_span())))
            }
            // `(a, b) => …`: a parameter list, not a parenthesised value
            Token::LParen if {
                let mut k = 1;
                let mut names = 0;
                loop {
                    if !matches!(self.peek_at(k), Token::Ident(_) | Token::TypeIdent(_)) { break false; }
                    names += 1;
                    match self.peek_at(k + 1) {
                        Token::Comma => k += 2,
                        Token::RParen => break names >= 2 && matches!(self.peek_at(k + 2), Token::FatArrow),
                        _ => break false,
                    }
                }
            } => self.parse_multi_param_lambda(),
            Token::LParen => {
                self.advance();
                // Check for Unit literal: ()
                if self.check(&Token::RParen) {
                    let end = self.peek_span();
                    self.advance();
                    return Ok(Spanned::new(Expr::Literal(Literal::Unit), start.merge(end)));
                }
                self.bracket_depth += 1;
                let expr = self.parse_expr();
                self.bracket_depth -= 1;
                let expr = expr?;
                let end = self.peek_span();
                self.expect(Token::RParen)?;
                // the span covers the parentheses: `(x |> f()) + 1` echoes whole
                Ok(Spanned::new(expr.node, start.merge(end)))
            }
            Token::After | Token::Every | Token::Ensure | Token::Requires | Token::Connect
                | Token::Variants | Token::Implies | Token::Contradicts => {
                let name = match self.peek() {
                    Token::After => "after", Token::Every => "every", Token::Ensure => "ensure",
                    Token::Requires => "requires", Token::Connect => "connect", Token::Variants => "variants",
                    Token::Implies => "implies", _ => "contradicts",
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
            ref t if reserved_word(t).is_some() && !matches!(t, Token::If | Token::Try) => Err(reserved_error(t, start).unwrap()),
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
        } else if let Token::TypeIdent(ref t) = self.peek().clone() {
            // Variant pattern: PascalCase identifier in pattern position.
            //   Pending              -> unit variant
            //   Accepted { id, .. }  -> struct-variant pattern
            //   Up(n)                -> tuple-variant pattern
            //   Foo::Bar             -> qualified variant
            let first = t.clone();
            self.advance();
            // Optional ::Name qualification.
            let (type_name, name) = if self.check(&Token::Colon) {
                // Look for ::
                if self.pos + 1 < self.tokens.len()
                    && matches!(self.tokens[self.pos + 1].token, Token::Colon)
                {
                    self.advance(); // first :
                    self.advance(); // second :
                    if let Token::TypeIdent(ref second) = self.peek().clone() {
                        let name = second.clone();
                        self.advance();
                        (Some(first), name)
                    } else {
                        return Err(ParseError::Expected {
                            expected: "variant name after '::'".to_string(),
                            found: self.peek().clone(),
                            span: self.peek_span(),
                        });
                    }
                } else {
                    (None, first)
                }
            } else {
                (None, first)
            };
            // Optional fields.
            let fields = if self.check(&Token::LBrace) {
                // Struct-variant pattern: { field, field2: pat, .. }
                self.advance();
                let mut fields = Vec::new();
                let mut rest = false;
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    if self.check(&Token::DotDot) {
                        self.advance();
                        rest = true;
                        break;
                    }
                    let (field_name, _) = self.expect_ident()?;
                    if self.check(&Token::Colon) {
                        self.advance();
                        let sub = self.parse_match_pattern()?;
                        fields.push((field_name, sub));
                    } else {
                        fields.push((field_name.clone(), MatchPattern::Variable(field_name)));
                    }
                    if self.check(&Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(Token::RBrace)?;
                VariantPatternFields::Struct { fields, rest }
            } else if self.check(&Token::LParen) {
                // Tuple-variant pattern: (sub_pat, sub_pat, ...)
                self.advance();
                let mut subs = Vec::new();
                while !self.check(&Token::RParen) && !self.is_at_end() {
                    subs.push(self.parse_match_pattern()?);
                    if self.check(&Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(Token::RParen)?;
                VariantPatternFields::Tuple(subs)
            } else {
                VariantPatternFields::Unit
            };
            Ok(MatchPattern::Variant { type_name, name, fields })
        } else if self.check(&Token::Minus) {
            // Negative number pattern: -5, -3.14
            self.advance();
            let lit = self.parse_literal()?;
            match lit.node {
                // -10..-2 / -5..5: a range with a negative lower bound
                Literal::Int(n) if self.check(&Token::DotDot) => {
                    self.advance();
                    let neg = if self.check(&Token::Minus) { self.advance(); true } else { false };
                    if let Token::IntLit(to) = self.peek().clone() {
                        self.advance();
                        Ok(MatchPattern::Range { from: -n, to: if neg { -to } else { to } })
                    } else {
                        Err(ParseError::Expected { expected: "an Int upper bound after `..`".to_string(), found: self.peek().clone(), span: self.peek_span() })
                    }
                }
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
                    let neg = if self.check(&Token::Minus) { self.advance(); true } else { false };
                    if let Token::IntLit(to) = self.peek().clone() {
                        self.advance();
                        return Ok(MatchPattern::Range { from, to: if neg { -to } else { to } });
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
            Token::DurationLit(val, unit, ms) => {
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
                    Literal::Duration(Duration { value: val, unit: du, ms }),
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

    /// Parse and expect failure; return the rendered error message.
    fn parse_err(input: &str) -> String {
        match parse(input) {
            Ok(_) => panic!("expected parse error for: {}", input),
            Err(e) => e.to_string(),
        }
    }

    // ── Fix-it diagnostics: the error message contains its own correction ──

    #[test]
    fn test_fixit_handler_return_type() {
        let err = parse_err("cell C { on add(a: Int) -> Int { return a } }");
        assert!(
            err.contains("handlers do not declare return types — put '-> Int' on the signal declaration inside face { }, then write: on add(a: Int) { ... }"),
            "got: {}", err
        );
    }

    #[test]
    fn test_fixit_match_fat_arrow() {
        let err = parse_err(r#"
            cell C {
                on f(x: Int) {
                    return match x {
                        1 => "one"
                        _ => "many"
                    }
                }
            }
        "#);
        assert!(err.contains("match arms use '->', not '=>'"), "got: {}", err);
    }

    #[test]
    fn test_fixit_state_machine_initial_equals() {
        let err = parse_err(r#"
            cell C {
                state doc {
                    initial = draft
                    draft -> published
                }
            }
        "#);
        assert!(
            err.contains("state machines declare the start state as 'initial: draft' (colon, not equals)"),
            "got: {}", err
        );
    }

    #[test]
    fn test_fixit_transition_guard_when() {
        let err = parse_err(r#"
            cell C {
                state doc {
                    initial: draft
                    draft -> published when ready { }
                }
            }
        "#);
        assert!(
            err.contains("guards are declared inside the transition block: draft -> published { guard { cond } }"),
            "got: {}", err
        );
    }

    // ── assert_fails: negative test assertions ──────────────────────

    #[test]
    fn test_parse_assert_fails_rule() {
        let input = r#"
            cell test T {
                rules {
                    assert f(1) == 2
                    assert_fails transition("1", "Delivered")
                }
            }
        "#;
        let program = parse(input).unwrap();
        let cell = &program.cells[0].node;
        if let Section::Rules(ref rules) = cell.sections[0].node {
            assert_eq!(rules.rules.len(), 2);
            assert!(matches!(rules.rules[0].node, Rule::Assert(_)));
            assert!(matches!(rules.rules[1].node, Rule::AssertFails(_)));
        } else {
            panic!("expected rules section");
        }
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

/// Build the value expression for a nested lvalue assignment by folding
/// `with()` from the outside in. `base` is the already-indexed root
/// (`name[k0]`); `accs` are the remaining accessors after the first.
///   build(base, [], v)        = v
///   build(base, [a, rest], v) = with(base, a, build(base[a], rest, v))
fn build_nested_with(
    base: Spanned<Expr>,
    accs: &[Spanned<Expr>],
    value: Spanned<Expr>,
    sp: Span,
) -> Spanned<Expr> {
    if accs.is_empty() {
        return value;
    }
    let key = accs[0].clone();
    let base_indexed = Spanned::new(
        Expr::Index {
            target: Box::new(base.clone()),
            index: Box::new(key.clone()),
        },
        sp,
    );
    let inner = build_nested_with(base_indexed, &accs[1..], value, sp);
    Spanned::new(
        Expr::FnCall {
            name: "with".to_string(),
            args: vec![base, key, inner],
        },
        sp,
    )
}

/// The value of an if-expression block: its last expression (popped from
/// the statements) — a nested `if … else …` STATEMENT included, which is an
/// if-expression here (`else { if k == "y" { 2 } else { 3 } }` was `()`).
fn block_result(stmts: &mut Vec<Spanned<Statement>>, span: Span) -> Spanned<Expr> {
    let unit = || Spanned::new(Expr::Literal(Literal::Unit), span);
    match stmts.last().map(|l| &l.node) {
        // a trailing `return` stays a return: `let x = if xs == [] { return
        // "empty" } else { xs[0] }` gave x = "empty" and went on
        Some(Statement::ExprStmt { .. }) => {
            let last = stmts.pop().unwrap();
            match last.node {
                Statement::ExprStmt { expr } => expr,
                _ => unit(),
            }
        }
        Some(Statement::If { else_body, .. }) if !else_body.is_empty() => {
            let last = stmts.pop().unwrap();
            let lspan = last.span;
            let Statement::If { condition, mut then_body, mut else_body } = last.node else { unreachable!() };
            let then_result = block_result(&mut then_body, lspan);
            let else_result = block_result(&mut else_body, lspan);
            Spanned::new(Expr::IfExpr {
                condition: Box::new(condition),
                then_body,
                then_result: Box::new(then_result),
                else_body,
                else_result: Box::new(else_result),
            }, lspan)
        }
        _ => unit(),
    }
}
