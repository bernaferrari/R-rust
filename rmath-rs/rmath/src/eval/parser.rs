//! Full R expression parser.
//!
//! Recursive-descent parser that converts R source code into SEXP trees
//! suitable for evaluation by `eval_safe()`.
//!
//! Supports:
//! - Numbers (integer and real), strings, identifiers
//! - All R keywords: TRUE, FALSE, NULL, NA, Inf, NaN, NA_real_, NA_integer_, NA_character_
//! - Binary operators: +, -, *, /, ^, <, >, <=, >=, ==, !=, &, &&, |, ||, %%, %/%
//! - Custom infix operators: %in%, %o%, %*%, any %xxx% sequence
//! - Unary minus/plus
//! - Assignment: <-, =, ->, <<-
//! - Function calls: f(x, y), f(x = 1)
//! - Parenthesized expressions: (expr)
//! - Blocks: { expr; expr; ... }
//! - Control flow: if/else, for, while, repeat, break, next, return
//! - Function definitions: function(args) body
//! - Subscript: x[i], x[i, j], x[[i]]
//! - Member access: x$name, x@slot
//! - Formula: y ~ x
//! - Backtick names: `weird name`
//! - ... varargs

use std::ffi::CString;

use crate::sexp::builder::{
    scalar_integer_in, scalar_logical_in, scalar_real_in, scalar_string_in,
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
    KwBreak,
    KwNext,
    KwReturn,
    // Eof
    Eof,
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos: 0,
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

        let ch = match self.peek_char() {
            Some(c) => c,
            None => return Token::Eof,
        };

        if ch == '\n' {
            self.advance();
            return Token::Newline;
        }

        if ch == '"' || ch == '\'' {
            return self.read_string();
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
            ':' => Token::Colon,
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

        // Handle hex literals: 0x...
        if self.peek_char() == Some('0')
            && (self.peek_char_at(1) == Some('x') || self.peek_char_at(1) == Some('X'))
        {
            s.push(self.advance().unwrap()); // 0
            s.push(self.advance().unwrap()); // x
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_hexdigit() {
                    s.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            if let Ok(v) = i64::from_str_radix(&s[2..], 16) {
                return Token::Int(v as i32);
            }
            return Token::Number(0.0);
        }

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
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
            } else if ch == 'L' {
                self.advance();
                if let Ok(v) = s.parse::<i32>() {
                    return Token::Int(v);
                }
                break;
            } else {
                break;
            }
        }

        let v: f64 = s.parse().unwrap_or(0.0);
        if !has_dot && s.parse::<i32>().is_ok() {
            let int_val: i32 = s.parse().unwrap_or(0);
            Token::Number(int_val as f64)
        } else {
            Token::Number(v)
        }
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
        write!(f, "parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct Parser<'arena> {
    tokens: Vec<Token>,
    pos: usize,
    arena: &'arena mut RArena,
}

impl<'arena> Parser<'arena> {
    pub fn new(input: &str, arena: &'arena mut RArena) -> Self {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            let is_eof = tok == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Parser {
            tokens,
            pos: 0,
            arena,
        }
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
            Err(ParseError(format!(
                "expected {:?}, got {:?}",
                expected, tok
            )))
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

    pub fn parse_program(&mut self) -> Result<SEXP, ParseError> {
        let mut exprs = Vec::new();
        loop {
            self.skip_terminators();
            if self.peek() == &Token::Eof {
                break;
            }
            exprs.push(self.parse_expr()?);
            self.skip_terminators();
        }

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
                let _op = self.advance();
                self.skip_newlines();
                let right = self.parse_assignment()?;
                // x -> y is equivalent to y <- x
                unsafe {
                    let op_sym = Rf_install(c"<-".as_ptr());
                    Ok(self.lang3(op_sym, right, left))
                }
            }
            _ => Ok(left),
        }
    }

    /// tilde: or ('~' or)*
    fn parse_tilde(&mut self) -> Result<SEXP, ParseError> {
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

    fn parse_comparison(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_addition()?;
        loop {
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
        }
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
        let mut left = self.parse_power()?;
        loop {
            let op_name = match self.peek() {
                Token::Star => "*".to_string(),
                Token::Slash => "/".to_string(),
                Token::Percent(name) => name.clone(),
                _ => return Ok(left),
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_power()?;
            let op = self.install_symbol(&op_name)?;
            left = self.lang3(op, left, right);
        }
    }

    fn parse_power(&mut self) -> Result<SEXP, ParseError> {
        let base = self.parse_colon()?;
        if self.peek() == &Token::Caret {
            self.advance();
            self.skip_newlines();
            let exp = self.parse_colon()?;
            unsafe {
                let op = Rf_install(c"^".as_ptr());
                Ok(self.lang3(op, base, exp))
            }
        } else {
            Ok(base)
        }
    }

    /// Colon operator: x:y (used for sequences like 1:10)
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
            _ => self.parse_postfix(),
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
                // Subscript: x[i] or x[i, j]
                Token::LBracket => {
                    self.advance();
                    let mut indices = Vec::new();
                    self.skip_newlines();
                    if self.peek() != &Token::RBracket {
                        loop {
                            self.skip_newlines();
                            if self.peek() == &Token::RBracket {
                                break; // trailing comma
                            }
                            indices.push(self.parse_expr()?);
                            self.skip_newlines();
                            if self.peek() == &Token::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RBracket)?;

                    unsafe {
                        let bracket_sym = Rf_install(c"[".as_ptr());
                        let nil = R_NilValue();
                        let mut args = nil;
                        for idx in indices.into_iter().rev() {
                            args = self.cons(idx, args);
                        }
                        args = self.cons(expr, args);
                        let call = self.cons(bracket_sym, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        expr = call;
                    }
                }
                // Double subscript: x[[i]]
                Token::LDoubleBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.skip_newlines();
                    self.expect(&Token::RDoubleBracket)?;

                    unsafe {
                        let dbracket_sym = Rf_install(c"[[".as_ptr());
                        let nil = R_NilValue();
                        let idx_cell = self.cons(idx, nil);
                        let args = self.cons(expr, idx_cell);
                        let call = self.cons(dbracket_sym, args);
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
            _ => Err(ParseError(format!(
                "expected name after $ or @, got {:?}",
                self.peek()
            ))),
        }
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
            Token::KwFunction => self.parse_function(),
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
            tok => {
                return Err(ParseError(format!(
                    "expected identifier in for, got {:?}",
                    tok
                )));
            }
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

    /// function(args) body
    fn parse_function(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'function'
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
                tok => return Err(ParseError(format!("expected formal arg, got {:?}", tok))),
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
                Ok(self.lang2(brace_sym, exprs.into_iter().next().unwrap()))
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
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(self.scalar_real(n))
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
                    "NA_character_" => Ok(self.scalar_na_string()),
                    _ => self.install_symbol(&name),
                }
            }
            ref tok => Err(ParseError(format!("unexpected token: {:?}", tok))),
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
            let (name, val) = self.parse_arg()?;
            args.push((name, val));
            self.skip_newlines();

            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                // Allow trailing comma
                if self.peek() == &Token::RParen {
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
                        let val = self.parse_expr()?;
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sexp::accessors::{CADR, CAR, CDR, CHAR, PRINTNAME, TYPEOF};
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
