//! Vectorized arithmetic and comparison builtin operations.
#![deny(unsafe_op_in_unsafe_fn)]
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

use crate::sexp::accessors::{CAR, CDR, CHAR, LENGTH, PRINTNAME, STRING_ELT, TAG, TYPEOF};
use crate::sexp::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3,
};
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::numeric::NumericVector;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Vectorized binary arithmetic
// ---------------------------------------------------------------------------

const VECTOR_CANCELLATION_POLL_INTERVAL: R_xlen_t = 1024;

#[inline]
fn poll_vector_cancellation(i: R_xlen_t) {
    if i % VECTOR_CANCELLATION_POLL_INTERVAL == 0 {
        crate::sexp::instance::check_cancellation();
    }
}

/// Apply a real-valued binary operation with recycling.
///
/// Returns REALSXP if either operand is REALSXP, otherwise returns INTSXP/LGLSXP.
/// Integer overflow produces NA_INTEGER.
pub unsafe fn real_binary(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let Some(a) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            return R_NilValue();
        };
        let n = a.recycled_len_with(b);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let use_real = op == "/" || op == "^" || a.needs_real_with(b);
        let integer_overflow_can_warn = matches!(op, "+" | "-" | "*");
        let result_raw = if use_real {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        } else {
            Rf_allocVector3(SEXPTYPE::INTSXP, n)
        };
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a.len(), b.len());
        let mut integer_overflow = false;

        for i in 0..n {
            poll_vector_cancellation(i);
            let x = a.real_at(i);
            let y = b.real_at(i);
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;

            if op == "^" && ((!y_na && y == 0.0) || (!x_na && x == 1.0)) {
                result.set_real_elt(i, 1.0);
                continue;
            }

            if x_na || y_na {
                if use_real {
                    result.set_real_elt(i, NA_REAL);
                } else {
                    result.set_integer_elt(i, NA_INTEGER);
                }
                continue;
            }

            let val = binary_arithmetic_value(op, x, y);

            if use_real {
                result.set_real_elt(i, val);
            } else {
                // Integer path: check for overflow
                if val.is_finite()
                    && val == val.floor()
                    && val >= i32::MIN as f64
                    && val <= i32::MAX as f64
                {
                    let ival = val as i32;
                    result.set_integer_elt(i, ival);
                } else {
                    integer_overflow |= integer_overflow_can_warn;
                    result.set_integer_elt(i, NA_INTEGER);
                }
            }
        }

        if integer_overflow {
            warn_simple("NAs produced by integer overflow");
        }
        propagate_binary_vector_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

#[inline]
fn binary_arithmetic_value(op: &str, x: f64, y: f64) -> f64 {
    match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => x / y,
        "^" => libm::pow(x, y),
        "%%" => crate::mainutils::arithmetic::myfmod(x, y),
        "%/%" => crate::mainutils::arithmetic::myfloor(x, y),
        _ => NA_REAL,
    }
}

/// Vectorized binary comparison with recycling.
///
/// Always returns LGLSXP of the result length.
unsafe fn binary_compare(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let Some(a) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            return R_NilValue();
        };
        let n = a.recycled_len_with(b);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a.len(), b.len());

        let use_real = a.needs_real_with(b);

        for i in 0..n {
            poll_vector_cancellation(i);
            let (x_na, y_na, cmp): (bool, bool, bool) = if use_real {
                let x = a.real_at(i);
                let y = b.real_at(i);
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
                let x = a.int_at(i);
                let y = b.int_at(i);
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

            let value = if x_na || y_na {
                NA_LOGICAL
            } else if cmp {
                TRUE
            } else {
                FALSE
            };
            result.set_logical_elt(i, value);
        }

        propagate_binary_vector_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

pub(super) fn warn_if_non_multiple_recycling(a_len: R_xlen_t, b_len: R_xlen_t) {
    let shorter = a_len.min(b_len);
    let longer = a_len.max(b_len);
    if shorter > 0 && longer % shorter != 0 {
        warn_simple("longer object length is not a multiple of shorter object length");
    }
}

fn warn_simple(message: &str) {
    let formatted = format!("Warning message:\n{message} \n");
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stderr(&formatted);
    } else {
        eprint!("{formatted}");
    }
}

unsafe fn copy_attr_if_present(result: SEXP, source: SEXP, symbol: SEXP) -> bool {
    unsafe {
        let attr = getAttrib(source, symbol);
        if attr.is_null() || attr == R_NilValue() {
            return false;
        }
        let attr = crate::mainutils::duplicate::duplicate(attr);
        setAttrib(result, symbol, attr);
        true
    }
}

unsafe fn copy_dims_if_present(result: SEXP, source: SEXP, result_len: R_xlen_t) -> bool {
    unsafe {
        if source.is_null() || LENGTH(source) as R_xlen_t != result_len {
            return false;
        }
        let dim = getAttrib(source, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            return false;
        }

        let dim = crate::mainutils::duplicate::duplicate(dim);
        setAttrib(result, R_DimSymbol(), dim);
        let dimnames = getAttrib(source, R_DimNamesSymbol());
        if !dimnames.is_null() && dimnames != R_NilValue() {
            let dimnames = crate::mainutils::duplicate::duplicate(dimnames);
            setAttrib(result, R_DimNamesSymbol(), dimnames);
        }
        true
    }
}

pub(super) unsafe fn propagate_binary_vector_attributes(
    result: SEXP,
    a: SEXP,
    b: SEXP,
    result_len: R_xlen_t,
) {
    unsafe {
        if copy_dims_if_present(result, a, result_len)
            || copy_dims_if_present(result, b, result_len)
        {
            return;
        }
        if LENGTH(a) as R_xlen_t == result_len && copy_attr_if_present(result, a, R_NamesSymbol()) {
            return;
        }
        if LENGTH(b) as R_xlen_t == result_len {
            copy_attr_if_present(result, b, R_NamesSymbol());
        }
    }
}

unsafe fn propagate_unary_vector_attributes(result: SEXP, source: SEXP, result_len: R_xlen_t) {
    unsafe {
        if copy_dims_if_present(result, source, result_len) {
            return;
        }
        if LENGTH(source) as R_xlen_t == result_len {
            copy_attr_if_present(result, source, R_NamesSymbol());
        }
    }
}

// ---------------------------------------------------------------------------
// Unary operations (vectorized)
// ---------------------------------------------------------------------------

/// Apply a unary real function element-wise to a numeric vector.
unsafe fn math1_vec(sa: SEXP, f: fn(f64) -> f64) -> SEXP {
    unsafe {
        let Some(x) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let n = x.len();
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);

        for i in 0..n {
            poll_vector_cancellation(i);
            let value = x.real_at(i);
            if value.to_bits() == R_NA_BIT_PATTERN {
                result.set_real_elt(i, NA_REAL);
            } else {
                let r = f(value);
                // Preserve incoming NaN (don't replace with NA_REAL)
                if r.is_nan() && !value.is_nan() {
                    // Newly produced NaN from valid input → leave as NaN
                }
                result.set_real_elt(i, r);
            }
        }

        propagate_unary_vector_attributes(result_raw, sa, n);
        result_raw
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
                if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
                    return super::complex_arith::complex_binary(op_name, a, b);
                }
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
                if TYPEOF(a) == SEXPTYPE::STRSXP && TYPEOF(b) == SEXPTYPE::STRSXP {
                    return character_compare(op_name, a, b);
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
        let Some(input) = NumericVector::from_raw(x) else {
            return R_NilValue();
        };
        let n = input.len();
        let result_type = if input.typeof_() == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result_raw = Rf_allocVector3(result_type, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);

        if result_type == SEXPTYPE::REALSXP {
            for i in 0..n {
                result.set_real_elt(i, -input.real_at(i));
            }
        } else {
            for i in 0..n {
                let value = input.int_at(i);
                let negated = if value == NA_INTEGER {
                    NA_INTEGER
                } else {
                    -value
                };
                result.set_integer_elt(i, negated);
            }
        }

        propagate_unary_vector_attributes(result_raw, x, n);
        result_raw
    }
}

unsafe fn character_compare(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let Some(a) = Sexp::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = Sexp::from_raw(sb) else {
            return R_NilValue();
        };
        let a_len = a.len();
        let b_len = b.len();
        let n = match (a_len, b_len) {
            (0, _) | (_, 0) => 0,
            _ => a_len.max(b_len),
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a_len, b_len);

        for i in 0..n {
            let ai = i % a_len;
            let bi = i % b_len;
            let ac = STRING_ELT(sa, ai);
            let bc = STRING_ELT(sb, bi);
            let value = if ac.is_null() || bc.is_null() || ac == R_NaString() || bc == R_NaString()
            {
                NA_LOGICAL
            } else {
                let av = Sexp::from_raw(ac).and_then(|s| s.try_as_str().ok());
                let bv = Sexp::from_raw(bc).and_then(|s| s.try_as_str().ok());
                match (av, bv) {
                    (Some(av), Some(bv)) if compare_strings(op, av, bv) => TRUE,
                    (Some(_), Some(_)) => FALSE,
                    _ => NA_LOGICAL,
                }
            };
            result.set_logical_elt(i, value);
        }

        propagate_binary_vector_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

fn compare_strings(op: &str, a: &str, b: &str) -> bool {
    match op {
        "<" => a < b,
        ">" => a > b,
        "<=" => a <= b,
        ">=" => a >= b,
        "==" => a == b,
        "!=" => a != b,
        _ => false,
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
        if TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return match op_name {
                "sqrt" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_sqrt)
                }
                "log" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_log)
                }
                "exp" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_exp)
                }
                "sinh" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_sinh)
                }
                "cosh" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_cosh)
                }
                "tanh" => {
                    super::complex_arith::complex_unary_vec(x, super::complex_arith::complex_tanh)
                }
                _ => R_NilValue(),
            };
        }

        let f: fn(f64) -> f64 = match op_name {
            "abs" => f64::abs,
            "sqrt" => |v: f64| if v < 0.0 { f64::NAN } else { libm::sqrt(v) },
            "log" => |v: f64| libm::log(v),
            "log2" => |v: f64| libm::log2(v),
            "log10" => |v: f64| libm::log10(v),
            "exp" => |v: f64| libm::exp(v),
            "sinh" => f64::sinh,
            "cosh" => f64::cosh,
            "tanh" => f64::tanh,
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
        let input_is_int = Sexp::from_raw(x)
            .is_some_and(|input| matches!(input.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP));

        if prefer_int || input_is_int {
            let _result_guard = protect(result);
            let Some(result_vec) = NumericVector::from_raw(result) else {
                return result;
            };
            let n = result_vec.len();
            let mut all_int = true;
            for i in 0..n {
                let v = result_vec.real_at(i);
                if v.to_bits() == R_NA_BIT_PATTERN {
                    continue;
                }
                if !v.is_finite() || v != v.floor() || v < i32::MIN as f64 || v > i32::MAX as f64 {
                    all_int = false;
                    break;
                }
            }
            if all_int {
                let iresult_raw = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let Some(iresult) = Sexp::from_raw(iresult_raw) else {
                    return result;
                };
                let _iresult_guard = protect(iresult_raw);
                for i in 0..n {
                    let v = result_vec.real_at(i);
                    let value = if v.to_bits() == R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        v as i32
                    };
                    iresult.set_integer_elt(i, value);
                }
                propagate_unary_vector_attributes(iresult_raw, x, n);
                return iresult_raw;
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

        let mut na_rm = false;

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                na_rm = logical_arg_is_true(CAR(current));
                break;
            }
            current = CDR(current);
        }

        // Collect all values from all data arguments.
        let mut vals: Vec<f64> = Vec::new();
        let mut has_na = false;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                current = CDR(current);
                continue;
            }

            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                if let Some(vector) = NumericVector::from_raw(arg) {
                    for i in 0..vector.len() {
                        let value = vector.real_at(i);
                        if value.to_bits() == R_NA_BIT_PATTERN {
                            if !na_rm {
                                has_na = true;
                                vals.push(value);
                            }
                            continue;
                        }
                        vals.push(value);
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
                    let v = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
                    if let Some(result) = Sexp::from_raw(v) {
                        result.set_real_elt(0, NA_REAL);
                        result.set_real_elt(1, NA_REAL);
                    }
                    return v;
                }
                let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let v = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
                if let Some(result) = Sexp::from_raw(v) {
                    result.set_real_elt(0, lo);
                    result.set_real_elt(1, hi);
                }
                v
            }
            _ => R_NilValue(),
        }
    }
}

fn tag_name_is(tag: SEXP, expected: &str) -> bool {
    unsafe {
        if tag.is_null() {
            return false;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return false;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .is_ok_and(|name| name == expected)
    }
}

unsafe fn logical_arg_is_true(x: SEXP) -> bool {
    unsafe {
        let Some(arg) = NumericVector::from_raw(x) else {
            return false;
        };
        if arg.len() == 0 {
            return false;
        }
        match arg.typeof_() {
            SEXPTYPE::LGLSXP => arg.int_at(0) == TRUE,
            SEXPTYPE::INTSXP => {
                let value = arg.int_at(0);
                value != 0 && value != NA_INTEGER
            }
            SEXPTYPE::REALSXP => {
                let value = arg.real_at(0);
                value != 0.0 && value.to_bits() != R_NA_BIT_PATTERN && !value.is_nan()
            }
            _ => false,
        }
    }
}

/// Handle `mean(x, na.rm = FALSE)` for atomic numeric vectors.
///
/// Full R reaches this through `mean.default`; the embedded evaluator keeps it
/// as a primitive while preserving the C-level numeric behavior.
pub unsafe fn do_mean(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = R_NilValue();
        let mut na_rm = false;

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if tag_name_is(TAG(current), "na.rm") {
                na_rm = logical_arg_is_true(arg);
            } else if x == R_NilValue() {
                x = arg;
            }
            current = CDR(current);
        }

        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(f64::NAN);
        }

        let Some(vector) = NumericVector::from_raw(x) else {
            return Rf_ScalarReal(f64::NAN);
        };

        let mut total = 0.0;
        let mut count = 0usize;
        for i in 0..vector.len() {
            let value = vector.real_at(i);
            let is_na = value.to_bits() == R_NA_BIT_PATTERN;
            let is_nan = value.is_nan() && !is_na;
            if is_na || is_nan {
                if na_rm {
                    continue;
                }
                return Rf_ScalarReal(if is_na { NA_REAL } else { f64::NAN });
            }
            total += value;
            count += 1;
        }

        if count == 0 {
            Rf_ScalarReal(f64::NAN)
        } else {
            Rf_ScalarReal(total / count as f64)
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
        let Some(x) = Sexp::from_raw(x) else {
            return Rf_ScalarLogical(FALSE);
        };
        let t = x.typeof_();
        let result = match op_name {
            "is.numeric" => t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP,
            "is.integer" => t == SEXPTYPE::INTSXP,
            "is.double" => t == SEXPTYPE::REALSXP,
            "is.logical" => t == SEXPTYPE::LGLSXP,
            "is.character" => t == SEXPTYPE::STRSXP,
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
        if TYPEOF(fun_sym) != SEXPTYPE::SYMSXP {
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
                ":" => ":",
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
                "sinh" => "sinh",
                "cosh" => "cosh",
                "tanh" => "tanh",
                "ceiling" => "ceiling",
                "floor" => "floor",
                "trunc" => "trunc",
                "round" => "round",
                "sign" => "sign",
                "length" => "length",
                "sum" => "sum",
                "mean" => "mean",
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
/// Creates session-owned BUILTINSXP nodes for each operator and binds them as
/// symbols in the base environment.
pub unsafe fn register_arithmetic_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::Rf_cons;

        let all_ops = [
            "+",
            "-",
            "*",
            "/",
            "^",
            "%%",
            "%/%",
            ":",
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
            "mean",
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
        for op_name in all_ops {
            let prim = super::primitive::make_primitive_binding(op_name, SEXPTYPE::BUILTINSXP);
            let sym = Rf_install(CString::new(op_name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

pub unsafe fn register_special_forms(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::Rf_cons;

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
            "on.exit",
            "$",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for op_name in special_forms {
            let prim = super::primitive::make_primitive_binding(op_name, SEXPTYPE::SPECIALSXP);
            let sym = Rf_install(CString::new(op_name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
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
    use crate::sexp::accessors::{INTEGER, LENGTH, LOGICAL, REAL, TYPEOF};
    use crate::sexp::context::RSignal;
    use crate::sexp::session::{CancellationToken, RSession};

    fn expect_cancelled<F>(f: F)
    where
        F: FnOnce(),
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let Err(payload) = result else {
            panic!("expected cancellation");
        };
        let Some(RSignal::Error { message }) = payload.downcast_ref::<RSignal>() else {
            panic!("expected RSignal::Error payload");
        };
        assert_eq!(message, "operation cancelled");
    }

    #[test]
    fn test_scalar_addition() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 3);
        }
    }

    #[test]
    fn test_real_addition() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarReal(1.5);
            let b = Rf_ScalarReal(2.5);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            let v = *REAL(result);
            assert!((v - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_scalar_multiplication() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(3);
            let b = Rf_ScalarInteger(4);
            let result = real_binary("*", a, b);
            assert_eq!(*INTEGER(result), 12);
        }
    }

    #[test]
    fn test_division_produces_real() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("/", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            let v = *REAL(result);
            assert!((v - 3.3333333333333335).abs() < 1e-10);
        }
    }

    #[test]
    fn test_division_by_zero_matches_r_real_semantics() {
        let _session = RSession::new();
        unsafe {
            let zero = Rf_ScalarInteger(0);

            let pos = real_binary("/", Rf_ScalarInteger(1), zero);
            assert_eq!(TYPEOF(pos), SEXPTYPE::REALSXP);
            assert!((*REAL(pos)).is_infinite());
            assert!((*REAL(pos)).is_sign_positive());

            let neg = real_binary("/", Rf_ScalarInteger(-1), zero);
            assert_eq!(TYPEOF(neg), SEXPTYPE::REALSXP);
            assert!((*REAL(neg)).is_infinite());
            assert!((*REAL(neg)).is_sign_negative());

            let nan = real_binary("/", Rf_ScalarInteger(0), zero);
            assert_eq!(TYPEOF(nan), SEXPTYPE::REALSXP);
            assert!((*REAL(nan)).is_nan());
            assert_ne!((*REAL(nan)).to_bits(), NA_REAL.to_bits());
        }
    }

    #[test]
    fn test_scalar_power() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(2);
            let b = Rf_ScalarInteger(10);
            let result = real_binary("^", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(result), 1024.0);
        }
    }

    #[test]
    fn test_comparison_lt() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            let result = binary_compare("<", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_eq() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(5);
            let b = Rf_ScalarInteger(5);
            let result = binary_compare("==", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_comparison_ne() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarReal(1.0);
            let b = Rf_ScalarReal(2.0);
            let result = binary_compare("!=", a, b);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_scalar_modulo() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("%%", a, b);
            assert_eq!(*INTEGER(result), 1);
        }
    }

    #[test]
    fn test_scalar_integer_division() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(10);
            let b = Rf_ScalarInteger(3);
            let result = real_binary("%/%", a, b);
            assert_eq!(*INTEGER(result), 3);
        }
    }

    #[test]
    fn test_modulo_and_floor_division_by_zero_match_r_shape() {
        let _session = RSession::new();
        unsafe {
            let int_zero = Rf_ScalarInteger(0);

            let int_mod = real_binary("%%", Rf_ScalarInteger(1), int_zero);
            assert_eq!(TYPEOF(int_mod), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(int_mod), NA_INTEGER);

            let int_floor_div = real_binary("%/%", Rf_ScalarInteger(1), int_zero);
            assert_eq!(TYPEOF(int_floor_div), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(int_floor_div), NA_INTEGER);

            let real_zero = Rf_ScalarReal(0.0);

            let real_mod = real_binary("%%", Rf_ScalarReal(1.0), real_zero);
            assert_eq!(TYPEOF(real_mod), SEXPTYPE::REALSXP);
            assert!((*REAL(real_mod)).is_nan());
            assert_ne!((*REAL(real_mod)).to_bits(), NA_REAL.to_bits());

            let real_floor_div = real_binary("%/%", Rf_ScalarReal(-1.0), real_zero);
            assert_eq!(TYPEOF(real_floor_div), SEXPTYPE::REALSXP);
            assert!((*REAL(real_floor_div)).is_infinite());
            assert!((*REAL(real_floor_div)).is_sign_negative());
        }
    }

    // --- Vector tests ---

    #[test]
    fn test_vector_addition_with_recycling() {
        let _session = RSession::new();
        unsafe {
            // c(1,2,3) + c(10,20) should recycle → c(11, 22, 13)
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = 2;
            *INTEGER(a).add(2) = 3;

            let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(b).add(0) = 10;
            *INTEGER(b).add(1) = 20;

            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 3);
            assert_eq!(*INTEGER(result).add(0), 11); // 1+10
            assert_eq!(*INTEGER(result).add(1), 22); // 2+20
            assert_eq!(*INTEGER(result).add(2), 13); // 3+10 (recycled)
        }
    }

    #[test]
    fn test_vector_comparison() {
        let _session = RSession::new();
        unsafe {
            // c(1,2,3) > c(2,1,2) → c(FALSE, TRUE, TRUE)
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = 2;
            *INTEGER(a).add(2) = 3;

            let b = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            *INTEGER(b).add(0) = 2;
            *INTEGER(b).add(1) = 1;
            *INTEGER(b).add(2) = 2;

            let result = binary_compare(">", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
            assert_eq!(LENGTH(result), 3);
            assert_eq!(*LOGICAL(result).add(0), FALSE); // 1 > 2
            assert_eq!(*LOGICAL(result).add(1), TRUE); // 2 > 1
            assert_eq!(*LOGICAL(result).add(2), TRUE); // 3 > 2
        }
    }

    #[test]
    fn test_vector_arithmetic_polls_cancellation() {
        let mut session = RSession::new();
        session.set_cancellation_token(Some(CancellationToken::cancelled()));

        unsafe {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
            let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
            for i in 0..2048 {
                *INTEGER(a).add(i) = i as i32;
                *INTEGER(b).add(i) = 1;
            }

            expect_cancelled(|| {
                let _ = real_binary("+", a, b);
            });
        }
    }

    #[test]
    fn test_vector_compare_polls_cancellation() {
        let mut session = RSession::new();
        session.set_cancellation_token(Some(CancellationToken::cancelled()));

        unsafe {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
            let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
            for i in 0..2048 {
                *INTEGER(a).add(i) = i as i32;
                *INTEGER(b).add(i) = i as i32;
            }

            expect_cancelled(|| {
                let _ = binary_compare("==", a, b);
            });
        }
    }

    #[test]
    fn test_unary_math_polls_cancellation() {
        let mut session = RSession::new();
        session.set_cancellation_token(Some(CancellationToken::cancelled()));

        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::REALSXP, 2048);
            for i in 0..2048 {
                *REAL(x).add(i) = i as f64;
            }

            expect_cancelled(|| {
                let _ = math1_vec(x, f64::sqrt);
            });
        }
    }

    #[test]
    fn test_vector_na_propagation() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(NA_INTEGER);
            let b = Rf_ScalarInteger(5);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), NA_INTEGER);
        }
    }

    #[test]
    fn test_scalar_plus_vector() {
        let _session = RSession::new();
        unsafe {
            // 1 + c(10, 20, 30) → c(11, 21, 31)
            let a = Rf_ScalarInteger(1);
            let b = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
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
        let _session = RSession::new();
        unsafe {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            *INTEGER(a).add(0) = 1;
            *INTEGER(a).add(1) = -5;
            *INTEGER(a).add(2) = 0;

            let result = unary_minus(a);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result).add(0), -1);
            assert_eq!(*INTEGER(result).add(1), 5);
            assert_eq!(*INTEGER(result).add(2), 0);
        }
    }

    #[test]
    fn test_real_plus_int_produces_real() {
        let _session = RSession::new();
        unsafe {
            let a = Rf_ScalarReal(1.5);
            let b = Rf_ScalarInteger(2);
            let result = real_binary("+", a, b);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert!((*REAL(result) - 3.5).abs() < 1e-10);
        }
    }
}
