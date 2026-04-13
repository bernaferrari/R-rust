//! Vectorized arithmetic and comparison builtin operations.
//!
//! These handle the core numeric operators (+, -, *, /, ^, %%, %/%),
//! comparison operators (<, >, <=, >=, ==, !=), and unary operators (!, -).
//!
//! In R, these are "builtin" functions — arguments are evaluated before
//! the function is called, unlike "special" forms.
//!
//! All binary operations support R's recycling rule: shorter vectors are
//! recycled to match the length of the longer operand.

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, INTEGER, LENGTH, LOGICAL, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3,
};
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::Rf_protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Scalar helpers (kept for single-element fast path)
// ---------------------------------------------------------------------------

/// Read a scalar real value from a numeric SEXP (element 0 only).
fn real_val(x: SEXP) -> Option<f64> {
    unsafe {
        if x.is_null() {
            return None;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP.0 => {
                let data = REAL(x);
                if data.is_null() { None } else { Some(*data) }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                let data = INTEGER(x);
                if data.is_null() {
                    None
                } else {
                    Some(*data as f64)
                }
            }
            t if t == SEXPTYPE::LGLSXP.0 => {
                let data = LOGICAL(x);
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

/// Read a scalar integer value from an integer or logical SEXP (element 0 only).
fn int_val(x: SEXP) -> Option<i32> {
    unsafe {
        if x.is_null() {
            return None;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP.0 => {
                let data = INTEGER(x);
                if data.is_null() { None } else { Some(*data) }
            }
            t if t == SEXPTYPE::LGLSXP.0 => {
                let data = LOGICAL(x);
                if data.is_null() { None } else { Some(*data) }
            }
            _ => None,
        }
    }
}

/// Check if both operands are integer/logical (result should prefer integer).
fn is_int_op(a: SEXP, b: SEXP) -> bool {
    unsafe {
        (TYPEOF(a) == SEXPTYPE::INTSXP.0 || TYPEOF(a) == SEXPTYPE::LGLSXP.0)
            && (TYPEOF(b) == SEXPTYPE::INTSXP.0 || TYPEOF(b) == SEXPTYPE::LGLSXP.0)
    }
}

// ---------------------------------------------------------------------------
// Element access with recycling
// ---------------------------------------------------------------------------

/// Get the f64 value at position `i` (with recycling) from a numeric SEXP.
#[inline]
unsafe fn elt_real(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP.0 => *REAL(x).add(idx as usize),
            t if t == SEXPTYPE::INTSXP.0 => {
                let v = *INTEGER(x).add(idx as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            }
            t if t == SEXPTYPE::LGLSXP.0 => {
                let v = *LOGICAL(x).add(idx as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            }
            _ => NA_REAL,
        }
    }
}

/// Get the i32 value at position `i` (with recycling) from an int/logical SEXP.
#[inline]
unsafe fn elt_int(x: SEXP, i: R_xlen_t) -> i32 {
    unsafe {
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP.0 => *INTEGER(x).add(idx as usize),
            t if t == SEXPTYPE::LGLSXP.0 => *LOGICAL(x).add(idx as usize),
            _ => NA_INTEGER,
        }
    }
}

/// Get the logical value at position `i` (with recycling) from a logical SEXP.
#[inline]
unsafe fn elt_logical(x: SEXP, i: R_xlen_t) -> i32 {
    unsafe {
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        *LOGICAL(x).add(idx as usize)
    }
}

// ---------------------------------------------------------------------------
// Vectorized binary arithmetic
// ---------------------------------------------------------------------------

/// Determine the result length for a binary operation (R's recycling rule).
/// Returns max(len(a), len(b)), or 0 if either is length 0.
#[inline]
fn result_length(a: SEXP, b: SEXP) -> R_xlen_t {
    unsafe {
        let na = XLENGTH(a);
        let nb = XLENGTH(b);
        if na == 0 || nb == 0 {
            0
        } else if na >= nb {
            na
        } else {
            nb
        }
    }
}

/// Apply a real-valued binary operation with recycling.
///
/// Returns REALSXP if either operand is REALSXP, otherwise returns INTSXP/LGLSXP.
/// Integer overflow produces NA_INTEGER.
pub unsafe fn real_binary(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let n = result_length(sa, sb);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }
        let use_real = TYPEOF(sa) == SEXPTYPE::REALSXP.0 || TYPEOF(sb) == SEXPTYPE::REALSXP.0;
        let result = if use_real {
            Rf_allocVector3(SEXPTYPE::REALSXP.0, n)
        } else {
            Rf_allocVector3(SEXPTYPE::INTSXP.0, n)
        };
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);

        let all_int = is_int_op(sa, sb);

        for i in 0..n {
            let x = elt_real(sa, i);
            let y = elt_real(sb, i);
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;

            if x_na || y_na {
                if use_real {
                    *REAL(result).add(i as usize) = NA_REAL;
                } else {
                    *INTEGER(result).add(i as usize) = NA_INTEGER;
                }
                continue;
            }

            let val = match op {
                "+" => x + y,
                "-" => x - y,
                "*" => x * y,
                "/" => {
                    if y == 0.0 {
                        NA_REAL
                    } else {
                        x / y
                    }
                }
                "^" => libm::pow(x, y),
                "%%" => {
                    if y == 0.0 {
                        NA_REAL
                    } else {
                        crate::mainutils::arithmetic::myfmod(x, y)
                    }
                }
                "%/%" => {
                    if y == 0.0 {
                        NA_REAL
                    } else {
                        crate::mainutils::arithmetic::myfloor(x, y)
                    }
                }
                _ => NA_REAL,
            };

            if use_real {
                *REAL(result).add(i as usize) = val;
            } else {
                // Integer path: check for overflow
                if val.is_finite()
                    && val == val.floor()
                    && val >= i32::MIN as f64
                    && val <= i32::MAX as f64
                {
                    let ival = val as i32;
                    // For %% and %/%, use floor semantics
                    if op == "%%" || op == "%/%" {
                        *INTEGER(result).add(i as usize) = ival;
                    } else {
                        *INTEGER(result).add(i as usize) = ival;
                    }
                } else {
                    *INTEGER(result).add(i as usize) = NA_INTEGER;
                }
            }
        }

        // For division, always return REALSXP (R semantics)
        if op == "/" {
            let reals = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            if !reals.is_null() {
                let _p2 = Rf_protect(reals);
                for i in 0..n {
                    let x = elt_real(sa, i);
                    let y = elt_real(sb, i);
                    let x_na = x.to_bits() == R_NA_BIT_PATTERN;
                    let y_na = y.to_bits() == R_NA_BIT_PATTERN;
                    if x_na || y_na || y == 0.0 {
                        *REAL(reals).add(i as usize) = if y == 0.0 { NA_REAL } else { NA_REAL };
                    } else {
                        *REAL(reals).add(i as usize) = x / y;
                    }
                }
                crate::sexp::protect::Rf_unprotect(2); // result + reals
                return reals;
            }
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// Vectorized binary comparison with recycling.
///
/// Always returns LGLSXP of the result length.
unsafe fn binary_compare(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let n = result_length(sa, sb);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.0, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);

        let use_real = TYPEOF(sa) == SEXPTYPE::REALSXP.0 || TYPEOF(sb) == SEXPTYPE::REALSXP.0;
        let dst = LOGICAL(result);

        for i in 0..n {
            let (x_na, y_na, cmp): (bool, bool, bool) = if use_real {
                let x = elt_real(sa, i);
                let y = elt_real(sb, i);
                let xn = x.to_bits() == R_NA_BIT_PATTERN;
                let yn = y.to_bits() == R_NA_BIT_PATTERN;
                let c = match op {
                    "<" => x < y,
                    ">" => x > y,
                    "<=" => x <= y,
                    ">=" => x >= y,
                    "==" => x == y,
                    "!=" => x != y,
                    _ => false,
                };
                (xn, yn, c)
            } else {
                let x = elt_int(sa, i);
                let y = elt_int(sb, i);
                let xn = x == NA_INTEGER;
                let yn = y == NA_INTEGER;
                let c = match op {
                    "<" => x < y,
                    ">" => x > y,
                    "<=" => x <= y,
                    ">=" => x >= y,
                    "==" => x == y,
                    "!=" => x != y,
                    _ => false,
                };
                (xn, yn, c)
            };

            *dst.add(i as usize) = if x_na || y_na {
                NA_LOGICAL
            } else if cmp {
                TRUE
            } else {
                FALSE
            };
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// Unary operations (vectorized)
// ---------------------------------------------------------------------------

/// Apply a unary real function element-wise to a numeric vector.
unsafe fn math1_vec(sa: SEXP, f: fn(f64) -> f64) -> SEXP {
    unsafe {
        if sa.is_null() || sa == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(sa);
        if t != SEXPTYPE::REALSXP.0 && t != SEXPTYPE::INTSXP.0 && t != SEXPTYPE::LGLSXP.0 {
            return R_NilValue();
        }
        let n = XLENGTH(sa);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let x = elt_real(sa, i);
            if x.to_bits() == R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                let mut r = f(x);
                // Preserve incoming NaN (don't replace with NA_REAL)
                if r.is_nan() && !x.is_nan() {
                    // Newly produced NaN from valid input → leave as NaN
                }
                *dst.add(i as usize) = r;
            }
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// Top-level dispatch functions (called by the evaluator)
// ---------------------------------------------------------------------------

/// Handle binary arithmetic: +, -, *, /, ^, %%, %/%
pub unsafe fn do_arith(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        match op_name {
            "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" => {
                let a = CAR(args);
                if a.is_null() || a == R_NilValue() {
                    return R_NilValue();
                }
                let b_cdr = CDR(args);
                if b_cdr.is_null() || b_cdr == R_NilValue() {
                    // Unary +/-
                    if op_name == "-" {
                        return unary_minus(a);
                    }
                    if op_name == "+" {
                        return a;
                    }
                    return R_NilValue();
                }
                let b = CAR(b_cdr);
                real_binary(op_name, a, b)
            }
            _ => R_NilValue(),
        }
    }
}

/// Handle comparison operators: <, >, <=, >=, ==, !=
pub unsafe fn do_relop(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        match op_name {
            "<" | ">" | "<=" | ">=" | "==" | "!=" => {
                let a = CAR(args);
                let b = CAR(CDR(args));
                if a.is_null() || b.is_null() {
                    return R_NilValue();
                }
                binary_compare(op_name, a, b)
            }
            _ => R_NilValue(),
        }
    }
}

/// Unary minus — negate each element of a numeric vector.
unsafe fn unary_minus(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);

        if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = Rf_protect(result);
            let src = INTEGER(x);
            let dst = INTEGER(result);
            for i in 0..n {
                let v = *src.add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER { NA_INTEGER } else { -v };
            }
            crate::sexp::protect::Rf_unprotect(1);
            result
        } else if t == SEXPTYPE::REALSXP.0 {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = Rf_protect(result);
            let src = REAL(x);
            let dst = REAL(result);
            for i in 0..n {
                *dst.add(i as usize) = -*src.add(i as usize);
            }
            crate::sexp::protect::Rf_unprotect(1);
            result
        } else {
            R_NilValue()
        }
    }
}

/// Handle unary math functions: abs, sqrt, log, log2, log10, exp,
/// ceiling, floor, trunc, round, sign.
pub unsafe fn do_math1(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let f: fn(f64) -> f64 = match op_name {
            "abs" => f64::abs,
            "sqrt" => |v: f64| if v < 0.0 { f64::NAN } else { libm::sqrt(v) },
            "log" => |v: f64| libm::log(v),
            "log2" => |v: f64| libm::log2(v),
            "log10" => |v: f64| libm::log10(v),
            "exp" => |v: f64| libm::exp(v),
            "ceiling" => f64::ceil,
            "floor" => f64::floor,
            "trunc" => f64::trunc,
            "round" => f64::round,
            "sign" => |v: f64| {
                if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            },
            _ => return R_NilValue(),
        };

        let result = math1_vec(x, f);
        if result.is_null() {
            return R_NilValue();
        }

        // For integer-friendly operations, try to return INTSXP
        let prefer_int =
            op_name == "ceiling" || op_name == "floor" || op_name == "trunc" || op_name == "round";
        let input_is_int = TYPEOF(x) == SEXPTYPE::INTSXP.0 || TYPEOF(x) == SEXPTYPE::LGLSXP.0;

        if prefer_int || input_is_int {
            let n = XLENGTH(result);
            let src = REAL(result);
            let mut all_int = true;
            for i in 0..n {
                let v = *src.add(i as usize);
                if v.to_bits() == R_NA_BIT_PATTERN {
                    continue;
                }
                if !v.is_finite() || v != v.floor() || v < i32::MIN as f64 || v > i32::MAX as f64 {
                    all_int = false;
                    break;
                }
            }
            if all_int {
                let iresult = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
                if !iresult.is_null() {
                    let _p = Rf_protect(iresult);
                    let dst = INTEGER(iresult);
                    for i in 0..n {
                        let v = *src.add(i as usize);
                        if v.to_bits() == R_NA_BIT_PATTERN {
                            *dst.add(i as usize) = NA_INTEGER;
                        } else {
                            *dst.add(i as usize) = v as i32;
                        }
                    }
                    crate::sexp::protect::Rf_unprotect(1);
                    return iresult;
                }
            }
        }

        result
    }
}

/// Handle `length(x)`.
pub unsafe fn do_length(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        Rf_ScalarInteger(LENGTH(x))
    }
}

/// Handle summary functions: sum, min, max, prod, range.
/// These accept multiple arguments and aggregate across all elements.
pub unsafe fn do_summary(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);

        // Collect all values from all arguments
        let mut vals: Vec<f64> = Vec::new();
        let mut has_na = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let t = TYPEOF(arg);
                if t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                    let n = XLENGTH(arg);
                    for i in 0..n {
                        let v = elt_real(arg, i);
                        if v.to_bits() == R_NA_BIT_PATTERN {
                            has_na = true;
                        }
                        vals.push(v);
                    }
                }
            }
            current = CDR(current);
        }

        if vals.is_empty() {
            return R_NilValue();
        }

        match op_name {
            "sum" => {
                if has_na {
                    return Rf_ScalarReal(NA_REAL);
                }
                let result = vals.iter().fold(0.0f64, |a, &b| a + b);
                Rf_ScalarReal(result)
            }
            "min" => {
                if has_na {
                    return Rf_ScalarReal(NA_REAL);
                }
                let result = vals.iter().copied().fold(f64::INFINITY, f64::min);
                Rf_ScalarReal(result)
            }
            "max" => {
                if has_na {
                    return Rf_ScalarReal(NA_REAL);
                }
                let result = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                Rf_ScalarReal(result)
            }
            "prod" => {
                if has_na {
                    return Rf_ScalarReal(NA_REAL);
                }
                let result = vals.iter().fold(1.0f64, |a, &b| a * b);
                Rf_ScalarReal(result)
            }
            "range" => {
                if has_na {
                    let v = Rf_allocVector3(SEXPTYPE::REALSXP.0, 2);
                    if !v.is_null() {
                        let data = REAL(v);
                        *data.add(0) = NA_REAL;
                        *data.add(1) = NA_REAL;
                    }
                    return v;
                }
                let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let v = Rf_allocVector3(SEXPTYPE::REALSXP.0, 2);
                if !v.is_null() {
                    let data = REAL(v);
                    *data.add(0) = lo;
                    *data.add(1) = hi;
                }
                v
            }
            _ => R_NilValue(),
        }
    }
}

/// Handle type-checking functions: is.numeric, is.integer, is.double,
/// is.logical, is.character, is.null.
pub unsafe fn do_is_type(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            if op_name == "is.null" {
                return Rf_ScalarLogical(TRUE);
            }
            return Rf_ScalarLogical(FALSE);
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
        Rf_ScalarLogical(if result { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// Operator name extraction
// ---------------------------------------------------------------------------

unsafe fn get_op_name(call: SEXP) -> &'static str {
    unsafe {
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
    }
}

// ---------------------------------------------------------------------------
// Registration functions
// ---------------------------------------------------------------------------

/// Register arithmetic and comparison builtins in the base environment.
///
/// Creates BUILTINSXP nodes for each operator and binds them as symbols
/// in the base environment. The nodes themselves are created once and
/// reused across calls to avoid leaking memory.
pub unsafe fn register_arithmetic_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::persistent_cons;
        use crate::sexp::ffi::SexprecCore;

        // Create builtin nodes once and reuse
        static BUILTIN_SEXPS: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
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

        let builtins = BUILTIN_SEXPS.get_or_init(|| {
            all_ops
                .iter()
                .map(|_| {
                    let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::BUILTINSXP));
                    boxed.sxpinfo.set_gp(1);
                    Box::into_raw(boxed) as usize
                })
                .collect::<Vec<usize>>()
        });

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for (i, op_name) in all_ops.iter().enumerate() {
            let prim: SEXP = builtins[i] as SEXP;
            let sym = Rf_install(CString::new(*op_name).unwrap_or_default().as_ptr());
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

        // Create special form nodes once and reuse
        static SPECIAL_SEXPS: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
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

        let specials = SPECIAL_SEXPS.get_or_init(|| {
            special_forms
                .iter()
                .map(|_| {
                    let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::SPECIALSXP));
                    boxed.sxpinfo.set_gp(1);
                    Box::into_raw(boxed) as usize
                })
                .collect::<Vec<usize>>()
        });

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for (i, op_name) in special_forms.iter().enumerate() {
            let prim: SEXP = specials[i] as SEXP;
            let sym = Rf_install(CString::new(*op_name).unwrap_or_default().as_ptr());
            let cell = persistent_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::TYPEOF;

    #[test]
    fn test_scalar_addition() {
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(result), 3);
        }
    }

    #[test]
    fn test_real_addition() {
        unsafe {
            let a = Rf_ScalarReal(1.5);
            let b = Rf_ScalarReal(2.5);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            let v = *REAL(result);
            assert!((v - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_scalar_multiplication() {
        unsafe {
            let a = Rf_ScalarInteger(3);
            let b = Rf_ScalarInteger(4);
            let result = real_binary("*", a, b);
            assert_eq!(*INTEGER(result), 12);
        }
    }

    #[test]
    fn test_division_produces_real() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("/", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            let v = *REAL(result);
            assert!((v - 3.3333333333333335).abs() < 1e-10);
        }
    }

    #[test]
    fn test_scalar_power() {
        unsafe {
            let a = Rf_ScalarInteger(2);
            let b = Rf_ScalarInteger(10);
            let result = real_binary("^", a, b);
            assert_eq!(*INTEGER(result), 1024);
        }
    }

    #[test]
    fn test_comparison_lt() {
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = binary_compare("<", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_eq() {
        unsafe {
            let a = Rf_ScalarInteger(5);
            let b = Rf_ScalarInteger(5);
            let result = binary_compare("==", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_ne() {
        unsafe {
            let a = Rf_ScalarReal(1.0);
            let b = Rf_ScalarReal(2.0);
            let result = binary_compare("!=", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_scalar_modulo() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("%%", a, b);
            assert_eq!(*INTEGER(result), 1);
        }
    }

    #[test]
    fn test_scalar_integer_division() {
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("%/%", a, b);
            assert_eq!(*INTEGER(result), 3);
        }
    }

    // --- Vector tests ---

    #[test]
    fn test_vector_addition_with_recycling() {
        unsafe {
            // c(1,2,3) + c(10,20) should recycle → c(11, 22, 13)
            let a = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = 2;
            *INTEGER(a).add(2) = 3;

            let b = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
            *INTEGER(b).add(0) = 10;
            *INTEGER(b).add(1) = 20;

            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 3);
            assert_eq!(*INTEGER(result).add(0), 11); // 1+10
            assert_eq!(*INTEGER(result).add(1), 22); // 2+20
            assert_eq!(*INTEGER(result).add(2), 13); // 3+10 (recycled)
        }
    }

    #[test]
    fn test_vector_comparison() {
        unsafe {
            // c(1,2,3) > c(2,1,2) → c(FALSE, TRUE, TRUE)
            let a = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = 2;
            *INTEGER(a).add(2) = 3;

            let b = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            *INTEGER(b).add(0) = 2;
            *INTEGER(b).add(1) = 1;
            *INTEGER(b).add(2) = 2;

            let result = binary_compare(">", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP.0);
            assert_eq!(LENGTH(result), 3);
            assert_eq!(*LOGICAL(result).add(0), FALSE); // 1 > 2
            assert_eq!(*LOGICAL(result).add(1), TRUE); // 2 > 1
            assert_eq!(*LOGICAL(result).add(2), TRUE); // 3 > 2
        }
    }

    #[test]
    fn test_vector_na_propagation() {
        unsafe {
            let a = Rf_ScalarInteger(NA_INTEGER);
            let b = Rf_ScalarInteger(5);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(result), NA_INTEGER);
        }
    }

    #[test]
    fn test_scalar_plus_vector() {
        unsafe {
            // 1 + c(10, 20, 30) → c(11, 21, 31)
            let a = Rf_ScalarInteger(1);
            let b = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            *INTEGER(b).add(0) = 10;
            *INTEGER(b).add(1) = 20;
            *INTEGER(b).add(2) = 30;

            let result = real_binary("+", a, b);
            assert_eq!(LENGTH(result), 3);
            assert_eq!(*INTEGER(result).add(0), 11);
            assert_eq!(*INTEGER(result).add(1), 21);
            assert_eq!(*INTEGER(result).add(2), 31);
        }
    }

    #[test]
    fn test_unary_minus_vector() {
        unsafe {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = -5;
            *INTEGER(a).add(2) = 0;

            let result = unary_minus(a);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(result).add(0), -1);
            assert_eq!(*INTEGER(result).add(1), 5);
            assert_eq!(*INTEGER(result).add(2), 0);
        }
    }

    #[test]
    fn test_real_plus_int_produces_real() {
        unsafe {
            let a = Rf_ScalarReal(1.5);
            let b = Rf_ScalarInteger(2);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            assert!((*REAL(result) - 3.5).abs() < 1e-10);
        }
    }
}
