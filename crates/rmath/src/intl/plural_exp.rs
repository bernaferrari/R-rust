#![warn(unused_unsafe)]
//! Plural expression evaluation for GNU gettext.
//!
//! Owned AST and metadata extraction for the `Plural-Forms:` entry of a
//! .mo file, replacing the pointer-based expression trees.

use std::os::raw::c_ulong;

use super::plural_parse::parse;

#[derive(Clone, Copy)]
pub(super) enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

pub(super) enum Expr {
    N,
    Number(c_ulong),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    pub(super) fn eval(&self, n: c_ulong) -> c_ulong {
        match self {
            Self::N => n,
            Self::Number(value) => *value,
            Self::Not(value) => c_ulong::from(value.eval(n) == 0),
            Self::Conditional(cond, yes, no) => {
                if cond.eval(n) != 0 {
                    yes.eval(n)
                } else {
                    no.eval(n)
                }
            }
            Self::Binary(op, left, right) => {
                let a = left.eval(n);
                match op {
                    Op::Or => c_ulong::from(a != 0 || right.eval(n) != 0),
                    Op::And => c_ulong::from(a != 0 && right.eval(n) != 0),
                    _ => {
                        let b = right.eval(n);
                        match op {
                            Op::Eq => c_ulong::from(a == b),
                            Op::Ne => c_ulong::from(a != b),
                            Op::Lt => c_ulong::from(a < b),
                            Op::Le => c_ulong::from(a <= b),
                            Op::Gt => c_ulong::from(a > b),
                            Op::Ge => c_ulong::from(a >= b),
                            Op::Add => a.wrapping_add(b),
                            Op::Sub => a.wrapping_sub(b),
                            Op::Mul => a.wrapping_mul(b),
                            Op::Div => a.checked_div(b).unwrap_or(0),
                            Op::Rem => a.checked_rem(b).unwrap_or(0),
                            Op::And | Op::Or => unreachable!(),
                        }
                    }
                }
            }
        }
    }
}

pub(super) struct PluralForms {
    count: c_ulong,
    expr: Option<Expr>,
}

impl PluralForms {
    pub(super) fn from_metadata(bytes: &[u8]) -> Self {
        for line in bytes.split(|&byte| byte == b'\n') {
            let line = line.trim_ascii();
            let line = line.strip_prefix(b"Plural-Forms:").unwrap_or(line);
            let mut count = None;
            let mut expr = None;
            for field in line.split(|&byte| byte == b';') {
                let Some(eq) = field.iter().position(|&byte| byte == b'=') else {
                    continue;
                };
                let value = field[eq + 1..].trim_ascii();
                match field[..eq].trim_ascii() {
                    b"nplurals" => {
                        count = std::str::from_utf8(value)
                            .ok()
                            .and_then(|s| s.parse::<c_ulong>().ok())
                            .filter(|&n| n != 0)
                    }
                    b"plural" => expr = parse(value),
                    _ => (),
                }
            }
            if let (Some(count), Some(expr)) = (count, expr) {
                return Self {
                    count,
                    expr: Some(expr),
                };
            }
        }
        Self {
            count: 2,
            expr: None,
        }
    }

    pub(super) fn index(&self, n: c_ulong) -> usize {
        let index = self
            .expr
            .as_ref()
            .map_or_else(|| c_ulong::from(n != 1), |expr| expr.eval(n));
        if index < self.count {
            usize::try_from(index).unwrap_or(0)
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_missing_fields_defaults_to_germanic() {
        let forms = PluralForms::from_metadata(b"Project-Id-Version: foo 1.0\n");
        assert_eq!(forms.index(1), 0);
        assert_eq!(forms.index(0), 1);
        assert_eq!(forms.index(2), 1);
    }

    #[test]
    fn test_metadata_invalid_fields_defaults_to_germanic() {
        let forms = PluralForms::from_metadata(b"Plural-Forms: nplurals=x; plural=&&&;\n");
        assert_eq!(forms.index(1), 0);
        assert_eq!(forms.index(2), 1);
    }

    #[test]
    fn test_metadata_three_form_expression() {
        let forms = PluralForms::from_metadata(
            b"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n",
        );
        assert_eq!(forms.index(1), 0);
        assert_eq!(forms.index(5), 2);
        assert_eq!(forms.index(21), 0);
    }

    #[test]
    fn test_metadata_whitespace_and_clamped_index() {
        let forms = PluralForms::from_metadata(b"Plural-Forms: nplurals = 3; plural = n;\n");
        assert_eq!(forms.index(2), 2);
        // Index beyond nplurals falls back to form 0.
        assert_eq!(forms.index(3), 0);
    }

    #[test]
    fn test_metadata_accepts_non_utf8_other_lines() {
        let forms = PluralForms::from_metadata(
            b"X-Note: \xff\xfe\nPlural-Forms: nplurals=3; plural=n==1 ? 0 : n==2 ? 1 : 2;\n",
        );
        assert_eq!(forms.index(1), 0);
        assert_eq!(forms.index(2), 1);
        assert_eq!(forms.index(4), 2);
    }
}
