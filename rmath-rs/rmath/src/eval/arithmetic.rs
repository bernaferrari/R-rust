//! Minimal arithmetic and comparison builtin operations.
//!
//! These handle the core numeric operators (+, -, *, /, ^, %%, %/%),
//! comparison operators (<, >, <=, >=, ==, !=), and unary operators (!, -).
//!
//! In R, these are "builtin" functions — arguments are evaluated before
//! the function is called, unlike "special" forms.

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, TYPEOF};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_cons};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
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

unsafe fn binary_arith(op: &str, a: SEXP, b: SEXP) -> SEXP {
    let va = real_val(a);
    let vb = real_val(b);
    match (va, vb) {
        (Some(x), Some(y)) => {
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
}

unsafe fn binary_compare(op: &str, a: SEXP, b: SEXP) -> SEXP {
    let va = real_val(a);
    let vb = real_val(b);
    match (va, vb) {
        (Some(x), Some(y)) => {
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
}

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

unsafe fn get_op_name(call: SEXP) -> &'static str {
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
            _ => "",
        },
        Err(_) => "",
    }
}

/// Register arithmetic and comparison builtins in the base environment.
///
/// Creates BUILTINSXP nodes for each operator and binds them as symbols
/// in the base environment.
pub unsafe fn register_arithmetic_builtins(env: SEXP) {
    unsafe {
        let arith_ops = ["+", "-", "*", "/", "^", "%%", "%/%"];
        let rel_ops = ["<", ">", "<=", ">=", "==", "!="];

        for op_name in &arith_ops {
            let prim = crate::sexp::memory::with_arena(|arena| {
                let p = arena.alloc_node(SEXPTYPE::BUILTINSXP);
                if !p.is_null() {
                    (*p).sxpinfo.set_gp(1);
                }
                p
            });
            if !prim.is_null() {
                let sym = Rf_install(CString::new(*op_name).unwrap().as_ptr());
                crate::sexp::envir::defineVar(sym, prim, env);
            }
        }

        for op_name in &rel_ops {
            let prim = crate::sexp::memory::with_arena(|arena| {
                let p = arena.alloc_node(SEXPTYPE::BUILTINSXP);
                if !p.is_null() {
                    (*p).sxpinfo.set_gp(1);
                }
                p
            });
            if !prim.is_null() {
                let sym = Rf_install(CString::new(*op_name).unwrap().as_ptr());
                crate::sexp::envir::defineVar(sym, prim, env);
            }
        }
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
