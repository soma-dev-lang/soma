use crate::ast::Span;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LexError {
    #[error("unexpected character '{ch}'")]
    UnexpectedChar { ch: char, pos: usize },
    #[error("unterminated string")]
    UnterminatedString { pos: usize },
    #[error("unterminated block comment")]
    UnterminatedComment { pos: usize },
    #[error("invalid number")]
    InvalidNumber { pos: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Cell,
    Face,
    Memory,
    Interior,
    Given,
    Promise,
    Signal,
    Emit,
    Await,
    On,
    Where,
    Else,
    Require,
    Let,
    True,
    False,
    // Control flow
    If,
    Return,
    For,
    In,
    While,
    Break,
    Continue,
    Try,
    Catch,
    Every,
    Match,
    // Operators
    Percent,  // %
    // Imports
    Use,
    // Runtime
    Runtime,
    Connect,
    Start,
    // Meta-cell keywords
    Property,
    Type,
    Checker,
    Backend,
    Builtin,
    Test,
    Assert,
    // State machine
    State,
    Guard,
    Effect,
    Initial,
    Matches,
    Native,
    // Rules keywords
    Rules,
    Contradicts,
    Implies,
    Requires,
    MutexGroup,
    Check,

    // Identifiers and literals
    Ident(String),
    TypeIdent(String),
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    DurationLit(f64, DurationUnitTok),
    PercentLit(f64),

    // Operators
    Lt,       // <
    Gt,       // >
    Le,       // <=
    Ge,       // >=
    EqEq,     // ==
    Ne,       // !=
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    AndAnd,   // &&
    OrOr,     // ||
    Bang,     // !
    Arrow,    // ->
    FatArrow, // =>
    Pipe,     // |>
    NullCoal, // ??
    Eq,       // =
    PlusEq,   // +=
    Dot,      // .

    // Delimiters
    LBrace,   // {
    RBrace,   // }
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    Comma,    // ,
    Colon,    // :

    // Special
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DurationUnitTok {
    Ms,
    S,
    Min,
    H,
    D,
    Years,
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Vec<char>,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments()?;
            if self.pos >= self.chars.len() {
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    span: Span::new(self.pos, self.pos),
                });
                break;
            }
            tokens.push(self.next_token()?);
        }
        Ok(tokens)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        loop {
            // Skip whitespace
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }

            if self.pos + 1 < self.chars.len()
                && self.chars[self.pos] == '/'
                && self.chars[self.pos + 1] == '/'
            {
                // Line comment
                while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                    self.pos += 1;
                }
                continue;
            }

            if self.pos + 1 < self.chars.len()
                && self.chars[self.pos] == '/'
                && self.chars[self.pos + 1] == '*'
            {
                // Block comment
                let start = self.pos;
                self.pos += 2;
                let mut depth = 1;
                while self.pos + 1 < self.chars.len() && depth > 0 {
                    if self.chars[self.pos] == '/' && self.chars[self.pos + 1] == '*' {
                        depth += 1;
                        self.pos += 2;
                    } else if self.chars[self.pos] == '*' && self.chars[self.pos + 1] == '/' {
                        depth -= 1;
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                    }
                }
                if depth > 0 {
                    return Err(LexError::UnterminatedComment { pos: start });
                }
                continue;
            }

            break;
        }
        Ok(())
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> char {
        let ch = self.chars[self.pos];
        self.pos += 1;
        ch
    }

    fn next_token(&mut self) -> Result<SpannedToken, LexError> {
        let start = self.pos;
        let ch = self.advance();

        let token = match ch {
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ',' => Token::Comma,
            ':' => Token::Colon,
            '.' => Token::Dot,
            '+' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::PlusEq
                } else {
                    Token::Plus
                }
            }
            '*' => Token::Star,
            '%' => Token::Percent,
            '?' => {
                if self.peek() == Some('?') {
                    self.advance();
                    Token::NullCoal
                } else {
                    return Err(LexError::UnexpectedChar { ch, pos: start });
                }
            }
            '/' => Token::Slash,

            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Arrow
                } else {
                    Token::Minus
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    Token::FatArrow
                } else {
                    Token::Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Ne
                } else {
                    Token::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Le
                } else {
                    Token::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Ge
                } else {
                    Token::Gt
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    Token::AndAnd
                } else {
                    return Err(LexError::UnexpectedChar { ch, pos: start });
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    Token::OrOr
                } else if self.peek() == Some('>') {
                    self.advance();
                    Token::Pipe
                } else {
                    return Err(LexError::UnexpectedChar { ch, pos: start });
                }
            }

            '"' => {
                // Check for """ (triple-quote multi-line string)
                if self.peek() == Some('"') && self.peek_next() == Some('"') {
                    self.advance(); // consume second "
                    self.advance(); // consume third "
                    // Now scan until closing """
                    let mut content = String::new();
                    loop {
                        if self.pos >= self.chars.len() {
                            return Err(LexError::UnterminatedString { pos: start });
                        }
                        let c = self.chars[self.pos];
                        if c == '"'
                            && self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '"'
                            && self.pos + 2 < self.chars.len() && self.chars[self.pos + 2] == '"'
                        {
                            self.pos += 3; // consume closing """
                            break;
                        }
                        content.push(c);
                        self.pos += 1;
                    }
                    // Strip leading newline after opening """
                    if content.starts_with('\n') {
                        content = content[1..].to_string();
                    }
                    // Strip trailing newline + whitespace before closing """
                    if let Some(last_nl) = content.rfind('\n') {
                        let trailing = &content[last_nl + 1..];
                        if trailing.chars().all(|c| c == ' ' || c == '\t') {
                            content = content[..last_nl].to_string();
                        }
                    }
                    // Dedent: find minimum leading whitespace across non-empty lines
                    let min_indent = content
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| l.len() - l.trim_start().len())
                        .min()
                        .unwrap_or(0);
                    if min_indent > 0 {
                        content = content
                            .lines()
                            .map(|l| {
                                if l.len() >= min_indent {
                                    &l[min_indent..]
                                } else {
                                    l
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                    Token::StringLit(content)
                } else {
                    // Regular string
                    let mut s = String::new();
                    loop {
                        match self.peek() {
                            None => return Err(LexError::UnterminatedString { pos: start }),
                            Some('"') => {
                                self.advance();
                                break;
                            }
                            Some('\\') => {
                                self.advance();
                                match self.peek() {
                                    Some('n') => { self.advance(); s.push('\n'); }
                                    Some('t') => { self.advance(); s.push('\t'); }
                                    Some('\\') => { self.advance(); s.push('\\'); }
                                    Some('"') => { self.advance(); s.push('"'); }
                                    _ => s.push('\\'),
                                }
                            }
                            Some(c) => {
                                self.advance();
                                s.push(c);
                            }
                        }
                    }
                    Token::StringLit(s)
                }
            }

            c if c.is_ascii_digit() => {
                self.pos = start; // back up
                return self.lex_number();
            }

            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut ident = String::new();
                ident.push(c);
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        ident.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                match ident.as_str() {
                    "cell" => Token::Cell,
                    "face" => Token::Face,
                    "memory" => Token::Memory,
                    "interior" => Token::Interior,
                    "given" => Token::Given,
                    "promise" => Token::Promise,
                    "signal" => Token::Signal,
                    "emit" => Token::Emit,
                    "await" => Token::Await,
                    "on" => Token::On,
                    "where" => Token::Where,
                    "else" => Token::Else,
                    "require" => Token::Require,
                    "let" => Token::Let,
                    "true" => Token::True,
                    "false" => Token::False,
                    "if" => Token::If,
                    "return" => Token::Return,
                    "for" => Token::For,
                    "in" => Token::In,
                    "use" => Token::Use,
                    "while" => Token::While,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "try" => Token::Try,
                    "catch" => Token::Catch,
                    "every" => Token::Every,
                    "match" => Token::Match,
                    "runtime" => Token::Runtime,
                    "connect" => Token::Connect,
                    "start" => Token::Start,
                    "property" => Token::Property,
                    "type" => Token::Type,
                    "checker" => Token::Checker,
                    "backend" => Token::Backend,
                    "builtin" => Token::Builtin,
                    "test" => Token::Test,
                    "assert" => Token::Assert,
                    "state" => Token::State,
                    "guard" => Token::Guard,
                    "effect" => Token::Effect,
                    "initial" => Token::Initial,
                    "matches" => Token::Matches,
                    "native" => Token::Native,
                    "rules" => Token::Rules,
                    "contradicts" => Token::Contradicts,
                    "implies" => Token::Implies,
                    "requires" => Token::Requires,
                    "mutex_group" => Token::MutexGroup,
                    "check" => Token::Check,
                    _ => {
                        if ident.chars().next().unwrap().is_ascii_uppercase() {
                            Token::TypeIdent(ident)
                        } else {
                            Token::Ident(ident)
                        }
                    }
                }
            }

            _ => return Err(LexError::UnexpectedChar { ch, pos: start }),
        };

        Ok(SpannedToken {
            token,
            span: Span::new(start, self.pos),
        })
    }

    fn lex_number(&mut self) -> Result<SpannedToken, LexError> {
        let start = self.pos;
        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                num_str.push(c);
                self.advance();
            } else if c == '.' && !is_float {
                // Check if next char is a digit (not a method call)
                if let Some(next) = self.peek_next() {
                    if next.is_ascii_digit() {
                        is_float = true;
                        num_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Handle scientific notation: e/E followed by optional +/- and digits
        if let Some(c) = self.peek() {
            if c == 'e' || c == 'E' {
                // Look ahead to confirm there are digits (possibly after +/-)
                let mut offset = 1;
                if let Some(sign) = self.chars.get(self.pos + offset).copied() {
                    if sign == '+' || sign == '-' {
                        offset += 1;
                    }
                }
                if let Some(d) = self.chars.get(self.pos + offset).copied() {
                    if d.is_ascii_digit() {
                        is_float = true;
                        num_str.push(self.advance()); // e or E
                        if let Some(s) = self.peek() {
                            if s == '+' || s == '-' {
                                num_str.push(self.advance());
                            }
                        }
                        while let Some(d) = self.peek() {
                            if d.is_ascii_digit() {
                                num_str.push(d);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Check for duration suffix or percentage
        if let Some(c) = self.peek() {
            if c == '%' {
                self.advance();
                let val: f64 = num_str
                    .parse()
                    .map_err(|_| LexError::InvalidNumber { pos: start })?;
                return Ok(SpannedToken {
                    token: Token::PercentLit(val),
                    span: Span::new(start, self.pos),
                });
            }

            if c.is_ascii_alphabetic() {
                let suffix_start = self.pos;
                let mut suffix = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphabetic() {
                        suffix.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let unit = match suffix.as_str() {
                    "ms" => Some(DurationUnitTok::Ms),
                    "s" => Some(DurationUnitTok::S),
                    "min" => Some(DurationUnitTok::Min),
                    "h" => Some(DurationUnitTok::H),
                    "d" => Some(DurationUnitTok::D),
                    "years" | "year" => Some(DurationUnitTok::Years),
                    _ => None,
                };

                if let Some(unit) = unit {
                    let val: f64 = num_str
                        .parse()
                        .map_err(|_| LexError::InvalidNumber { pos: start })?;
                    return Ok(SpannedToken {
                        token: Token::DurationLit(val, unit),
                        span: Span::new(start, self.pos),
                    });
                } else {
                    // Not a duration suffix, back up
                    self.pos = suffix_start;
                }
            }
        }

        if is_float {
            let val: f64 = num_str
                .parse()
                .map_err(|_| LexError::InvalidNumber { pos: start })?;
            Ok(SpannedToken {
                token: Token::FloatLit(val),
                span: Span::new(start, self.pos),
            })
        } else {
            let val: i64 = num_str
                .parse()
                .map_err(|_| LexError::InvalidNumber { pos: start })?;
            Ok(SpannedToken {
                token: Token::IntLit(val),
                span: Span::new(start, self.pos),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_cell() {
        let input = r#"cell Counter { face { } }"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Cell);
        assert_eq!(tokens[1].token, Token::TypeIdent("Counter".to_string()));
    }

    #[test]
    fn test_lex_type_ident() {
        let input = "Map<String, Int>";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::TypeIdent("Map".to_string()));
        assert_eq!(tokens[1].token, Token::Lt);
        assert_eq!(tokens[2].token, Token::TypeIdent("String".to_string()));
        assert_eq!(tokens[3].token, Token::Comma);
        assert_eq!(tokens[4].token, Token::TypeIdent("Int".to_string()));
        assert_eq!(tokens[5].token, Token::Gt);
    }

    #[test]
    fn test_lex_memory_properties() {
        let input = "[persistent, consistent, capacity(1000)]";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::LBracket);
        assert_eq!(tokens[1].token, Token::Ident("persistent".to_string()));
        assert_eq!(tokens[2].token, Token::Comma);
        assert_eq!(tokens[3].token, Token::Ident("consistent".to_string()));
        assert_eq!(tokens[4].token, Token::Comma);
        assert_eq!(tokens[5].token, Token::Ident("capacity".to_string()));
        assert_eq!(tokens[6].token, Token::LParen);
        assert_eq!(tokens[7].token, Token::IntLit(1000));
        assert_eq!(tokens[8].token, Token::RParen);
        assert_eq!(tokens[9].token, Token::RBracket);
    }

    #[test]
    fn test_lex_duration() {
        let input = "30min 7years 100ms";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::DurationLit(30.0, DurationUnitTok::Min)));
        assert!(matches!(tokens[1].token, Token::DurationLit(7.0, DurationUnitTok::Years)));
        assert!(matches!(tokens[2].token, Token::DurationLit(100.0, DurationUnitTok::Ms)));
    }

    #[test]
    fn test_lex_percentage() {
        let input = "70%";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::PercentLit(70.0)));
    }

    #[test]
    fn test_lex_string() {
        let input = r#""hello world""#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("hello world".to_string()));
    }

    #[test]
    fn test_lex_operators() {
        let input = "-> => == != <= >= && ||";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Arrow);
        assert_eq!(tokens[1].token, Token::FatArrow);
        assert_eq!(tokens[2].token, Token::EqEq);
        assert_eq!(tokens[3].token, Token::Ne);
        assert_eq!(tokens[4].token, Token::Le);
        assert_eq!(tokens[5].token, Token::Ge);
        assert_eq!(tokens[6].token, Token::AndAnd);
        assert_eq!(tokens[7].token, Token::OrOr);
    }

    #[test]
    fn test_comments() {
        let input = "cell // this is a comment\nCounter";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Cell);
        assert_eq!(tokens[1].token, Token::TypeIdent("Counter".to_string()));
    }

    #[test]
    fn test_block_comment() {
        let input = "cell /* block comment */ Counter";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Cell);
        assert_eq!(tokens[1].token, Token::TypeIdent("Counter".to_string()));
    }

    #[test]
    fn test_triple_quote_multiline() {
        let input = "let x = \"\"\"\n    <html>\n        <body>hello</body>\n    </html>\n    \"\"\"";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[3].token,
            Token::StringLit("<html>\n    <body>hello</body>\n</html>".to_string())
        );
    }

    #[test]
    fn test_triple_quote_inline() {
        let input = r#""""hello""""#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("hello".to_string()));
    }

    #[test]
    fn test_triple_quote_empty() {
        let input = r#""""""""#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("".to_string()));
    }

    #[test]
    fn test_triple_quote_with_inner_quotes() {
        let input = "\"\"\"<div class=\"foo\">bar</div>\"\"\"";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token,
            Token::StringLit("<div class=\"foo\">bar</div>".to_string())
        );
    }

    #[test]
    fn test_regular_string_still_works() {
        let input = r#""hello" "world""#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("hello".to_string()));
        assert_eq!(tokens[1].token, Token::StringLit("world".to_string()));
    }
}
