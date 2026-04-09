//! Minimal R expression parser.
//!
//! Recursive-descent parser that converts R source code into SEXP trees
//! suitable for evaluation by `eval_safe()`. Supports:
//!
//! - Numbers (integer and real)
//! - Strings (double-quoted)
//! - `TRUE`, `FALSE`, `NULL`, `NA`, `Inf`, `NaN`
//! - Identifiers
//! - Binary operators: `+`, `-`, `*`, `/`, `^`, `<`, `>`, `<=`, `>=`,
//!   `==`, `!=`, `&`, `&&`, `|`, `||`, `%%`, `%/%`
//! - Unary minus/plus
//! - Assignment: `<-`, `=`
//! - Function calls: `f(x, y)`
//! - Parenthesized expressions: `(expr)`
//! - Semicolons and newlines as expression separators

use std::ffi::CString;

use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_cons, Rf_lang2, Rf_lang3, Rf_mkString,
};
use crate::sexp::ffi::{FALSE, SEXP, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::RArena;
use crate::sexp::symbol::Rf_install;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Int(i32),
    Str(String),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent,
    SlashPercent,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    And2,
    Or,
    Or2,
    Not,
    Mod,
    Assign,
    LeftAssign,
    LParen,
    RParen,
    Comma,
    Semicolon,
    Newline,
    Tilde,
    Colon,
    Eof,
}

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
            while let Some(ch) = self.advance() {
                if ch == '\n' {
                    break;
                }
            }
        }
    }

    fn next_token(&mut self) -> Token {
        loop {
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

            if ch.is_ascii_digit() || (ch == '.' && self.peek_digit_at(1)) {
                return self.read_number();
            }

            if ch.is_alphabetic() || ch == '.' || ch == '_' {
                return self.read_ident();
            }

            self.advance();

            return match ch {
                '+' => Token::Plus,
                '-' => Token::Minus,
                '*' => Token::Star,
                '/' => {
                    if self.peek_char() == Some('%') {
                        self.advance();
                        Token::SlashPercent
                    } else {
                        Token::Slash
                    }
                }
                '^' => Token::Caret,
                '%' => {
                    if self.peek_char() == Some('%') {
                        self.advance();
                        Token::Percent
                    } else {
                        Token::Mod
                    }
                }
                '<' => {
                    if self.peek_char() == Some('-') {
                        self.advance();
                        Token::LeftAssign
                    } else if self.peek_char() == Some('=') {
                        self.advance();
                        Token::Le
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
                ',' => Token::Comma,
                ';' => Token::Semicolon,
                '~' => Token::Tilde,
                ':' => Token::Colon,
                _ => Token::Eof,
            };
        }
    }

    fn peek_digit_at(&self, offset: usize) -> bool {
        self.chars
            .get(self.pos + offset)
            .map_or(false, |c| c.is_ascii_digit())
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let mut has_dot = false;
        let mut has_e = false;

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
                if self.peek_char() == Some('+') || self.peek_char() == Some('-') {
                    s.push(self.advance().unwrap());
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
        let quote = self.advance().unwrap();
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
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
        Token::Ident(s)
    }
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    arena: *mut RArena,
}

impl Parser {
    pub fn new(input: &str, arena: &mut RArena) -> Self {
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
            arena: arena as *mut RArena,
        }
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
            return Ok(exprs.into_iter().next().unwrap());
        }

        unsafe {
            let brace_sym = Rf_install(CString::new("{").unwrap().as_ptr());
            let nil = R_NilValue();
            let mut list = Rf_cons(exprs.pop().unwrap(), nil);
            while let Some(e) = exprs.pop() {
                let cell = Rf_cons(e, list);
                list = cell;
            }
            Ok(Rf_lang2(brace_sym, list))
        }
    }

    fn skip_terminators(&mut self) {
        while self.peek() == &Token::Semicolon || self.peek() == &Token::Newline {
            self.advance();
        }
    }

    fn parse_expr(&mut self) -> Result<SEXP, ParseError> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<SEXP, ParseError> {
        let left = self.parse_or()?;

        match self.peek() {
            Token::LeftAssign | Token::Assign => {
                let _op = self.advance();
                let right = self.parse_assignment()?;
                unsafe {
                    let assign_sym = Rf_install(CString::new("<-").unwrap().as_ptr());
                    Ok(Rf_lang3(assign_sym, left, right))
                }
            }
            _ => Ok(left),
        }
    }

    fn parse_or(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_and()?;
        loop {
            match self.peek() {
                Token::Or2 => {
                    self.advance();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(CString::new("||").unwrap().as_ptr());
                        left = Rf_lang3(op, left, right);
                    }
                }
                Token::Or => {
                    self.advance();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(CString::new("|").unwrap().as_ptr());
                        left = Rf_lang3(op, left, right);
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
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(CString::new("&&").unwrap().as_ptr());
                        left = Rf_lang3(op, left, right);
                    }
                }
                Token::And => {
                    self.advance();
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(CString::new("&").unwrap().as_ptr());
                        left = Rf_lang3(op, left, right);
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
                let op = Rf_install(CString::new("!").unwrap().as_ptr());
                Ok(Rf_lang2(op, operand))
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
            let right = self.parse_addition()?;
            unsafe {
                let op = Rf_install(CString::new(op_name).unwrap().as_ptr());
                left = Rf_lang3(op, left, right);
            }
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
            let right = self.parse_multiplication()?;
            unsafe {
                let op = Rf_install(CString::new(op_name).unwrap().as_ptr());
                left = Rf_lang3(op, left, right);
            }
        }
    }

    fn parse_multiplication(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_power()?;
        loop {
            let op_name = match self.peek() {
                Token::Star => "*",
                Token::Slash => "/",
                Token::Percent => "%%",
                Token::SlashPercent => "%/%",
                _ => return Ok(left),
            };
            self.advance();
            let right = self.parse_power()?;
            unsafe {
                let op = Rf_install(CString::new(op_name).unwrap().as_ptr());
                left = Rf_lang3(op, left, right);
            }
        }
    }

    fn parse_power(&mut self) -> Result<SEXP, ParseError> {
        let base = self.parse_unary()?;
        if self.peek() == &Token::Caret {
            self.advance();
            let exp = self.parse_unary()?;
            unsafe {
                let op = Rf_install(CString::new("^").unwrap().as_ptr());
                Ok(Rf_lang3(op, base, exp))
            }
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<SEXP, ParseError> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                unsafe {
                    let op = Rf_install(CString::new("-").unwrap().as_ptr());
                    Ok(Rf_lang2(op, operand))
                }
            }
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_call_or_atom(),
        }
    }

    fn parse_call_or_atom(&mut self) -> Result<SEXP, ParseError> {
        let atom = self.parse_atom()?;

        if self.peek() == &Token::LParen {
            self.advance();
            let args = self.parse_arglist()?;
            self.expect(&Token::RParen)?;

            unsafe {
                let nil = R_NilValue();
                let mut arg_list = nil;
                for (name, val) in args.into_iter().rev() {
                    let cell = Rf_cons(val, arg_list);
                    if let Some(n) = name {
                        let sym = Rf_install(CString::new(n).unwrap().as_ptr());
                        crate::sexp::accessors::SETTAG(cell, sym);
                    }
                    arg_list = cell;
                }
                Ok(Rf_lang2(atom, arg_list))
            }
        } else {
            Ok(atom)
        }
    }

    fn parse_arglist(&mut self) -> Result<Vec<(Option<String>, SEXP)>, ParseError> {
        let mut args = Vec::new();
        self.skip_terminators();
        if self.peek() == &Token::RParen {
            return Ok(args);
        }

        loop {
            self.skip_terminators();
            let (name, val) = self.parse_arg()?;
            args.push((name, val));
            self.skip_terminators();

            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_terminators();
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_arg(&mut self) -> Result<(Option<String>, SEXP), ParseError> {
        if let Token::Ident(name) = self.peek().clone() {
            let saved = self.pos;
            self.advance();
            if self.peek() == &Token::Assign {
                self.advance();
                let val = self.parse_expr()?;
                return Ok((Some(name), val));
            }
            self.pos = saved;
        }
        let val = self.parse_expr()?;
        Ok((None, val))
    }

    fn parse_atom(&mut self) -> Result<SEXP, ParseError> {
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                unsafe { Ok(Rf_ScalarReal(n)) }
            }
            Token::Int(n) => {
                self.advance();
                unsafe { Ok(Rf_ScalarInteger(n)) }
            }
            Token::Str(s) => {
                self.advance();
                unsafe {
                    let c_s = CString::new(s.as_str()).unwrap_or_default();
                    Ok(Rf_mkString(c_s.as_ptr()))
                }
            }
            Token::Ident(ref name) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "TRUE" => unsafe { Ok(Rf_ScalarLogical(TRUE)) },
                    "FALSE" => unsafe { Ok(Rf_ScalarLogical(FALSE)) },
                    "NULL" => unsafe { Ok(R_NilValue()) },
                    "NA" => unsafe { Ok(Rf_ScalarInteger(crate::sexp::ffi::NA_INTEGER)) },
                    "Inf" => unsafe { Ok(Rf_ScalarReal(f64::INFINITY)) },
                    "NaN" => unsafe { Ok(Rf_ScalarReal(f64::NAN)) },
                    "NA_real_" => unsafe { Ok(Rf_ScalarReal(crate::sexp::ffi::NA_REAL)) },
                    "NA_integer_" => unsafe { Ok(Rf_ScalarInteger(crate::sexp::ffi::NA_INTEGER)) },
                    "NA_character_" => unsafe {
                        let c_na = CString::new("NA").unwrap();
                        Ok(Rf_mkString(c_na.as_ptr()))
                    },
                    _ => unsafe {
                        let sym = Rf_install(CString::new(name.as_str()).unwrap().as_ptr());
                        Ok(sym)
                    },
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            ref tok => Err(ParseError(format!("unexpected token: {:?}", tok))),
        }
    }
}

pub fn parse(input: &str, arena: &mut RArena) -> Result<SEXP, ParseError> {
    let mut parser = Parser::new(input, arena);
    parser.parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{CAR, TYPEOF};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::memory::RArena;

    fn parse_str(input: &str) -> Result<SEXP, ParseError> {
        let mut arena = RArena::new();
        parse(input, &mut arena)
    }

    #[test]
    fn test_integer_literal() {
        unsafe {
            let result = parse_str("42").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
        }
    }

    #[test]
    fn test_real_literal() {
        unsafe {
            let result = parse_str("3.14").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
        }
    }

    #[test]
    fn test_int_suffix() {
        unsafe {
            let result = parse_str("42L").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
        }
    }

    #[test]
    fn test_string_literal() {
        unsafe {
            let result = parse_str("\"hello\"").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP.0);
        }
    }

    #[test]
    fn test_true_false_null() {
        unsafe {
            let t = parse_str("TRUE").unwrap();
            assert_eq!(TYPEOF(t), SEXPTYPE::LGLSXP.0);

            let f = parse_str("FALSE").unwrap();
            assert_eq!(TYPEOF(f), SEXPTYPE::LGLSXP.0);

            let n = parse_str("NULL").unwrap();
            assert_eq!(n, R_NilValue());
        }
    }

    #[test]
    fn test_identifier() {
        unsafe {
            let result = parse_str("x").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::SYMSXP.0);
        }
    }

    #[test]
    fn test_addition() {
        unsafe {
            let result = parse_str("1 + 2").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);

            let op = CAR(result);
            assert_eq!(TYPEOF(op), SEXPTYPE::SYMSXP.0);
        }
    }

    #[test]
    fn test_chained_addition() {
        unsafe {
            let result = parse_str("1 + 2 + 3").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_operator_precedence() {
        unsafe {
            let result = parse_str("2 + 3 * 4").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_unary_minus() {
        unsafe {
            let result = parse_str("-5").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_assignment() {
        unsafe {
            let result = parse_str("x <- 42").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_equals_assign() {
        unsafe {
            let result = parse_str("x = 42").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_function_call() {
        unsafe {
            let result = parse_str("f(x, y)").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);

            let fun = CAR(result);
            assert_eq!(TYPEOF(fun), SEXPTYPE::SYMSXP.0);
        }
    }

    #[test]
    fn test_parenthesized() {
        unsafe {
            let result = parse_str("(1 + 2)").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_comparison() {
        unsafe {
            let result = parse_str("x < 10").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_multi_expr() {
        unsafe {
            let result = parse_str("x <- 1; y <- 2").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_empty_input() {
        unsafe {
            let result = parse_str("").unwrap();
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_complex_expr() {
        unsafe {
            let result = parse_str("sqrt(x^2 + y^2)").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_named_arg() {
        unsafe {
            let result = parse_str("f(x = 1)").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_na_inf_nan() {
        unsafe {
            let na = parse_str("NA").unwrap();
            assert_eq!(TYPEOF(na), SEXPTYPE::INTSXP.0);

            let inf = parse_str("Inf").unwrap();
            assert_eq!(TYPEOF(inf), SEXPTYPE::REALSXP.0);

            let nan = parse_str("NaN").unwrap();
            assert_eq!(TYPEOF(nan), SEXPTYPE::REALSXP.0);
        }
    }

    #[test]
    fn test_power() {
        unsafe {
            let result = parse_str("2^3").unwrap();
            assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_logical_ops() {
        unsafe {
            let and = parse_str("x && y").unwrap();
            assert_eq!(TYPEOF(and), SEXPTYPE::LANGSXP.0);

            let or = parse_str("x || y").unwrap();
            assert_eq!(TYPEOF(or), SEXPTYPE::LANGSXP.0);

            let not = parse_str("!x").unwrap();
            assert_eq!(TYPEOF(not), SEXPTYPE::LANGSXP.0);
        }
    }

    #[test]
    fn test_error_unexpected_rparen() {
        assert!(parse_str(")").is_err());
    }
}
