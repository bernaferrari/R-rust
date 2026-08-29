//! Full R expression parser.
//!
//! Recursive-descent parser that converts R source code into SEXP trees
//! suitable for evaluation by `eval_safe()`.
//!
//! Supports:
//! - Numbers (integer and real), strings, identifiers
//! - All R keywords: TRUE, FALSE, NULL, NA, Inf, NaN, NA_real_, NA_integer_, NA_character_, NA_complex_
//! - Binary operators: +, -, *, /, ^, <, >, <=, >=, ==, !=, &, &&, |, ||, %%, %/%
//! - Native pipe: x |> f(), x |> f(y = _), x |> _[i] / _$col extractor chains
//! - Custom infix operators: %in%, %o%, %*%, any %xxx% sequence
//! - Unary minus/plus
//! - Assignment: <-, =, ->, <<-
//! - Function calls: f(x, y), f(x = 1)
//! - Parenthesized expressions: (expr)
//! - Blocks: { expr; expr; ... }
//! - Control flow: if/else, for, while, repeat, break, next, return
//! - Function definitions: function(args) body
//! - Subscript: x[i], x[i, j], x[i, ], x[[i]], x[[i, j]] (empty slots → missing arg)
//! - Member access: x$name, x@slot
//! - Formula: y ~ x
//! - Backtick names: `weird name`
//! - ... varargs

use std::ffi::CString;

use crate::sexp::accessors::{CADR, CAR, CDR, CHAR, PRINTNAME, SETCAR, TAG, TYPEOF};
use crate::sexp::builder::{
    scalar_complex_in, scalar_integer_in, scalar_logical_in, scalar_real_in, scalar_string_in,
};
use crate::sexp::ffi::{FALSE, NA_LOGICAL, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::memory::RArena;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Complex(f64),
    Int(i32),
    Str(String),
    Ident(String),
    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent(String), // %% or %/% or %in% etc. — the full %...% operator text
    // Comparison
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    // Logical
    And,
    And2,
    Or,
    Or2,
    Not,
    // Pipe
    Pipe,
    // Pipe bind (`=>`, gated by the _R_USE_PIPEBIND_ envvar)
    PipeBind,
    // Pipe placeholder (`_`)
    Placeholder,
    // Assignment
    Assign,      // =
    LeftAssign,  // <-
    RightAssign, // ->
    LeftSuper,   // <<-
    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LDoubleBracket,
    RDoubleBracket,
    Comma,
    Semicolon,
    Newline,
    // Special
    Tilde,
    Colon,
    DoubleColon,
    TripleColon,
    Dollar,
    At,
    DotDotDot,
    // Keywords
    KwIf,
    KwElse,
    KwFor,
    KwIn,
    KwWhile,
    KwRepeat,
    KwFunction,
    KwLambda,
    KwBreak,
    KwNext,
    KwReturn,
    // Eof
    Eof,
    // Malformed numeric lexeme ("0x", "0x1p", "1e") — upstream's
    // lexer-level ERROR, rendered as "unexpected input".
    Invalid,
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    /// Position where the most recently returned token starts, i.e. after
    /// leading whitespace and comments — gram.y captures yylloc in
    /// token() right after SkipSpace, so this (not the pre-skip position)
    /// is what upstream location reporting uses.
    last_token_start: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos: 0,
            last_token_start: 0,
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        if self.peek_char() == Some('#') {
            while let Some(ch) = self.peek_char() {
                if ch == '\n' {
                    break;
                }
                self.advance();
            }
        }
    }
    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        self.skip_comment();
        self.last_token_start = self.pos;

        let ch = match self.peek_char() {
            Some(c) => c,
            None => return Token::Eof,
        };

        if ch == '\n' {
            self.advance();
            return Token::Newline;
        }

        if (ch == 'r' || ch == 'R') && matches!(self.peek_char_at(1), Some('"') | Some('\'')) {
            self.advance();
            return self.read_raw_string();
        }

        if ch == '"' || ch == '\'' {
            return self.read_string();
        }

        if ch == '\\' && self.peek_char_at(1) == Some('(') {
            self.advance();
            return Token::KwLambda;
        }

        // Backtick names
        if ch == '`' {
            return self.read_backtick_name();
        }

        // Numbers: digit or .digit
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit()))
        {
            return self.read_number();
        }

        // Identifiers and keywords
        if ch.is_alphabetic() || ch == '.' || ch == '_' {
            return self.read_ident();
        }

        self.advance();

        match ch {
            '+' => Token::Plus,
            '-' => {
                if self.peek_char() == Some('>') {
                    self.advance();
                    Token::RightAssign
                } else {
                    Token::Minus
                }
            }
            '*' => Token::Star,
            '/' => Token::Slash,
            '^' => Token::Caret,
            '%' => {
                // Custom infix operators: %...%
                // Also handles %% and %/%
                if self.peek_char() == Some('/') && self.peek_char_at(1) == Some('%') {
                    // %/% integer division
                    self.advance(); // skip /
                    self.advance(); // skip %
                    Token::Percent("%/%".to_string())
                } else if self.peek_char() == Some('%') {
                    self.advance();
                    Token::Percent("%%".to_string())
                } else {
                    // Read custom %...% operator
                    let mut op = String::from("%");
                    while let Some(c) = self.peek_char() {
                        if c == '%' {
                            self.advance();
                            op.push('%');
                            break;
                        }
                        op.push(c);
                        self.advance();
                    }
                    Token::Percent(op)
                }
            }
            '<' => {
                if self.peek_char() == Some('-') {
                    self.advance();
                    Token::LeftAssign
                } else if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Le
                } else if self.peek_char() == Some('<') {
                    self.advance();
                    if self.peek_char() == Some('-') {
                        self.advance();
                        Token::LeftSuper
                    } else {
                        Token::Lt // fallback, shouldn't happen
                    }
                } else {
                    Token::Lt
                }
            }
            '>' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Ge
                } else {
                    Token::Gt
                }
            }
            '=' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Eq
                } else if self.peek_char() == Some('>') {
                    // gram.y: '=' + '>' → PIPEBIND ("=>"), the pipe-bind
                    // symbol; the feature gate is applied at reduce time.
                    self.advance();
                    Token::PipeBind
                } else {
                    Token::Assign
                }
            }
            '!' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Ne
                } else {
                    Token::Not
                }
            }
            '&' => {
                if self.peek_char() == Some('&') {
                    self.advance();
                    Token::And2
                } else {
                    Token::And
                }
            }
            '|' => {
                if self.peek_char() == Some('|') {
                    self.advance();
                    Token::Or2
                } else if self.peek_char() == Some('>') {
                    self.advance();
                    Token::Pipe
                } else {
                    Token::Or
                }
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => {
                if self.peek_char() == Some('[') {
                    self.advance();
                    Token::LDoubleBracket
                } else {
                    Token::LBracket
                }
            }
            ']' => {
                if self.peek_char() == Some(']') {
                    self.advance();
                    Token::RDoubleBracket
                } else {
                    Token::RBracket
                }
            }
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            '~' => Token::Tilde,
            ':' => {
                if self.peek_char() == Some(':') {
                    self.advance();
                    if self.peek_char() == Some(':') {
                        self.advance();
                        Token::TripleColon
                    } else {
                        Token::DoubleColon
                    }
                } else {
                    Token::Colon
                }
            }
            '$' => Token::Dollar,
            '@' => Token::At,
            '.' => {
                // Check for ...
                if self.peek_char() == Some('.') && self.peek_char_at(1) == Some('.') {
                    self.advance();
                    self.advance();
                    Token::DotDotDot
                } else {
                    Token::Ident(".".to_string())
                }
            }
            _ => Token::Eof,
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let mut has_dot = false;
        let mut has_e = false;

        // Hex literals: 0x... (gram.y `NumericValue`). The mantissa takes
        // hex digits and at most one '.', needs at least one character, and
        // may carry a [pP][+-]?[0-9]+ binary exponent (fractions do not
        // require one, and "0x.8p1" / "0x1." are accepted).
        if self.peek_char() == Some('0')
            && (self.peek_char_at(1) == Some('x') || self.peek_char_at(1) == Some('X'))
        {
            let Some(zero) = self.advance() else {
                return Token::Eof;
            };
            let Some(prefix) = self.advance() else {
                return Token::Eof;
            };
            s.push(zero);
            s.push(prefix);
            let mut nd = 0usize;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_hexdigit() {
                    s.push(ch);
                    self.advance();
                    nd += 1;
                } else if ch == '.' && !has_dot {
                    has_dot = true;
                    s.push(ch);
                    self.advance();
                    nd += 1;
                } else {
                    break;
                }
            }
            if nd == 0 {
                // "0x" with no mantissa: upstream lexical error.
                return Token::Invalid;
            }
            if matches!(self.peek_char(), Some('p') | Some('P')) {
                has_e = true;
                s.push(self.advance().unwrap());
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    s.push(self.advance().unwrap());
                }
                let mut ed = 0usize;
                loop {
                    match self.peek_char() {
                        Some(d) if d.is_ascii_digit() => {
                            s.push(d);
                            self.advance();
                            ed += 1;
                        }
                        _ => break,
                    }
                }
                if ed == 0 {
                    // "0x1p" without exponent digits: upstream lexical error.
                    return Token::Invalid;
                }
            }
            // libc strtod is correctly rounded for C99 hex floats, matching
            // trunk's R_strtod on the hex path; it also accepts arbitrarily
            // long hex integers (beyond u64).
            let Some(v) = (unsafe { crate::mainutils::coerce::parse_double_str(&s) }) else {
                return Token::Invalid;
            };
            if self.peek_char() == Some('i') {
                self.advance();
                return Token::Complex(v);
            }
            if self.peek_char() == Some('L') {
                self.advance();
                return Self::integer_or_float(v, &s, has_dot, has_e);
            }
            return Token::Number(v);
        }

        // Decimal literals. `exp_digits` counts digits after the exponent
        // marker so "1e" / "1e+" can be rejected like upstream.
        let mut exp_digits: Option<usize> = None;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
                if let Some(n) = exp_digits.as_mut() {
                    *n += 1;
                }
            } else if ch == '.' && !has_dot && !has_e {
                has_dot = true;
                s.push(ch);
                self.advance();
            } else if (ch == 'e' || ch == 'E') && !has_e {
                has_e = true;
                has_dot = true;
                s.push(ch);
                self.advance();
                if (self.peek_char() == Some('+') || self.peek_char() == Some('-'))
                    && let Some(sign) = self.advance()
                {
                    s.push(sign);
                }
                exp_digits = Some(0);
            } else {
                // 'L' and everything else ends the literal; the L suffix is
                // handled after the loop (left unconsumed here).
                break;
            }
        }
        if exp_digits == Some(0) {
            // "1e" / "1e+": upstream lexical error.
            return Token::Invalid;
        }

        let v: f64 = s.parse().unwrap_or(0.0);
        if self.peek_char() == Some('i') {
            self.advance();
            return Token::Complex(v);
        }
        if self.peek_char() == Some('L') {
            self.advance();
            return Self::integer_or_float(v, &s, has_dot, has_e);
        }
        // Bare decimal literals are double in R and there are no octal
        // source literals ("010" is ten); only the L suffix makes an integer.
        Token::Number(v)
    }

    /// Trunk's L-suffix rule (gram.y `NumericValue`): the literal becomes an
    /// integer only when its value is integral and fits in `int`; otherwise
    /// the numeric value wins, with one of upstream's literal warnings.
    fn integer_or_float(v: f64, lexeme: &str, has_dot: bool, has_e: bool) -> Token {
        let fits = v.is_finite() && v == v.trunc() && i32::try_from(v as i64).is_ok();
        let lexeme = format!("{lexeme}L");
        if fits {
            if has_dot && !has_e {
                warn_literal(&format!(
                    "integer literal {lexeme} contains unnecessary decimal point"
                ));
            }
            return Token::Int(v as i32);
        }
        if has_dot && !has_e {
            warn_literal(&format!(
                "integer literal {lexeme} contains decimal; using numeric value"
            ));
        } else {
            warn_literal(&format!(
                "non-integer value {lexeme} qualified with L; using numeric value"
            ));
        }
        Token::Number(v)
    }

    fn read_raw_string(&mut self) -> Token {
        let quote = match self.advance() {
            Some('"') => '"',
            Some('\'') => '\'',
            _ => '"',
        };

        // R's r"(...)" form: opening quote is immediately followed by '(' and
        // the string ends at ')"' (parens are delimiters, not part of content).
        if quote == '"' && self.peek_char() == Some('(') {
            self.advance();
            let mut s = String::new();
            loop {
                match self.advance() {
                    Some(')') if self.peek_char() == Some('"') => {
                        self.advance();
                        break;
                    }
                    Some(c) => s.push(c),
                    None => break,
                }
            }
            return Token::Str(s);
        }

        let mut s = String::new();
        loop {
            match self.advance() {
                Some(c) if c == quote => {
                    if self.peek_char() == Some(quote) {
                        s.push(quote);
                        self.advance();
                    } else {
                        break;
                    }
                }
                Some(c) => s.push(c),
                None => break,
            }
        }
        Token::Str(s)
    }

    fn read_string(&mut self) -> Token {
        let quote = self.advance().unwrap_or('"');
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('\'') => s.push('\''),
                    Some(c) if c == quote => s.push(c),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => break,
                },
                Some(c) if c == quote => break,
                Some(c) => s.push(c),
                None => break,
            }
        }
        Token::Str(s)
    }

    fn read_backtick_name(&mut self) -> Token {
        self.advance(); // skip `
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('`') => break,
                Some(c) => s.push(c),
                None => break,
            }
        }
        Token::Ident(s)
    }

    fn read_ident(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_alphanumeric() || ch == '.' || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if (s == "r" || s == "R") && matches!(self.peek_char(), Some('"') | Some('\'')) {
            return self.read_raw_string();
        }

        // A standalone `_` is the pipe placeholder (R 4.2+ `|>`); it is
        // only valid inside the RHS of a pipe and is rejected elsewhere by
        // the post-parse scan.
        if s == "_" {
            return Token::Placeholder;
        }
        // Check keywords
        match s.as_str() {
            "if" => Token::KwIf,
            "else" => Token::KwElse,
            "for" => Token::KwFor,
            "in" => Token::KwIn,
            "while" => Token::KwWhile,
            "repeat" => Token::KwRepeat,
            "function" => Token::KwFunction,
            "break" => Token::KwBreak,
            "next" => Token::KwNext,
            "return" => Token::KwReturn,
            _ => Token::Ident(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Parse error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The message is already upstream-shaped ("unexpected ')' in ..."
        // or "unexpected end of input"); top-level renderers wrap it as
        // `Error: <message>` exactly like Rscript.
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse-error context window, mirroring upstream's `PARSE_CONTEXT_SIZE`:
/// how many trailing source characters are considered for the
/// `in "<context>"` suffix.
const PARSE_CONTEXT_WINDOW: usize = 256;

/// Emit one of the literal-suffix warnings from gram.y's `NumericValue`
/// (e.g. "non-integer value 0x1p1024L qualified with L; using numeric
/// value").
fn warn_literal(message: &str) {
    if let Ok(c) = CString::new(message) {
        unsafe { crate::main::errors::Rf_warning(c.as_ptr()) };
    }
}

/// How a token appears in an upstream "unexpected X" parse error, per
/// gram.y's `yytname_translations` table (constants and symbols in prose,
/// operators and keywords as quoted source text).
fn token_display(tok: &Token) -> String {
    match tok {
        Token::Number(_) | Token::Complex(_) | Token::Int(_) => "numeric constant".to_string(),
        Token::Str(_) => "string constant".to_string(),
        Token::Ident(_) => "symbol".to_string(),
        Token::LeftAssign => "assignment".to_string(),
        Token::Newline => "end of line".to_string(),
        Token::Eof => "end of input".to_string(),
        Token::Invalid => "input".to_string(),
        Token::Percent(op) => format!("'{op}'"),
        Token::Plus => "'+'".to_string(),
        Token::Minus => "'-'".to_string(),
        Token::Star => "'*'".to_string(),
        Token::Slash => "'/'".to_string(),
        Token::Caret => "'^'".to_string(),
        Token::Lt => "'<'".to_string(),
        Token::Gt => "'>'".to_string(),
        Token::Le => "'<='".to_string(),
        Token::Ge => "'>='".to_string(),
        Token::Eq => "'=='".to_string(),
        Token::Ne => "'!='".to_string(),
        Token::And => "'&'".to_string(),
        Token::And2 => "'&&'".to_string(),
        Token::Or => "'|'".to_string(),
        Token::Or2 => "'||'".to_string(),
        Token::Not => "'!'".to_string(),
        Token::Pipe => "'|>'".to_string(),
        Token::PipeBind => "'=>'".to_string(),
        Token::Placeholder => "'_'".to_string(),
        Token::Assign => "'='".to_string(),
        Token::RightAssign => "'->'".to_string(),
        Token::LeftSuper => "'<<-'".to_string(),
        Token::LParen => "'('".to_string(),
        Token::RParen => "')'".to_string(),
        Token::LBrace => "'{'".to_string(),
        Token::RBrace => "'}'".to_string(),
        Token::RBracket => "']'".to_string(),
        Token::LBracket => "'['".to_string(),
        Token::LDoubleBracket => "'[['".to_string(),
        Token::RDoubleBracket => "']]'".to_string(),
        Token::Comma => "','".to_string(),
        Token::Semicolon => "';'".to_string(),
        Token::Tilde => "'~'".to_string(),
        Token::Colon => "':'".to_string(),
        Token::DoubleColon => "'::'".to_string(),
        Token::TripleColon => "':::'".to_string(),
        Token::Dollar => "'$'".to_string(),
        Token::At => "'@'".to_string(),
        Token::DotDotDot => "'...'".to_string(),
        Token::KwIf => "'if'".to_string(),
        Token::KwElse => "'else'".to_string(),
        Token::KwFor => "'for'".to_string(),
        Token::KwIn => "'in'".to_string(),
        Token::KwWhile => "'while'".to_string(),
        Token::KwRepeat => "'repeat'".to_string(),
        Token::KwFunction => "'function'".to_string(),
        Token::KwLambda => "'\\('".to_string(),
        Token::KwBreak => "'break'".to_string(),
        Token::KwNext => "'next'".to_string(),
        Token::KwReturn => "'return'".to_string(),
    }
}

/// The print name of a symbol as an owned Rust string.
fn symbol_name_string(sym: SEXP) -> String {
    unsafe {
        let chars = CHAR(PRINTNAME(sym));
        if chars.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Upstream's `StringTrue`: the accepted spellings of a true envvar value.
fn string_true(value: &str) -> bool {
    matches!(value, "T" | "True" | "TRUE" | "true")
}

/// gram.y `xxpipebind`'s feature gate: `_R_USE_PIPEBIND_` must hold a true
/// value for `=>` to parse. Upstream caches the lookup in a function-local
/// static that short-circuits once it has seen a true value; the flag
/// below reproduces that sticky-true behavior process-wide.
fn pipebind_enabled() -> bool {
    use std::sync::atomic::{AtomicI32, Ordering};
    static USE_PIPEBIND: AtomicI32 = AtomicI32::new(0);
    if USE_PIPEBIND.load(Ordering::Relaxed) == 1 {
        return true;
    }
    let enabled = std::env::var("_R_USE_PIPEBIND_")
        .map(|v| string_true(&v))
        .unwrap_or(false);
    if enabled {
        USE_PIPEBIND.store(1, Ordering::Relaxed);
    }
    enabled
}

/// gram.y `checkForPipeBind`: whether a `=>` call survived anywhere in the
/// expression tree (recursing through language object CARs only).
fn expr_contains_pipebind(expr: SEXP) -> bool {
    if expr.is_null() {
        return false;
    }
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let pipebind_sym = Rf_install(c"=>".as_ptr());
        let nil = R_NilValue();
        let mut cur = expr;
        while cur != nil {
            let car = CAR(cur);
            if car == pipebind_sym || expr_contains_pipebind(car) {
                return true;
            }
            cur = CDR(cur);
        }
        false
    }
}

/// Functions upstream marks IS_SPECIAL_SYMBOL (names.c `Spec_name`); gram.y's
/// `check_rhs()` screens these out of pipe RHS calls (`9 |> `/`(3)` is an
/// error, not division).
fn is_special_rhs_function(name: &str) -> bool {
    matches!(
        name,
        "if" | "while"
            | "repeat"
            | "for"
            | "break"
            | "next"
            | "return"
            | "function"
            | "("
            | "{"
            | "+"
            | "-"
            | "*"
            | "/"
            | "^"
            | "%%"
            | "%/%"
            | "%*%"
            | ":"
            | "::"
            | ":::"
            | "?"
            | "|>"
            | "~"
            | "@"
            | "=>"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&"
            | "|"
            | "&&"
            | "||"
            | "!"
            | "<-"
            | "<<-"
            | "="
            | "$"
            | "["
            | "[["
            | "$<-"
            | "[<-"
            | "[[<-"
    )
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct Parser<'arena> {
    tokens: Vec<Token>,
    /// Half-open char span `[start, end)` of each token in `source`,
    /// parallel to `tokens`. Used to render upstream-style parse errors
    /// (`unexpected ')' in "<context>"`); the EOF token has an empty span.
    spans: Vec<(usize, usize)>,
    /// The parsed source as chars, for slicing token context windows.
    source: Vec<char>,
    pos: usize,
    arena: &'arena mut RArena,
    /// The shared node representing the pipe placeholder `_`.
    ///
    /// Upstream represents the placeholder as a distinguished string, so it
    /// prints as `"_"` in error messages and can be spotted by identity in
    /// the post-parse scan. One node per parser keeps identity comparisons
    /// valid while user `"_"` string literals remain distinct objects.
    placeholder: SEXP,
    /// Whether the lexer produced any `=>` (PIPEBIND) token. gram.y's
    /// `HavePipeBind`: it arms the post-parse "invalid use of pipe bind
    /// symbol" scan so inputs without `=>` skip it entirely.
    have_pipebind: bool,
}

impl<'arena> Parser<'arena> {
    pub fn new(input: &str, arena: &'arena mut RArena) -> Self {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut spans = Vec::new();
        let mut have_pipebind = false;
        loop {
            let tok = lexer.next_token();
            let end = lexer.pos;
            // True token start (post-whitespace), like upstream yylloc.
            let start = lexer.last_token_start;
            let is_eof = tok == Token::Eof;
            if tok == Token::PipeBind {
                have_pipebind = true;
            }
            tokens.push(tok);
            spans.push((start, end));
            if is_eof {
                break;
            }
        }
        Parser {
            tokens,
            spans,
            source: input.chars().collect(),
            pos: 0,
            arena,
            placeholder: std::ptr::null_mut(),
            have_pipebind,
        }
    }

    /// The shared placeholder node for `_`, allocated on first use.
    fn placeholder_node(&mut self) -> SEXP {
        if self.placeholder.is_null() {
            self.placeholder = self.scalar_string("_");
        }
        self.placeholder
    }

    fn install_symbol(&self, name: &str) -> Result<SEXP, ParseError> {
        let c_name =
            CString::new(name).map_err(|_| ParseError(format!("symbol contains NUL: {name:?}")))?;
        Ok(unsafe { Rf_install(c_name.as_ptr()) })
    }

    fn cons(&mut self, car: SEXP, cdr: SEXP) -> SEXP {
        self.arena.cons(car, cdr, std::ptr::null_mut())
    }

    fn lang2(&mut self, car: SEXP, arg: SEXP) -> SEXP {
        let nil = unsafe { R_NilValue() };
        let arg_cell = self.cons(arg, nil);
        let call = self.cons(car, arg_cell);
        if !call.is_null() {
            unsafe {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
        }
        call
    }

    fn lang3(&mut self, car: SEXP, arg1: SEXP, arg2: SEXP) -> SEXP {
        let nil = unsafe { R_NilValue() };
        let arg2_cell = self.cons(arg2, nil);
        let arg1_cell = self.cons(arg1, arg2_cell);
        let call = self.cons(car, arg1_cell);
        if !call.is_null() {
            unsafe {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
        }
        call
    }

    fn scalar_real(&mut self, value: f64) -> SEXP {
        scalar_real_in(self.arena, value).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_complex(&mut self, imaginary: f64) -> SEXP {
        scalar_complex_in(self.arena, 0.0, imaginary).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_complex_parts(&mut self, real: f64, imaginary: f64) -> SEXP {
        scalar_complex_in(self.arena, real, imaginary).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_integer(&mut self, value: i32) -> SEXP {
        scalar_integer_in(self.arena, value).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_logical(&mut self, value: i32) -> SEXP {
        scalar_logical_in(self.arena, value).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_string(&mut self, value: &str) -> SEXP {
        scalar_string_in(self.arena, value).map_or(std::ptr::null_mut(), |s| s.as_raw())
    }

    fn scalar_na_string(&mut self) -> SEXP {
        let Some(strings) = self.arena.alloc_vector_sexp(SEXPTYPE::STRSXP, 1) else {
            return std::ptr::null_mut();
        };
        let data = unsafe { (*strings.as_raw()).gengc_next_node as *mut SEXP };
        if data.is_null() {
            return std::ptr::null_mut();
        }
        unsafe {
            *data = R_NaString();
        }
        strings.as_raw()
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            // Upstream renders bison "expecting" errors as plain
            // "unexpected X" (gram.y edits the expecting clause away).
            Err(self.unexpected_at(self.pos - 1))
        }
    }

    /// Build the upstream-shaped parse error for the token at `index`:
    /// `unexpected ')' in "<source context>"`, with no context when the
    /// input ran out. The context is the source through the end of the
    /// offending token, windowed to the last `PARSE_CONTEXT_WINDOW` chars
    /// and reduced to its final two lines (source.c `parseError`).
    fn unexpected_at(&self, index: usize) -> ParseError {
        let tok = self.tokens.get(index).cloned().unwrap_or(Token::Eof);
        let head = format!("unexpected {}", token_display(&tok));
        if matches!(tok, Token::Eof) {
            // The REPL resets the parse context between attempts, so EOF
            // errors surface without one.
            return ParseError(head);
        }
        let end = self
            .spans
            .get(index)
            .map(|&(_, end)| end)
            .unwrap_or(self.source.len());
        let start = end.saturating_sub(PARSE_CONTEXT_WINDOW);
        let window: String = self.source[start..end].iter().collect();
        let mut lines: Vec<&str> = window.split('\n').collect();
        // getParseContext drops the empty line after a trailing newline.
        if matches!(lines.last(), Some(last) if last.is_empty()) {
            lines.pop();
        }
        match lines.as_slice() {
            [] | [""] => ParseError(head),
            [line] => ParseError(format!("{head} in \"{line}\"")),
            lines => ParseError(format!(
                "{head} in:\n\"{}\n{}\"",
                lines[lines.len() - 2],
                lines[lines.len() - 1]
            )),
        }
    }

    /// 1-based line and 0-based column of a char offset in the source.
    /// Upstream counts columns in first-bytes (like chars) and snaps tabs
    /// to 8-column stops; plain char counting matches it off tab runs.
    fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let mut line = 1usize;
        let mut col = 0usize;
        for &ch in &self.source[..offset] {
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Render `<msg> (<input>:line:col)` for a token index. Upstream
    /// captures the location right after SkipSpace consumed the token's
    /// first character (gram.y token()), so the reported column is one
    /// past the token's first character.
    fn token_position_error(&self, index: usize, msg: &str) -> ParseError {
        let (start, _) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((self.source.len(), self.source.len()));
        let (line, col) = self.line_col(start);
        ParseError(format!("{msg} (<input>:{line}:{})", col + 1))
    }

    /// Position for the post-parse per-expression checks (gram.y's
    /// R_Parse1 handler): the parser's lookahead is reported there. A
    /// consumed newline resets the column to 0 (the Status-3 line
    /// adjustment then restores the expression's last line), and EOF
    /// behaves the same way since upstream parses a virtual trailing
    /// newline; any other terminator reports its own position with the
    /// same +1 quirk as `token_position_error`.
    fn pipebind_position_error(&self, msg: &str) -> ParseError {
        let index = self.pos;
        let (start, _) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((self.source.len(), self.source.len()));
        let (line, _) = self.line_col(start);
        match self.tokens.get(index) {
            Some(Token::Newline) | Some(Token::Eof) | None => {
                ParseError(format!("{msg} (<input>:{line}:0)"))
            }
            _ => {
                let (_, col) = self.line_col(start);
                ParseError(format!("{msg} (<input>:{line}:{})", col + 1))
            }
        }
    }

    /// Skip newlines (but not semicolons).
    fn skip_newlines(&mut self) {
        while self.peek() == &Token::Newline {
            self.advance();
        }
    }

    /// Skip semicolons and newlines.
    fn skip_terminators(&mut self) {
        while self.peek() == &Token::Semicolon || self.peek() == &Token::Newline {
            self.advance();
        }
    }

    // -----------------------------------------------------------------------
    // Top-level
    // -----------------------------------------------------------------------

    pub fn parse_top_level_expressions(&mut self) -> Result<Vec<SEXP>, ParseError> {
        let mut exprs = Vec::new();
        loop {
            self.skip_terminators();
            if self.peek() == &Token::Eof {
                break;
            }
            let expr = self.parse_expr()?;
            // gram.y rejects any `_` placeholder that survived the pipe
            // rewrite: nested placeholders or `_` used outside a pipe.
            if self.expr_contains_placeholder(expr) {
                return Err(ParseError("invalid use of pipe placeholder".to_string()));
            }
            // gram.y's checkForPipeBind: any `=>` call that survived the
            // pipe rewrite (i.e. `=>` anywhere but a pipe's direct RHS)
            // is an invalid pipe bind. Armed only when the lexer saw `=>`.
            if self.have_pipebind && expr_contains_pipebind(expr) {
                return Err(self.pipebind_position_error("invalid use of pipe bind symbol"));
            }
            exprs.push(expr);
            self.skip_terminators();
        }

        Ok(exprs)
    }

    pub fn parse_program(&mut self) -> Result<SEXP, ParseError> {
        let mut exprs = self.parse_top_level_expressions()?;
        if exprs.is_empty() {
            return unsafe { Ok(R_NilValue()) };
        }
        if exprs.len() == 1 {
            return Ok(exprs
                .into_iter()
                .next()
                .unwrap_or_else(|| unsafe { R_NilValue() }));
        }

        // Multiple expressions → wrap in { }
        unsafe {
            let brace_sym = Rf_install(c"{".as_ptr());
            let nil = R_NilValue();
            let mut list = self.cons(exprs.pop().unwrap_or(nil), nil);
            while let Some(e) = exprs.pop() {
                let cell = self.cons(e, list);
                list = cell;
            }
            let call = self.cons(brace_sym, list);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            Ok(call)
        }
    }

    // -----------------------------------------------------------------------
    // Expression hierarchy (precedence climbing)
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Result<SEXP, ParseError> {
        self.parse_assignment()
    }

    /// assignment: tilde (('=' | '<-' | '->' | '<<-') assignment)?
    fn parse_assignment(&mut self) -> Result<SEXP, ParseError> {
        let left = self.parse_tilde()?;

        match self.peek() {
            Token::LeftAssign | Token::Assign | Token::LeftSuper => {
                let op = self.advance().clone();
                self.skip_newlines();
                let right = self.parse_assignment()?;
                unsafe {
                    let op_sym = match op {
                        Token::Assign => Rf_install(c"=".as_ptr()),
                        Token::LeftSuper => Rf_install(c"<<-".as_ptr()),
                        _ => Rf_install(c"<-".as_ptr()),
                    };
                    Ok(self.lang3(op_sym, left, right))
                }
            }
            Token::RightAssign => {
                // Chained right assignment: `1 -> x -> y` assigns both x and
                // y, folding left-associatively into nested `<-` calls.
                let mut value = left;
                while self.peek() == &Token::RightAssign {
                    self.advance();
                    self.skip_newlines();
                    let target = self.parse_tilde()?;
                    // x -> y is equivalent to y <- x
                    unsafe {
                        let op_sym = Rf_install(c"<-".as_ptr());
                        value = self.lang3(op_sym, target, value);
                    }
                }
                Ok(value)
            }
            _ => Ok(left),
        }
    }

    /// tilde: or ('~' or)*
    fn parse_tilde(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Tilde {
            self.advance();
            self.skip_newlines();
            let right = self.parse_or()?;
            unsafe {
                let op = Rf_install(c"~".as_ptr());
                return Ok(self.lang2(op, right));
            }
        }

        let mut left = self.parse_or()?;
        loop {
            if self.peek() == &Token::Tilde {
                self.advance();
                self.skip_newlines();
                let right = self.parse_or()?;
                unsafe {
                    let op = Rf_install(c"~".as_ptr());
                    left = self.lang3(op, left, right);
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_and()?;
        loop {
            match self.peek() {
                Token::Or2 => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(c"||".as_ptr());
                        left = self.lang3(op, left, right);
                    }
                }
                Token::Or => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(c"|".as_ptr());
                        left = self.lang3(op, left, right);
                    }
                }
                _ => return Ok(left),
            }
        }
    }

    fn parse_and(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_not()?;
        loop {
            match self.peek() {
                Token::And2 => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(c"&&".as_ptr());
                        left = self.lang3(op, left, right);
                    }
                }
                Token::And => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(c"&".as_ptr());
                        left = self.lang3(op, left, right);
                    }
                }
                _ => return Ok(left),
            }
        }
    }

    fn parse_not(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Not {
            self.advance();
            let operand = self.parse_comparison()?;
            unsafe {
                let op = Rf_install(c"!".as_ptr());
                Ok(self.lang2(op, operand))
            }
        } else {
            self.parse_comparison()
        }
    }

    /// Comparison operators. gram.y declares these %nonassoc, so after one
    /// comparison a second operator at this level is a syntax error:
    /// `1 < 2 < 3` fails to parse while `(1 < 2) < 3` is fine.
    fn parse_comparison(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_addition()?;
        let op_name = match self.peek() {
            Token::Lt => "<",
            Token::Gt => ">",
            Token::Le => "<=",
            Token::Ge => ">=",
            Token::Eq => "==",
            Token::Ne => "!=",
            _ => return Ok(left),
        };
        self.advance();
        self.skip_newlines();
        let right = self.parse_addition()?;
        let op = self.install_symbol(op_name)?;
        left = self.lang3(op, left, right);
        if matches!(
            self.peek(),
            Token::Lt | Token::Gt | Token::Le | Token::Ge | Token::Eq | Token::Ne
        ) {
            // Non-associative chained comparison; the offending token is
            // the second comparison operator at the current position.
            return Err(self.unexpected_at(self.pos));
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op_name = match self.peek() {
                Token::Plus => "+",
                Token::Minus => "-",
                _ => return Ok(left),
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_multiplication()?;
            let op = self.install_symbol(op_name)?;
            left = self.lang3(op, left, right);
        }
    }

    fn parse_multiplication(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_special()?;
        loop {
            let op_name = match self.peek() {
                Token::Star => "*".to_string(),
                Token::Slash => "/".to_string(),
                _ => return Ok(left),
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_special()?;
            let op = self.install_symbol(&op_name)?;
            left = self.lang3(op, left, right);
        }
    }
    /// Percent-delimited special operators (`%%`, `%/%`, `%in%`, `%*%`,
    /// user-defined `%foo%`) and the native pipe `|>`. gram.y declares
    /// SPECIAL and PIPE at the same %left tier — tighter than `*` / `/`
    /// and `+` / `-`, looser than `:` — so `2 |> f() * 10` parses as
    /// `(2 |> f()) * 10` and `x |> f() |> g()` as `g(f(x))`, while
    /// `1:3 %% 2` still groups the colon first.
    fn parse_special(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_pipebind()?;
        loop {
            match self.peek() {
                Token::Percent(name) => {
                    let op_name = name.clone();
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_pipebind()?;
                    let op = self.install_symbol(&op_name)?;
                    left = self.lang3(op, left, right);
                }
                Token::Pipe => {
                    self.advance();
                    self.skip_newlines();
                    let rhs_start = self.pos;
                    let right = self.parse_pipebind()?;
                    left = self.build_pipe(left, right, rhs_start)?;
                }
                _ => return Ok(left),
            }
        }
    }

    /// The `=>` pipe-bind operator: gram.y's `expr PIPEBIND expr`, a
    /// %left tier between SPECIAL/PIPE and `:` — tighter than `|>`, so
    /// `x |> y => log(y)` groups as `x |> (y => log(y))`, and looser than
    /// `+`, so the bind's RHS stops at the first additive operator.
    /// Left-associative: `a => b => c` nests leftward.
    fn parse_pipebind(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_colon()?;
        while self.peek() == &Token::PipeBind {
            let op_index = self.pos;
            self.advance();
            self.skip_newlines();
            let right = self.parse_colon()?;
            left = self.build_pipebind(left, right, op_index)?;
        }
        Ok(left)
    }

    /// Build the call for `lhs => rhs`, porting gram.y's `xxpipebind()`:
    /// a plain `=>(lhs, rhs)` binary call, gated by the
    /// `_R_USE_PIPEBIND_` envvar. Only the `|>` rewrite in `build_pipe`
    /// ever consumes such a call; anything left over fails the post-parse
    /// "invalid use of pipe bind symbol" scan.
    fn build_pipebind(
        &mut self,
        lhs: SEXP,
        rhs: SEXP,
        op_index: usize,
    ) -> Result<SEXP, ParseError> {
        if !pipebind_enabled() {
            // Reported at the `=>` token (gram.y passes &@2). Upstream's
            // location points one past the token's first character.
            return Err(self.token_position_error(
                op_index,
                "'=>' is disabled; set '_R_USE_PIPEBIND_' envvar to a true value to enable it",
            ));
        }
        let op = self.install_symbol("=>")?;
        Ok(self.lang3(op, lhs, rhs))
    }

    /// Build the call for `lhs |> rhs`, porting gram.y's `xxpipe()`.
    ///
    /// The RHS must be a function call; a bare symbol is rejected. The LHS
    /// is inserted as the first argument unless a `_` placeholder names the
    /// insertion point:
    /// - a top-level argument `tag = _` is replaced in place — the
    ///   placeholder must be named and may only appear once, and
    /// - an extractor chain like `_[i]`, `_$col`, `_[[i]]`, `_@slot` has
    ///   its placeholder head replaced (R 4.5+).
    ///
    /// A pipe-bind RHS (`x |> y => expr`) rewrites instead into
    /// `(function(y) expr)(x)`; `rhs_start` is the token index where the
    /// pipe's RHS began, for the "RHS variable must be a symbol" location.
    fn build_pipe(&mut self, lhs: SEXP, rhs: SEXP, rhs_start: usize) -> Result<SEXP, ParseError> {
        if unsafe { TYPEOF(rhs) } != SEXPTYPE::LANGSXP {
            return Err(ParseError(
                "The pipe operator requires a function call as RHS".to_string(),
            ));
        }

        // xxpipe's pipe-bind branch, checked before everything else: rewrite
        // `lhs |> var => expr` into `(function(var) expr)(lhs)`.
        let pipebind_sym = self.install_symbol("=>")?;
        if unsafe { CAR(rhs) } == pipebind_sym {
            let var = unsafe { CADR(rhs) };
            let body = unsafe { crate::sexp::accessors::CADDR(rhs) };
            if var.is_null() || unsafe { TYPEOF(var) } != SEXPTYPE::SYMSXP {
                return Err(self.token_position_error(rhs_start, "RHS variable must be a symbol"));
            }
            let nil = unsafe { R_NilValue() };
            // alist = list1(R_MissingArg); SET_TAG(alist, var)
            let formal_cell = self.cons(unsafe { crate::sexp::globals::R_MissingArg() }, nil);
            unsafe {
                crate::sexp::accessors::SETTAG(formal_cell, var);
            }
            // fun = lang4(function, alist, expr, R_NilValue)
            let body_cell = self.cons(body, nil);
            let formals_cell = self.cons(formal_cell, body_cell);
            let fun_sym = self.install_symbol("function")?;
            let fun = self.cons(fun_sym, formals_cell);
            if !fun.is_null() {
                unsafe {
                    (*fun).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
            }
            // lang2(fun, lhs): call the fresh closure on the pipe LHS.
            return Ok(self.lang2(fun, lhs));
        }

        let placeholder = self.placeholder;
        unsafe {
            let fun = CAR(rhs);

            // A placeholder in the function position is never substitutable.
            if self.expr_contains_placeholder(fun) {
                return Err(ParseError(
                    "pipe placeholder cannot be used in the RHS function".to_string(),
                ));
            }

            // Extractor chains: `x |> _[2]`, `d |> _$col`, ...
            if let Some(phcell) = self.find_extractor_placeholder_cell(rhs) {
                let nil = R_NilValue();
                let mut rest = CDR(CDR(rhs));
                while rest != nil {
                    if self.expr_contains_placeholder(CAR(rest)) {
                        return Err(ParseError(
                            "pipe placeholder may only appear once".to_string(),
                        ));
                    }
                    rest = CDR(rest);
                }
                SETCAR(phcell, lhs);
                return Ok(rhs);
            }

            // A top-level placeholder argument marks the insertion point.
            let nil = R_NilValue();
            let mut cell = CDR(rhs);
            while cell != nil {
                if CAR(cell) == placeholder {
                    let tag = TAG(cell);
                    if tag.is_null() || tag == nil {
                        return Err(ParseError(
                            "pipe placeholder can only be used as a named argument".to_string(),
                        ));
                    }
                    let mut rest = CDR(cell);
                    while rest != nil {
                        if CAR(rest) == placeholder {
                            return Err(ParseError(
                                "pipe placeholder may only appear once".to_string(),
                            ));
                        }
                        rest = CDR(rest);
                    }
                    SETCAR(cell, lhs);
                    return Ok(rhs);
                }
                cell = CDR(cell);
            }

            // Screen out syntactically special functions like `/` or `$`.
            if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                let name = symbol_name_string(fun);
                if is_special_rhs_function(&name) {
                    return Err(ParseError(format!(
                        "function '{name}' not supported in RHS call of a pipe"
                    )));
                }
            }

            // Default: prepend the LHS as the first argument.
            let args = self.cons(lhs, CDR(rhs));
            let call = self.cons(fun, args);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            Ok(call)
        }
    }

    /// Find the argument cell whose CAR is the placeholder at the head of
    /// an extractor chain (`[`, `[[`, `$`, `@`), mirroring gram.y's
    /// `findExtractorChainPHCell()`. Returns the cell to overwrite with
    /// the pipe LHS.
    fn find_extractor_placeholder_cell(&self, expr: SEXP) -> Option<SEXP> {
        unsafe {
            let fun = CAR(expr);
            let bracket = Rf_install(c"[".as_ptr());
            let bracket2 = Rf_install(c"[[".as_ptr());
            let dollar = Rf_install(c"$".as_ptr());
            let at = Rf_install(c"@".as_ptr());
            if fun != bracket && fun != bracket2 && fun != dollar && fun != at {
                return None;
            }
            let arg1 = CADR(expr);
            if arg1 == self.placeholder {
                Some(CDR(expr))
            } else if TYPEOF(arg1) == SEXPTYPE::LANGSXP {
                self.find_extractor_placeholder_cell(arg1)
            } else {
                None
            }
        }
    }

    /// Whether `expr` contains the pipe placeholder node anywhere,
    /// mirroring gram.y's `checkForPlaceholder()`. Used for the RHS
    /// function slot and for the final per-expression scan.
    fn expr_contains_placeholder(&self, expr: SEXP) -> bool {
        if expr.is_null() || self.placeholder.is_null() {
            return false;
        }
        if expr == self.placeholder {
            return true;
        }
        unsafe {
            let t = TYPEOF(expr);
            if t != SEXPTYPE::LANGSXP && t != SEXPTYPE::LISTSXP {
                return false;
            }
            let nil = R_NilValue();
            let mut cur = expr;
            while cur != nil {
                if self.expr_contains_placeholder(CAR(cur)) {
                    return true;
                }
                cur = CDR(cur);
            }
            false
        }
    }

    /// Power operator: tightest binary operator, right-associative, and
    /// tighter than unary sign, so `2^3^2 == 2^(3^2)` and the exponent may
    /// carry a unary sign (`2^-1`).
    fn parse_power(&mut self) -> Result<SEXP, ParseError> {
        let base = self.parse_postfix()?;
        if self.peek() == &Token::Caret {
            self.advance();
            self.skip_newlines();
            let exp = self.parse_unary()?;
            unsafe {
                let op = Rf_install(c"^".as_ptr());
                Ok(self.lang3(op, base, exp))
            }
        } else {
            Ok(base)
        }
    }

    /// Colon operator: x:y (used for sequences like 1:10). Looser than unary
    /// sign but tighter than `*` / `/`, so `-2:3 == (-2):3` while
    /// `2*1:3 == 2*(1:3)`.
    fn parse_colon(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            if self.peek() == &Token::Colon {
                self.advance();
                self.skip_newlines();
                let right = self.parse_unary()?;
                unsafe {
                    let op = Rf_install(c":".as_ptr());
                    left = self.lang3(op, left, right);
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<SEXP, ParseError> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                unsafe {
                    let op = Rf_install(c"-".as_ptr());
                    Ok(self.lang2(op, operand))
                }
            }
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            Token::Not => {
                self.advance();
                let operand = self.parse_unary()?;
                unsafe {
                    let op = Rf_install(c"!".as_ptr());
                    Ok(self.lang2(op, operand))
                }
            }
            _ => self.parse_power(),
        }
    }

    // -----------------------------------------------------------------------
    // Postfix operations (left-associative)
    // -----------------------------------------------------------------------

    /// Parse postfix operations: function calls, subscript, member access.
    /// These are all left-associative and chain together.
    fn parse_postfix(&mut self) -> Result<SEXP, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                // Function call: f(args)
                Token::LParen => {
                    self.advance();
                    let args = self.parse_arglist()?;
                    self.expect(&Token::RParen)?;

                    unsafe {
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in args.into_iter().rev() {
                            let cell = self.cons(val, arg_list);
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        let call = self.cons(expr, arg_list);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                // Subscript: x[i], x[i, j]; empty slots (leading/trailing/
                // doubled commas) become missing arguments, like gram.y xxsub0.
                Token::LBracket => {
                    self.advance();
                    let slots = self.parse_subscript_slots(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;

                    unsafe {
                        let bracket_sym = Rf_install(c"[".as_ptr());
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list);
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list);
                        let call = self.cons(bracket_sym, arg_list);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                // Double subscript: x[[i]], x[[i, j]] — multiple comma-
                // separated slots allowed, empty slots become missing args.
                Token::LDoubleBracket => {
                    self.advance();
                    let slots = self.parse_subscript_slots(&Token::RDoubleBracket)?;
                    self.expect(&Token::RDoubleBracket)?;

                    unsafe {
                        let dbracket_sym = Rf_install(c"[[".as_ptr());
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list);
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list);
                        let call = self.cons(dbracket_sym, arg_list);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                // Dollar: x$name
                Token::Dollar => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let dollar_sym = Rf_install(c"$".as_ptr());
                        let nil = R_NilValue();
                        let name_cell = self.cons(name, nil);
                        let args = self.cons(expr, name_cell);
                        let call = self.cons(dollar_sym, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                // Namespace lookup: pkg::name or pkg:::name
                Token::DoubleColon | Token::TripleColon => {
                    let op_token = self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let op = match op_token {
                            Token::TripleColon => Rf_install(c":::".as_ptr()),
                            _ => Rf_install(c"::".as_ptr()),
                        };
                        expr = self.lang3(op, expr, name);
                    }
                }
                // At: x@name
                Token::At => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let at_sym = Rf_install(c"@".as_ptr());
                        let nil = R_NilValue();
                        let name_cell = self.cons(name, nil);
                        let args = self.cons(expr, name_cell);
                        let call = self.cons(at_sym, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                _ => return Ok(expr),
            }
        }
    }

    /// Parse a member name after $ or @ — can be identifier or backtick name.
    fn parse_member_name(&mut self) -> Result<SEXP, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                self.install_symbol(&name)
            }
            Token::Str(name) => {
                // Allow "name" after $ for compatibility
                let name = name.clone();
                self.advance();
                self.install_symbol(&name)
            }
            _ => Err(self.unexpected_at(self.pos)),
        }
    }

    /// Parse the slot list of `[` / `[[` subscripts: comma-separated
    /// expressions where empty slots (leading/trailing/doubled commas, or
    /// nothing before the closing bracket) become R_MissingArg, mirroring
    /// gram.y's xxsub0. A `name = value` slot is tagged like a call argument
    /// (subset.rs ExtractDropArg relies on tags for `drop=` / `exact=`).
    fn parse_subscript_slots(
        &mut self,
        close: &Token,
    ) -> Result<Vec<(Option<String>, SEXP)>, ParseError> {
        let mut slots = Vec::new();
        self.skip_newlines();
        if self.peek() == close {
            return Ok(slots);
        }
        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                // Empty slot before a comma
                slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                self.advance();
                self.skip_newlines();
                if self.peek() == close {
                    // Trailing comma: empty final slot
                    slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
                continue;
            }
            if self.peek() == close {
                break;
            }
            let (name, val) = self.parse_arg()?;
            slots.push((name, val));
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek() == close {
                    slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
            } else {
                break;
            }
        }
        Ok(slots)
    }

    // -----------------------------------------------------------------------
    // Primary expressions (atoms and control structures)
    // -----------------------------------------------------------------------

    fn parse_primary(&mut self) -> Result<SEXP, ParseError> {
        match self.peek().clone() {
            // Keywords
            Token::KwIf => self.parse_if(),
            Token::KwFor => self.parse_for(),
            Token::KwWhile => self.parse_while(),
            Token::KwRepeat => self.parse_repeat(),
            Token::KwFunction | Token::KwLambda => self.parse_function(),
            Token::KwBreak => {
                self.advance();
                unsafe {
                    let sym = Rf_install(c"break".as_ptr());
                    Ok(self.lang2(sym, R_NilValue()))
                }
            }
            Token::KwNext => {
                self.advance();
                unsafe {
                    let sym = Rf_install(c"next".as_ptr());
                    Ok(self.lang2(sym, R_NilValue()))
                }
            }
            Token::KwReturn => {
                self.advance();
                let val = if self.peek() == &Token::LParen {
                    self.advance();
                    let e = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    e
                } else {
                    // return without parens — return next expression
                    // In R, `return expr` is valid without parens
                    self.parse_expr()?
                };
                unsafe {
                    let sym = Rf_install(c"return".as_ptr());
                    Ok(self.lang2(sym, val))
                }
            }
            // Block: { expr; expr; ... }
            Token::LBrace => self.parse_block(),
            // Parenthesized expr or tuple
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    // Empty parens → NULL
                    self.advance();
                    return unsafe { Ok(R_NilValue()) };
                }
                let expr = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            // Anything else → atom
            _ => self.parse_atom(),
        }
    }

    // -----------------------------------------------------------------------
    // Control structures
    // -----------------------------------------------------------------------

    /// if (cond) expr [else expr]
    fn parse_if(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'if'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        // Skip newlines after )
        self.skip_newlines();
        let body = self.parse_expr()?;

        // Check for else
        self.skip_newlines();
        if self.peek() == &Token::KwElse {
            self.advance();
            self.skip_newlines();
            let alt = self.parse_expr()?;
            unsafe {
                let if_sym = Rf_install(c"if".as_ptr());
                let nil = R_NilValue();
                let alt_cell = self.cons(alt, nil);
                let body_cell = self.cons(body, alt_cell);
                let cond_cell = self.cons(cond, body_cell);
                let call = self.cons(if_sym, cond_cell);
                if !call.is_null() {
                    (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                Ok(call)
            }
        } else {
            unsafe {
                let if_sym = Rf_install(c"if".as_ptr());
                Ok(self.lang3(if_sym, cond, body))
            }
        }
    }

    /// for (var in seq) body
    fn parse_for(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'for'
        self.expect(&Token::LParen)?;

        // var must be an identifier
        let var_name = match self.advance() {
            Token::Ident(name) => name,
            _ => return Err(self.unexpected_at(self.pos - 1)),
        };
        let var = self.install_symbol(&var_name)?;

        self.expect(&Token::KwIn)?;
        let seq = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let for_sym = Rf_install(c"for".as_ptr());
            let nil = R_NilValue();
            let body_cell = self.cons(body, nil);
            let seq_cell = self.cons(seq, body_cell);
            let var_cell = self.cons(var, seq_cell);
            let call = self.cons(for_sym, var_cell);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            Ok(call)
        }
    }

    /// while (cond) body
    fn parse_while(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'while'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let while_sym = Rf_install(c"while".as_ptr());
            Ok(self.lang3(while_sym, cond, body))
        }
    }

    /// repeat body
    fn parse_repeat(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'repeat'
        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let repeat_sym = Rf_install(c"repeat".as_ptr());
            Ok(self.lang2(repeat_sym, body))
        }
    }

    /// function(args) body — also handles R 4.1+ `\(...)` lambda syntax.
    fn parse_function(&mut self) -> Result<SEXP, ParseError> {
        match self.peek() {
            Token::KwFunction | Token::KwLambda => {
                self.advance();
            }
            _ => {}
        }
        self.expect(&Token::LParen)?;
        let formals = self.parse_formals()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let fn_sym = Rf_install(c"function".as_ptr());
            let nil = R_NilValue();
            let body_cell = self.cons(body, nil);
            let formals_cell = self.cons(formals, body_cell);
            let call = self.cons(fn_sym, formals_cell);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            Ok(call)
        }
    }

    /// Parse formal arguments: name [= default], name [= default], ...
    /// Returns a pairlist of (name default) pairs.
    fn parse_formals(&mut self) -> Result<SEXP, ParseError> {
        let nil = unsafe { R_NilValue() };
        let mut pairs: Vec<(String, SEXP)> = Vec::new();

        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(nil);
        }

        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::DotDotDot => {
                    self.advance();
                    pairs.push(("...".to_string(), unsafe { R_NilValue() }));
                }
                Token::Ident(name) => {
                    let name = name.clone();
                    self.advance();
                    let default = if self.peek() == &Token::Assign {
                        self.advance();
                        self.parse_expr()?
                    } else {
                        unsafe { crate::sexp::globals::R_MissingArg() }
                    };
                    pairs.push((name, default));
                }
                _ => return Err(self.unexpected_at(self.pos)),
            }

            self.skip_newlines();
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        // Build pairlist in reverse (R expects first arg first)
        unsafe {
            let mut list = nil;
            for (name, default) in pairs.into_iter().rev() {
                let cell = self.cons(default, list);
                let sym = self.install_symbol(&name)?;
                crate::sexp::accessors::SETTAG(cell, sym);
                list = cell;
            }
            Ok(list)
        }
    }

    // -----------------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------------

    /// { expr; expr; ... }
    fn parse_block(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume '{'
        self.skip_terminators();

        let mut exprs = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            exprs.push(self.parse_expr()?);
            self.skip_terminators();
        }
        self.expect(&Token::RBrace)?;

        if exprs.is_empty() {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                Ok(self.lang2(brace_sym, R_NilValue()))
            }
        } else if exprs.len() == 1 {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                Ok(self.lang2(brace_sym, exprs.pop().unwrap_or_else(|| R_NilValue())))
            }
        } else {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                let nil = R_NilValue();
                let mut list = self.cons(exprs.pop().unwrap_or(nil), nil);
                while let Some(e) = exprs.pop() {
                    list = self.cons(e, list);
                }
                let call = self.cons(brace_sym, list);
                if !call.is_null() {
                    (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                Ok(call)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Atoms
    // -----------------------------------------------------------------------

    fn parse_atom(&mut self) -> Result<SEXP, ParseError> {
        // An operand position never terminates on a newline (gram.l's
        // EatLines): `1 +\n2` continues on the next line and `1 +\n` runs
        // out of input, exactly like upstream.
        self.skip_newlines();
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(self.scalar_real(n))
            }
            Token::Complex(n) => {
                self.advance();
                Ok(self.scalar_complex(n))
            }
            Token::Int(n) => {
                self.advance();
                Ok(self.scalar_integer(n))
            }
            Token::Str(s) => {
                self.advance();
                Ok(self.scalar_string(&s))
            }
            Token::DotDotDot => {
                self.advance();
                unsafe {
                    let sym = Rf_install(c"...".as_ptr());
                    Ok(sym)
                }
            }
            Token::Placeholder => {
                self.advance();
                Ok(self.placeholder_node())
            }
            Token::Ident(ref name) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "TRUE" => Ok(self.scalar_logical(TRUE)),
                    "FALSE" => Ok(self.scalar_logical(FALSE)),
                    "NULL" => unsafe { Ok(R_NilValue()) },
                    "NA" => Ok(self.scalar_logical(NA_LOGICAL)),
                    "Inf" => Ok(self.scalar_real(f64::INFINITY)),
                    "NaN" => Ok(self.scalar_real(f64::NAN)),
                    "NA_real_" => Ok(self.scalar_real(crate::sexp::ffi::NA_REAL)),
                    "NA_integer_" => Ok(self.scalar_integer(crate::sexp::ffi::NA_INTEGER)),
                    "NA_complex_" => Ok(self.scalar_complex_parts(
                        crate::sexp::ffi::NA_REAL,
                        crate::sexp::ffi::NA_REAL,
                    )),
                    "NA_character_" => Ok(self.scalar_na_string()),
                    _ => self.install_symbol(&name),
                }
            }
            _ => Err(self.unexpected_at(self.pos)),
        }
    }

    // -----------------------------------------------------------------------
    // Argument list
    // -----------------------------------------------------------------------

    fn parse_arglist(&mut self) -> Result<Vec<(Option<String>, SEXP)>, ParseError> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(args);
        }

        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
                continue;
            }
            let (name, val) = self.parse_arg()?;
            args.push((name, val));
            self.skip_newlines();

            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_arg(&mut self) -> Result<(Option<String>, SEXP), ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                let saved = self.pos;
                let name = name.clone();
                self.advance();
                match self.peek() {
                    Token::Assign => {
                        self.advance();
                        let val = if self.peek() == &Token::Comma || self.peek() == &Token::RParen {
                            unsafe { crate::sexp::globals::R_MissingArg() }
                        } else {
                            self.parse_expr()?
                        };
                        Ok((Some(name), val))
                    }
                    // Handle `name = expr` where = is assignment
                    _ => {
                        self.pos = saved;
                        let val = self.parse_expr()?;
                        Ok((None, val))
                    }
                }
            }
            Token::DotDotDot => {
                self.advance();
                unsafe {
                    let sym = Rf_install(c"...".as_ptr());
                    Ok((None, sym))
                }
            }
            _ => {
                let val = self.parse_expr()?;
                Ok((None, val))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse R source into an expression tree allocated in `arena`.
///
/// The arena should belong to the active `RSession`: symbols are interned in
/// the active session while expression nodes and literals are allocated in this
/// borrowed arena.
pub fn parse(input: &str, arena: &mut RArena) -> Result<SEXP, ParseError> {
    let mut parser = Parser::new(input, arena);
    parser.parse_program()
}

/// Parse R source into the sequence of top-level expressions it contains.
///
/// This mirrors GNU R's `parse()` result shape: a source stream with multiple
/// complete expressions becomes an `EXPRSXP` with one element per expression,
/// instead of a synthetic `{ ... }` block used by direct evaluation.
pub fn parse_expressions(input: &str, arena: &mut RArena) -> Result<Vec<SEXP>, ParseError> {
    let mut parser = Parser::new(input, arena);
    parser.parse_top_level_expressions()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sexp::accessors::{CADR, CAR, CDR, CHAR, COMPLEX, PRINTNAME, TYPEOF, XLENGTH};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::session::RSession;

    fn parse_str(input: &str) -> Result<SEXP, ParseError> {
        let session = Box::leak(Box::new(RSession::new()));
        session
            .with_arena(|arena| parse(input, arena))
            .unwrap_or_else(|| Err(ParseError("test session is closed".to_string())))
    }

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    unsafe fn call_head_name(call: SEXP) -> String {
        unsafe {
            let printname = PRINTNAME(CAR(call));
            let chars = CHAR(printname);
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned()
        }
    }

    fn generated_parser_input(mut seed: u64, len: usize) -> String {
        const ALPHABET: &[u8] = b"abcxyz0123456789+-*/^<>=!&|(){}[],$@_.'\"`# \n\t;:";
        let mut out = String::with_capacity(len);
        for _ in 0..len {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            out.push(ALPHABET[((seed >> 32) as usize) % ALPHABET.len()] as char);
        }
        out
    }

    fn adversarial_iterations(default: u64) -> u64 {
        std::env::var("RPORT_ADVERSARIAL_ITERS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    #[test]
    fn adversarial_parser_inputs_do_not_panic() {
        let mut session = RSession::new();
        let fixed = [
            ")",
            "(((((((((",
            "\"unterminated",
            "'unterminated\\",
            "`unterminated",
            "x[[[[1]]",
            "function(x,,y) x",
            "if (TRUE) { # comment without close",
            "a <- 1 +\n# comment\n)",
            "repeat { next; break; } }",
        ];

        for input in fixed {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.with_arena(|arena| parse(input, arena))
            }));
            assert!(result.is_ok(), "parser panicked for fixed input: {input:?}");
        }

        for seed in 0..adversarial_iterations(256) {
            let input = generated_parser_input(seed, (seed as usize % 96) + 1);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.with_arena(|arena| parse(&input, arena))
            }));
            assert!(result.is_ok(), "parser panicked for seed {seed}: {input:?}");
        }
    }

    // --- Basic atoms ---

    #[test]
    fn test_integer_literal() {
        unsafe {
            let result = must(parse_str("42"));
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        }
    }

    #[test]
    fn test_real_literal() {
        unsafe {
            let result = must(parse_str("3.14"));
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        }
    }

    #[test]
    fn test_complex_literal() {
        unsafe {
            let result = must(parse_str("2i"));
            assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
            assert_eq!(XLENGTH(result), 1);
            let data = COMPLEX(result);
            assert_eq!((*data).r, 0.0);
            assert_eq!((*data).i, 2.0);
        }
    }

    #[test]
    fn test_na_complex_literal() {
        unsafe {
            let result = must(parse_str("NA_complex_"));
            assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
            assert_eq!(XLENGTH(result), 1);
            let data = COMPLEX(result);
            assert_eq!((*data).r.to_bits(), crate::sexp::ffi::R_NA_BIT_PATTERN);
            assert_eq!((*data).i.to_bits(), crate::sexp::ffi::R_NA_BIT_PATTERN);
        }
    }

    #[test]
    fn test_int_suffix() {
        unsafe {
            let result = must(parse_str("42L"));
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        }
    }

    #[test]
    fn test_string_literal() {
        unsafe {
            let result = must(parse_str("\"hello\""));
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
        }
    }

    #[test]
    fn test_true_false_null() {
        unsafe {
            let t = must(parse_str("TRUE"));
            assert_eq!(TYPEOF(t), SEXPTYPE::LGLSXP);

            let f = must(parse_str("FALSE"));
            assert_eq!(TYPEOF(f), SEXPTYPE::LGLSXP);

            let n = must(parse_str("NULL"));
            assert_eq!(n, R_NilValue());
        }
    }

    #[test]
    fn test_identifier() {
        unsafe {
            let result = must(parse_str("x"));
            assert_eq!(TYPEOF(result), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_na_inf_nan() {
        unsafe {
            let na = must(parse_str("NA"));
            assert_eq!(TYPEOF(na), SEXPTYPE::LGLSXP);

            let inf = must(parse_str("Inf"));
            assert_eq!(TYPEOF(inf), SEXPTYPE::REALSXP);

            let nan = must(parse_str("NaN"));
            assert_eq!(TYPEOF(nan), SEXPTYPE::REALSXP);
        }
    }

    #[test]
    fn test_na_character() {
        unsafe {
            let na_chr = must(parse_str("NA_character_"));
            assert_eq!(TYPEOF(na_chr), SEXPTYPE::STRSXP);
            assert_eq!(crate::sexp::accessors::LENGTH(na_chr), 1);
            let elt = crate::sexp::accessors::STRING_ELT(na_chr, 0);
            assert!(!elt.is_null());
            assert_eq!(
                crate::sexp::accessors::TYPEOF(elt),
                SEXPTYPE::CHARSXP.as_c_int()
            );
            assert_eq!((*elt).sxpinfo.gp(), 1);
        }
    }

    // --- Binary operators ---

    #[test]
    fn test_addition() {
        unsafe {
            let result = must(parse_str("1 + 2"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
            let op = CAR(result);
            assert_eq!(TYPEOF(op), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_chained_addition() {
        unsafe {
            let result = must(parse_str("1 + 2 + 3"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_operator_precedence() {
        unsafe {
            let result = must(parse_str("2 + 3 * 4"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_power() {
        unsafe {
            let result = must(parse_str("2^3"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_integer_div() {
        unsafe {
            let result = must(parse_str("5 %/% 2"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_modulus() {
        unsafe {
            let result = must(parse_str("5 %% 2"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- Unary ---

    #[test]
    fn test_unary_minus() {
        unsafe {
            let result = must(parse_str("-5"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- Assignment ---

    #[test]
    fn test_assignment() {
        unsafe {
            let result = must(parse_str("x <- 42"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_equals_assign() {
        unsafe {
            let result = must(parse_str("x = 42"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
            assert_eq!(call_head_name(result), "=");
        }
    }

    #[test]
    fn test_right_assign() {
        unsafe {
            let result = must(parse_str("42 -> x"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_super_assignment_preserves_operator() {
        unsafe {
            let result = must(parse_str("x <<- 42"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
            assert_eq!(call_head_name(result), "<<-");
        }
    }

    // --- Function calls ---

    #[test]
    fn test_function_call() {
        unsafe {
            let result = must(parse_str("f(x, y)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
            let fun = CAR(result);
            assert_eq!(TYPEOF(fun), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_subscript_argument_order() {
        unsafe {
            let result = must(parse_str("x[2]"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
            let args = CDR(result);
            assert_eq!(TYPEOF(CAR(args)), SEXPTYPE::SYMSXP);
            assert_eq!(TYPEOF(CADR(args)), SEXPTYPE::REALSXP);
        }
    }

    #[test]
    fn test_named_arg() {
        unsafe {
            let result = must(parse_str("f(x = 1)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_parenthesized() {
        unsafe {
            let result = must(parse_str("(1 + 2)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- Logical ---

    #[test]
    fn test_comparison() {
        unsafe {
            let result = must(parse_str("x < 10"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_logical_ops() {
        unsafe {
            let and = must(parse_str("x && y"));
            assert_eq!(TYPEOF(and), SEXPTYPE::LANGSXP);

            let or = must(parse_str("x || y"));
            assert_eq!(TYPEOF(or), SEXPTYPE::LANGSXP);

            let not = must(parse_str("!x"));
            assert_eq!(TYPEOF(not), SEXPTYPE::LANGSXP);
        }
    }

    // --- Programs ---

    #[test]
    fn test_multi_expr() {
        unsafe {
            let result = must(parse_str("x <- 1; y <- 2"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_comment_preserves_newline_terminator() {
        unsafe {
            let result = must(parse_str("x <- 1 # comment\ny <- 2\nx + y"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_newline_after_infix_continues_expression() {
        unsafe {
            let result = must(parse_str("x <- 1 +\n 2 *\n 3"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_empty_input() {
        unsafe {
            let result = must(parse_str(""));
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_complex_expr() {
        unsafe {
            let result = must(parse_str("sqrt(x^2 + y^2)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_error_unexpected_rparen() {
        assert!(parse_str(")").is_err());
    }

    // --- NEW: Control flow ---

    #[test]
    fn test_if() {
        unsafe {
            let result = must(parse_str("if (x > 0) x"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_if_else() {
        unsafe {
            let result = must(parse_str("if (x > 0) x else -x"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_for_loop() {
        unsafe {
            let result = must(parse_str("for (i in 1:10) print(i)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_while_loop() {
        unsafe {
            let result = must(parse_str("while (x > 0) x <- x - 1"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_repeat_loop() {
        unsafe {
            let result = must(parse_str("repeat { x <- x + 1 }"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_break_next() {
        unsafe {
            let brk = must(parse_str("break"));
            assert_eq!(TYPEOF(brk), SEXPTYPE::LANGSXP);

            let nxt = must(parse_str("next"));
            assert_eq!(TYPEOF(nxt), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Blocks ---

    #[test]
    fn test_block() {
        unsafe {
            let result = must(parse_str("{ a <- 1; b <- 2; a + b }"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_empty_block() {
        unsafe {
            let result = must(parse_str("{}"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Function definitions ---

    #[test]
    fn test_function_def() {
        unsafe {
            let result = must(parse_str("function(x) x^2"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_function_multi_args() {
        unsafe {
            let result = must(parse_str("function(x, y = 1) x + y"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_function_no_args() {
        unsafe {
            let result = must(parse_str("function() 42"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Subscript ---

    #[test]
    fn test_subscript() {
        unsafe {
            let result = must(parse_str("x[1]"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_subscript_multi() {
        unsafe {
            let result = must(parse_str("x[i, j]"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_double_subscript() {
        unsafe {
            let result = must(parse_str("x[[1]]"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Member access ---

    #[test]
    fn test_dollar_access() {
        unsafe {
            let result = must(parse_str("df$col"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_at_access() {
        unsafe {
            let result = must(parse_str("obj@slot"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Custom infix ---

    #[test]
    fn test_custom_infix() {
        unsafe {
            let result = must(parse_str("x %in% y"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_matrix_multiply() {
        unsafe {
            let result = must(parse_str("A %*% B"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Backtick names ---

    #[test]
    fn test_backtick_name() {
        unsafe {
            let result = must(parse_str("`weird name`"));
            assert_eq!(TYPEOF(result), SEXPTYPE::SYMSXP);
        }
    }

    // --- NEW: Formula ---

    #[test]
    fn test_formula() {
        unsafe {
            let result = must(parse_str("y ~ x + z"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: DotDotDot ---

    #[test]
    fn test_varargs() {
        unsafe {
            let result = must(parse_str("f(...)"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    // --- NEW: Chained postfix ---

    #[test]
    fn test_chained_dollar_subscript() {
        unsafe {
            let result = must(parse_str("df$col[1]"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_function_in_block() {
        unsafe {
            let result = must(parse_str("{ f <- function(x) x^2; f(3) }"));
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        }
    }
}
