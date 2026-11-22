#![warn(unused_unsafe)]
//! Plural expression parser for GNU gettext.
//!
//! Bounded precedence-climbing parser producing the owned `Expr` AST in
//! `plural_exp`. Grammar matches the Bison original: ternary `?:` is the
//! lowest-precedence, right-associative operator; binary operators are
//! left-associative in `||`, `&&`, equality, comparison, additive, and
//! multiplicative tiers; `!`, `n`, numbers, and parentheses are primaries.

use std::os::raw::c_ulong;

use super::plural_exp::{Expr, Op};

const OPS: &[(&[u8], u8, Op)] = &[
    (b"||", 1, Op::Or),
    (b"&&", 2, Op::And),
    (b"==", 3, Op::Eq),
    (b"!=", 3, Op::Ne),
    (b"<=", 4, Op::Le),
    (b">=", 4, Op::Ge),
    (b"<", 4, Op::Lt),
    (b">", 4, Op::Gt),
    (b"+", 5, Op::Add),
    (b"-", 5, Op::Sub),
    (b"*", 6, Op::Mul),
    (b"/", 6, Op::Div),
    (b"%", 6, Op::Rem),
];

struct Parser<'a> {
    rest: &'a [u8],
    remaining: usize,
}

impl Parser<'_> {
    fn eat(&mut self, token: &[u8]) -> bool {
        self.rest = self.rest.trim_ascii_start();
        if let Some(tail) = self.rest.strip_prefix(token) {
            self.rest = tail;
            true
        } else {
            false
        }
    }

    fn spend(&mut self) -> Option<()> {
        self.remaining = self.remaining.checked_sub(1)?;
        Some(())
    }

    fn primary(&mut self) -> Option<Expr> {
        self.spend()?;
        if self.eat(b"!") {
            return Some(Expr::Not(Box::new(self.primary()?)));
        }
        if self.eat(b"(") {
            let inner = self.expression(0)?;
            return self.eat(b")").then_some(inner);
        }
        if self.eat(b"n") {
            return Some(Expr::N);
        }
        let mut value = 0 as c_ulong;
        let end = self
            .rest
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(self.rest.len());
        if end == 0 {
            return None;
        }
        for &byte in &self.rest[..end] {
            value = value
                .wrapping_mul(10)
                .wrapping_add(c_ulong::from(byte - b'0'));
        }
        self.rest = &self.rest[end..];
        Some(Expr::Number(value))
    }

    fn expression(&mut self, min: u8) -> Option<Expr> {
        let mut left = self.primary()?;
        loop {
            self.rest = self.rest.trim_ascii_start();
            let Some(&(token, prec, op)) = OPS
                .iter()
                .find(|&&(token, _, _)| self.rest.starts_with(token))
            else {
                break;
            };
            if prec < min {
                break;
            }
            self.spend()?;
            self.rest = &self.rest[token.len()..];
            let right = self.expression(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        if min == 0 && self.eat(b"?") {
            self.spend()?;
            let yes = self.expression(0)?;
            if !self.eat(b":") {
                return None;
            }
            let no = self.expression(0)?;
            left = Expr::Conditional(Box::new(left), Box::new(yes), Box::new(no));
        }
        Some(left)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Option<Expr> {
    // Limit both nested calls and the size of left-associated trees.
    let mut parser = Parser {
        rest: bytes,
        remaining: 128,
    };
    let expr = parser.expression(0)?;
    parser.eat(b";");
    parser.rest.trim_ascii().is_empty().then_some(expr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &[u8], n: c_ulong) -> Option<c_ulong> {
        parse(src).map(|expr| expr.eval(n))
    }

    #[test]
    fn test_parse_n() {
        assert_eq!(eval(b"n", 5), Some(5));
    }

    #[test]
    fn test_parse_number() {
        assert_eq!(eval(b"42", 0), Some(42));
    }

    #[test]
    fn test_parse_n_not_eq_1() {
        assert_eq!(eval(b"n != 1", 1), Some(0));
        assert_eq!(eval(b"n != 1", 2), Some(1));
    }

    #[test]
    fn test_parse_ternary() {
        assert_eq!(eval(b"n == 1 ? 0 : 1", 1), Some(0));
        assert_eq!(eval(b"n == 1 ? 0 : 1", 2), Some(1));
    }

    #[test]
    fn test_parse_ternary_nested_and_right_associative() {
        // a ? b : c ? d : e parses as a ? b : (c ? d : e).
        assert_eq!(eval(b"n == 1 ? 0 : n == 2 ? 1 : 2", 1), Some(0));
        assert_eq!(eval(b"n == 1 ? 0 : n == 2 ? 1 : 2", 2), Some(1));
        assert_eq!(eval(b"n == 1 ? 0 : n == 2 ? 1 : 2", 9), Some(2));
        assert_eq!(eval(b"(n == 1) ? (n ? 2 : 3) : 4", 1), Some(2));
    }

    #[test]
    fn test_parse_complex() {
        let src =
            b"n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2;";
        assert_eq!(eval(src, 1), Some(0));
        assert_eq!(eval(src, 5), Some(2));
        assert_eq!(eval(src, 21), Some(0));
    }

    #[test]
    fn test_parse_with_semicolon() {
        assert_eq!(eval(b"n != 1;", 1), Some(0));
    }

    #[test]
    fn test_parse_not() {
        assert_eq!(eval(b"!n", 0), Some(1));
        assert_eq!(eval(b"!n", 5), Some(0));
    }

    #[test]
    fn test_parse_modulo() {
        assert_eq!(eval(b"n % 3", 7), Some(1));
    }

    #[test]
    fn test_parse_chained_comparison() {
        // (n < 4) < 2 is always true.
        assert_eq!(eval(b"n < 4 < 2", 1), Some(1));
        assert_eq!(eval(b"n < 4 < 2", 9), Some(1));
    }

    #[test]
    fn test_parse_malformed() {
        assert!(parse(b"n +").is_none());
        assert!(parse(b"n ? 1").is_none());
        assert!(parse(b"(n").is_none());
        assert!(parse(b"n junk").is_none());
    }

    #[test]
    fn test_parse_rejects_excessive_nesting() {
        let mut nested = b"!".repeat(129);
        nested.push(b'n');
        assert!(parse(&nested).is_none());
        let mut long = b"1+".repeat(129);
        long.push(b'1');
        assert!(parse(&long).is_none());
    }
}
