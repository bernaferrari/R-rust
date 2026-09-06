//! Structured R-ish script generator shared by the fuzz targets.
//!
//! Tokens bias the byte soup toward R-significant syntax so the fuzzer
//! reaches parser/eval edge cases instead of dying in pure noise.

use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
pub struct ScriptInput {
    pub tokens: Vec<Token>,
}

#[derive(Arbitrary, Debug, Clone)]
pub enum Token {
    Lit(u8),
    Quote,
    BraceOpen,
    BraceClose,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    Assign,
    Arrow,
    Pipe,
    Dollar,
    Percent,
    Name(u8),
    Digit(u8),
    Space,
    Newline,
    Tab,
    Backslash,
    Grave,
    Keyword(Keyword),
}

#[derive(Arbitrary, Debug, Clone)]
pub enum Keyword {
    Function,
    If,
    Else,
    While,
    Repeat,
    For,
    In,
    Next,
    Break,
    True,
    False,
    Null,
    Na,
    Inf,
    Nan,
    Local,
    Quote,
    TryCatch,
    Stop,
    Warning,
    Return,
    Gc,
    Rm,
    Library,
}

pub fn render_token(t: &Token) -> String {
    render(&ScriptInput { tokens: vec![t.clone()] })
}

pub fn render(input: &ScriptInput) -> String {
    use Token::*;
    let mut s = String::new();
    for t in &input.tokens {
        match t {
            Lit(b) => s.push(*b as char),
            Quote => s.push('"'),
            BraceOpen => s.push('{'),
            BraceClose => s.push('}'),
            ParenOpen => s.push('('),
            ParenClose => s.push(')'),
            BracketOpen => s.push('['),
            BracketClose => s.push(']'),
            Assign => s.push('='),
            Arrow => s.push_str("<-"),
            Pipe => s.push_str("%>%"),
            Dollar => s.push('$'),
            Percent => s.push('%'),
            Name(b) => s.push((b'a' + (b % 26)) as char),
            Digit(b) => s.push((b'0' + (b % 10)) as char),
            Space => s.push(' '),
            Newline => s.push('\n'),
            Tab => s.push('\t'),
            Backslash => s.push('\\'),
            Grave => s.push('`'),
            Keyword(k) => s.push_str(k.as_str()),
        }
    }
    s
}

impl Keyword {
    fn as_str(&self) -> &'static str {
        match self {
            Keyword::Function => "function",
            Keyword::If => "if",
            Keyword::Else => "else",
            Keyword::While => "while",
            Keyword::Repeat => "repeat",
            Keyword::For => "for",
            Keyword::In => "in",
            Keyword::Next => "next",
            Keyword::Break => "break",
            Keyword::True => "TRUE",
            Keyword::False => "FALSE",
            Keyword::Null => "NULL",
            Keyword::Na => "NA",
            Keyword::Inf => "Inf",
            Keyword::Nan => "NaN",
            Keyword::Local => "local",
            Keyword::Quote => "quote",
            Keyword::TryCatch => "tryCatch",
            Keyword::Stop => "stop",
            Keyword::Warning => "warning",
            Keyword::Return => "return",
            Keyword::Gc => "gc",
            Keyword::Rm => "rm",
            Keyword::Library => "library",
        }
    }
}
