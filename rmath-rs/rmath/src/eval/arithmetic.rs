//! Minimal arithmetic and comparison builtin operations.
//!
//! These handle the core numeric operators (+, -, *, /, ^, %%, %/%),
//! comparison operators (<, >, <=, >=, ==, !=), and unary operators (!, -).
//!
//! In R, these are "builtin" functions — arguments are evaluated before
//! the function is called, unlike "special" forms.

use std::ffi::CString;

use crate::sexp::accessors::{CAR, CDR, TYPEOF};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal};
use crate::sexp::ffi::{FALSE, NA_REAL, R_NA_BIT_PATTERN, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

fn real_val(x: SEXP) -> Option<f64> {
    unsafe {
        if x.is_null() {
            return None;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP.0 => {
                let data = crate::sexp::accessors::REAL(x);
                if data.is_null() { None } else { Some(*data) }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                let data = crate::sexp::accessors::INTEGER(x);
                if data.is_null() {
                    None
                } else {
                    Some(*data as f64)
                }
            }
            t if t == SEXPTYPE::LGLSXP.0 => {
                let data = crate::sexp::accessors::LOGICAL(x);
                if data.is_null() {
                    None
                } else {
                    Some(*data as f64)
                }
            }
            _ => None,
        }
    }
}

fn int_val(x: SEXP) -> Option<i32> {
    unsafe {
        if x.is_null() {
            return None;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP.0 => {
                let data = crate::sexp::accessors::INTEGER(x);
                if data.is_null() { None } else { Some(*data) }
            }
            t if t == SEXPTYPE::LGLSXP.0 => {
                let data = crate::sexp::accessors::LOGICAL(x);
                if data.is_null() { None } else { Some(*data) }
            }
            _ => None,
        }
    }
}

fn is_int_op(a: SEXP, b: SEXP) -> bool {
    unsafe {
        (TYPEOF(a) == SEXPTYPE::INTSXP.0 || TYPEOF(a) == SEXPTYPE::LGLSXP.0)
            && (TYPEOF(b) == SEXPTYPE::INTSXP.0 || TYPEOF(b) == SEXPTYPE::LGLSXP.0)
    }
}

unsafe fn binary_arith(op: &str, a: SEXP, b: SEXP) -> SEXP { unsafe {
    let va = real_val(a);
    let vb = real_val(b);
    match (va, vb) {
        (Some(x), Some(y)) => {
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;
            if x_na || y_na {
                return Rf_ScalarReal(NA_REAL);
            }
            let result = match op {
                "+" => x + y,
                "-" => x - y,
                "*" => x * y,
                "/" => x / y,
                "^" => libm::pow(x, y),
                "%%" => {
                    if y == 0.0 {
                        f64::NAN
                    } else {
                        x % y
                    }
                }
                "%/%" => {
                    if y == 0.0 {
                        f64::NAN
                    } else {
                        (x / y).floor()
                    }
                }
                _ => return R_NilValue(),
            };
            if is_int_op(a, b)
                && result.is_finite()
                && result == result.floor()
                && result as i64 as f64 == result
            {
                Rf_ScalarInteger(result as i32)
            } else {
                Rf_ScalarReal(result)
            }
        }
        _ => R_NilValue(),
    }
}}

unsafe fn binary_compare(op: &str, a: SEXP, b: SEXP) -> SEXP { unsafe {
    let va = real_val(a);
    let vb = real_val(b);
    match (va, vb) {
        (Some(x), Some(y)) => {
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;
            if x_na || y_na {
                return Rf_ScalarLogical(crate::sexp::ffi::NA_LOGICAL);
            }
            let result = match op {
                "<" => x < y,
                ">" => x > y,
                "<=" => x <= y,
                ">=" => x >= y,
                "==" => x == y,
                "!=" => x != y,
                _ => return R_NilValue(),
            };
            Rf_ScalarLogical(if result { TRUE } else { FALSE })
        }
        _ => Rf_ScalarLogical(FALSE),
    }
}}

pub unsafe fn do_arith(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        match op_name {
            "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" => {
                if args == R_NilValue() || CDR(args) == R_NilValue() {
                    if op_name == "-" {
                        if let Some(x) = real_val(CAR(args)) {
                            if let Some(_iv) = int_val(CAR(args)) {
                                return Rf_ScalarInteger(-(int_val(CAR(args)).unwrap()));
                            }
                            return Rf_ScalarReal(-x);
                        }
                    }
                    if op_name == "+" {
                        return CAR(args);
                    }
                    return R_NilValue();
                }
                binary_arith(op_name, CAR(args), CAR(CDR(args)))
            }
            _ => R_NilValue(),
        }
    }
}

pub unsafe fn do_relop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        match op_name {
            "<" | ">" | "<=" | ">=" | "==" | "!=" => {
                binary_compare(op_name, CAR(args), CAR(CDR(args)))
            }
            _ => R_NilValue(),
        }
    }
}

unsafe fn get_op_name(call: SEXP) -> &'static str { unsafe {
    if call.is_null() {
        return "";
    }
    let fun_sym = CAR(call);
    if TYPEOF(fun_sym) != SEXPTYPE::SYMSXP.0 {
        return "";
    }
    let pname = crate::sexp::accessors::PRINTNAME(fun_sym);
    if pname.is_null() {
        return "";
    }
    let s = crate::sexp::accessors::CHAR(pname);
    if s.is_null() {
        return "";
    }
    match std::ffi::CStr::from_ptr(s).to_str() {
        Ok(name) => match name {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "^" => "^",
            "%%" => "%%",
            "%/%" => "%/%",
            "<" => "<",
            ">" => ">",
            "<=" => "<=",
            ">=" => ">=",
            "==" => "==",
            "!=" => "!=",
            "!" => "!",
            "abs" => "abs",
            "sqrt" => "sqrt",
            "log" => "log",
            "log2" => "log2",
            "log10" => "log10",
            "exp" => "exp",
            "ceiling" => "ceiling",
            "floor" => "floor",
            "trunc" => "trunc",
            "round" => "round",
            "sign" => "sign",
            "length" => "length",
            "sum" => "sum",
            "min" => "min",
            "max" => "max",
            "prod" => "prod",
            "range" => "range",
            "is.numeric" => "is.numeric",
            "is.integer" => "is.integer",
            "is.double" => "is.double",
            "is.logical" => "is.logical",
            "is.character" => "is.character",
            "is.null" => "is.null",
            _ => "",
        },
        Err(_) => "",
    }
}}

/// Register arithmetic and comparison builtins in the base environment.
///
/// Creates BUILTINSXP nodes for each operator and binds them as symbols
/// in the base environment.
pub unsafe fn register_arithmetic_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::persistent_cons;
        use crate::sexp::ffi::SexprecCore;

        let all_ops = [
            "+",
            "-",
            "*",
            "/",
            "^",
            "%%",
            "%/%",
            "<",
            ">",
            "<=",
            ">=",
            "==",
            "!=",
            "abs",
            "sqrt",
            "log",
            "log2",
            "log10",
            "exp",
            "ceiling",
            "floor",
            "trunc",
            "round",
            "sign",
            "length",
            "sum",
            "min",
            "max",
            "prod",
            "range",
            "is.numeric",
            "is.integer",
            "is.double",
            "is.logical",
            "is.character",
            "is.null",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for op_name in &all_ops {
            let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::BUILTINSXP));
            boxed.sxpinfo.set_gp(1);
            let prim: SEXP = Box::leak(boxed);
            let sym = Rf_install(CString::new(*op_name).unwrap().as_ptr());
            let cell = persistent_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

pub unsafe fn register_special_forms(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::persistent_cons;
        use crate::sexp::ffi::SexprecCore;

        let special_forms = [
            "<-",
            "<<-",
            "=",
            "if",
            "{",
            "(",
            "function",
            "while",
            "for",
            "repeat",
            "break",
            "next",
            "return",
            "invisible",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for op_name in &special_forms {
            let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::SPECIALSXP));
            boxed.sxpinfo.set_gp(1);
            let prim: SEXP = Box::leak(boxed);
            let sym = Rf_install(CString::new(*op_name).unwrap().as_ptr());
            let cell = persistent_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

pub unsafe fn do_math1(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let v = real_val(x);
        if let Some(v) = v {
            if v.to_bits() == R_NA_BIT_PATTERN {
                return Rf_ScalarReal(NA_REAL);
            }
            let result = match op_name {
                "abs" => v.abs(),
                "sqrt" => {
                    if v < 0.0 {
                        f64::NAN
                    } else {
                        libm::sqrt(v)
                    }
                }
                "log" => libm::log(v),
                "log2" => libm::log2(v),
                "log10" => libm::log10(v),
                "exp" => libm::exp(v),
                "ceiling" => v.ceil(),
                "floor" => v.floor(),
                "trunc" => v.trunc(),
                "round" => v.round(),
                "sign" => {
                    if v > 0.0 {
                        1.0
                    } else if v < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                _ => return R_NilValue(),
            };
            if result.is_finite()
                && result == result.floor()
                && result as i64 as f64 == result
                && (result as i64) >= i32::MIN as i64
                && (result as i64) <= i32::MAX as i64
            {
                let v_int = int_val(x);
                if v_int.is_some()
                    || op_name == "ceiling"
                    || op_name == "floor"
                    || op_name == "trunc"
                    || op_name == "round"
                {
                    return Rf_ScalarInteger(result as i32);
                }
            }
            Rf_ScalarReal(result)
        } else {
            R_NilValue()
        }
    }
}

pub unsafe fn do_length(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        Rf_ScalarInteger(crate::sexp::accessors::LENGTH(x))
    }
}

pub unsafe fn do_summary(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let mut vals: Vec<f64> = Vec::new();
        let mut has_na = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(v) = real_val(CAR(current)) {
                if v.to_bits() == R_NA_BIT_PATTERN {
                    has_na = true;
                }
                vals.push(v);
            }
            current = CDR(current);
        }
        if vals.is_empty() {
            return R_NilValue();
        }
        if has_na {
            return Rf_ScalarReal(NA_REAL);
        }
        let result = match op_name {
            "sum" => vals.iter().fold(0.0, |a, &b| a + b),
            "min" => vals.iter().cloned().fold(f64::INFINITY, f64::min),
            "max" => vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            "prod" => vals.iter().fold(1.0, |a, &b| a * b),
            "range" => {
                let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let v = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::REALSXP.0, 2);
                let data = crate::sexp::accessors::REAL(v);
                *data.add(0) = lo;
                *data.add(1) = hi;
                return v;
            }
            _ => return R_NilValue(),
        };
        Rf_ScalarReal(result)
    }
}

pub unsafe fn do_is_type(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            if op_name == "is.null" {
                return Rf_ScalarLogical(crate::sexp::ffi::TRUE);
            }
            return Rf_ScalarLogical(crate::sexp::ffi::FALSE);
        }
        let t = TYPEOF(x);
        let result = match op_name {
            "is.numeric" => t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0,
            "is.integer" => t == SEXPTYPE::INTSXP.0,
            "is.double" => t == SEXPTYPE::REALSXP.0,
            "is.logical" => t == SEXPTYPE::LGLSXP.0,
            "is.character" => t == SEXPTYPE::STRSXP.0,
            "is.null" => false,
            _ => false,
        };
        Rf_ScalarLogical(if result {
            crate::sexp::ffi::TRUE
        } else {
            crate::sexp::ffi::FALSE
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::TYPEOF;

    #[test]
    fn test_addition() {
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = binary_arith("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 3);
        }
    }

    #[test]
    fn test_real_addition() {
        unsafe {
            let a = Rf_ScalarReal(1.5);
            let b = Rf_ScalarReal(2.5);
            let result = binary_arith("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            let v = *crate::sexp::accessors::REAL(result);
            assert!((v - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_multiplication() {
        unsafe {
            let a = Rf_ScalarInteger(3);
            let b = Rf_ScalarInteger(4);
            let result = binary_arith("*", a, b);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 12);
        }
    }

    #[test]
    fn test_division_produces_real() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = binary_arith("/", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            let v = *crate::sexp::accessors::REAL(result);
            assert!((v - 3.3333333333333335).abs() < 1e-10);
        }
    }

    #[test]
    fn test_power() {
        unsafe {
            let a = Rf_ScalarInteger(2);
            let b = Rf_ScalarInteger(10);
            let result = binary_arith("^", a, b);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 1024);
        }
    }

    #[test]
    fn test_comparison_lt() {
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = binary_compare("<", a, b);
            assert_eq!(*crate::sexp::accessors::LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_eq() {
        unsafe {
            let a = Rf_ScalarInteger(5);
            let b = Rf_ScalarInteger(5);
            let result = binary_compare("==", a, b);
            assert_eq!(*crate::sexp::accessors::LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_ne() {
        unsafe {
            let a = Rf_ScalarReal(1.0);
            let b = Rf_ScalarReal(2.0);
            let result = binary_compare("!=", a, b);
            assert_eq!(*crate::sexp::accessors::LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_modulo() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = binary_arith("%%", a, b);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 1);
        }
    }

    #[test]
    fn test_integer_division() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = binary_arith("%/%", a, b);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 3);
        }
    }
}
