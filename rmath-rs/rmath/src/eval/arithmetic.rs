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

use crate::sexp::accessors::{
    CAR, CDR, CHAR, LENGTH, PRINTNAME, SET_STRING_ELT, STRING_ELT, TAG, TYPEOF,
};
use crate::sexp::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::constructors::{
    Rf_ScalarComplex, Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_mkChar,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE,
    TRUE,
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

fn arithmetic_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
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

unsafe fn set_single_string_class(result: SEXP, class_name: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _class_guard = protect(class);
        let cstr = CString::new(class_name).unwrap_or_default();
        SET_STRING_ELT(class, 0, Rf_mkChar(cstr.as_ptr()));
        setAttrib(result, Rf_install(c"class".as_ptr()), class);
    }
}

unsafe fn set_string_attribute(result: SEXP, name: &str, value: &str) {
    unsafe {
        let attr = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if attr.is_null() {
            return;
        }
        let _attr_guard = protect(attr);
        let cvalue = CString::new(value).unwrap_or_default();
        SET_STRING_ELT(attr, 0, Rf_mkChar(cvalue.as_ptr()));
        let cname = CString::new(name).unwrap_or_default();
        setAttrib(result, Rf_install(cname.as_ptr()), attr);
    }
}

unsafe fn date_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_date = crate::mainutils::essentials::sexp_has_class(a, "Date");
        let b_is_date = crate::mainutils::essentials::sexp_has_class(b, "Date");
        if !a_is_date && !b_is_date {
            return None;
        }

        match op {
            "+" if a_is_date && b_is_date => {
                arithmetic_error("binary + is not defined for \"Date\" objects");
            }
            "+" if a_is_date || b_is_date => {
                let result = real_binary(op, a, b);
                set_single_string_class(result, "Date");
                Some(result)
            }
            "-" if a_is_date && b_is_date => {
                let result = real_binary(op, a, b);
                set_string_attribute(result, "units", "days");
                set_single_string_class(result, "difftime");
                Some(result)
            }
            "-" if a_is_date => {
                let result = real_binary(op, a, b);
                set_single_string_class(result, "Date");
                Some(result)
            }
            "-" if b_is_date => {
                arithmetic_error("can only subtract from \"Date\" objects");
            }
            "*" | "/" | "^" | "%%" | "%/%" => {
                arithmetic_error(format!("{op} not defined for \"Date\" objects"));
            }
            _ => None,
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
                if let Some(result) = date_binary_arithmetic(op_name, a, b) {
                    return result;
                }
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
        let op = SummaryOp::from_name(get_op_name(call));
        let na_rm = parse_summary_na_rm(args);
        let shape = scan_summary_shape(args, op);

        match op {
            SummaryOp::Sum => eval_sum(args, shape, na_rm),
            SummaryOp::Prod => eval_prod(args, shape, na_rm),
            SummaryOp::Min => eval_minmax(args, shape, na_rm, SummaryOp::Min),
            SummaryOp::Max => eval_minmax(args, shape, na_rm, SummaryOp::Max),
            SummaryOp::Range => eval_range(args, shape, na_rm),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SummaryOp {
    Sum,
    Min,
    Max,
    Prod,
    Range,
}

impl SummaryOp {
    fn from_name(name: &str) -> Self {
        match name {
            "sum" => Self::Sum,
            "min" => Self::Min,
            "max" => Self::Max,
            "prod" => Self::Prod,
            "range" => Self::Range,
            _ => summary_error(format!("unsupported summary primitive '{name}'")),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct SummaryShape {
    saw_real: bool,
    saw_complex: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MissingKind {
    NaN,
    NA,
}

fn merge_missing(current: Option<MissingKind>, next: MissingKind) -> Option<MissingKind> {
    match (current, next) {
        (Some(MissingKind::NA), _) | (_, MissingKind::NA) => Some(MissingKind::NA),
        _ => Some(MissingKind::NaN),
    }
}

fn real_missing(value: f64) -> Option<MissingKind> {
    if !value.is_nan() {
        None
    } else if value.to_bits() == R_NA_BIT_PATTERN {
        Some(MissingKind::NA)
    } else {
        Some(MissingKind::NaN)
    }
}

fn missing_real(kind: MissingKind) -> f64 {
    match kind {
        MissingKind::NA => NA_REAL,
        MissingKind::NaN => f64::NAN,
    }
}

fn complex_missing(value: Rcomplex) -> bool {
    value.r.is_nan() || value.i.is_nan()
}

unsafe fn parse_summary_na_rm(args: SEXP) -> bool {
    unsafe {
        let mut na_rm = false;
        let mut seen = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                if seen {
                    summary_error("formal argument \"na.rm\" matched by multiple actual arguments");
                }
                seen = true;
                na_rm = summary_logical_arg(CAR(current));
            }
            current = CDR(current);
        }
        na_rm
    }
}

unsafe fn summary_logical_arg(x: SEXP) -> bool {
    unsafe {
        let Some(arg) = NumericVector::from_raw(x) else {
            summary_error("invalid 'na.rm' value");
        };
        if arg.len() == 0 {
            summary_error("invalid 'na.rm' value");
        }
        match arg.typeof_() {
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => {
                let value = arg.int_at(0);
                if value == NA_INTEGER {
                    summary_error("invalid 'na.rm' value");
                }
                value != 0
            }
            SEXPTYPE::REALSXP => {
                let value = arg.real_at(0);
                if value.is_nan() {
                    summary_error("invalid 'na.rm' value");
                }
                value != 0.0
            }
            _ => summary_error("invalid 'na.rm' value"),
        }
    }
}

unsafe fn scan_summary_shape(args: SEXP, op: SummaryOp) -> SummaryShape {
    unsafe {
        let mut shape = SummaryShape::default();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            if value.is_null() || value == R_NilValue() {
                current = CDR(current);
                continue;
            }
            let value_type = Sexp::from_raw(value)
                .map(|value| value.typeof_())
                .unwrap_or(SEXPTYPE::NILSXP);
            match value_type {
                t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {}
                t if t == SEXPTYPE::REALSXP => shape.saw_real = true,
                t if t == SEXPTYPE::CPLXSXP => {
                    if matches!(op, SummaryOp::Min | SummaryOp::Max | SummaryOp::Range) {
                        summary_error("invalid 'type' (complex) of argument");
                    }
                    shape.saw_complex = true;
                }
                _ => summary_error("invalid 'type' of argument"),
            }
            current = CDR(current);
        }
        shape
    }
}

unsafe fn eval_sum(args: SEXP, shape: SummaryShape, na_rm: bool) -> SEXP {
    unsafe {
        let mut int_total: i64 = 0;
        let mut real_total = 0.0;
        let mut complex_total = Rcomplex { r: 0.0, i: 0.0 };
        let mut missing = None;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.int_at(i);
                        if item == NA_INTEGER {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        int_total += item as i64;
                        real_total += item as f64;
                        complex_total.r += item as f64;
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.real_at(i);
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        real_total += item;
                        complex_total.r += item;
                    }
                }
                Some(SEXPTYPE::CPLXSXP) => {
                    let vector = Sexp::from_raw_unchecked(value);
                    for (i, item) in vector.iter_complex().enumerate() {
                        poll_vector_cancellation(i as R_xlen_t);
                        if complex_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        complex_total.r += item.r;
                        complex_total.i += item.i;
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if shape.saw_complex {
            if missing.is_some() {
                return Rf_ScalarComplex(Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                });
            }
            return Rf_ScalarComplex(complex_total);
        }
        if shape.saw_real {
            if let Some(kind) = missing {
                return Rf_ScalarReal(missing_real(kind));
            }
            return Rf_ScalarReal(real_total);
        }
        if missing.is_some() || int_total > i32::MAX as i64 || int_total < i32::MIN as i64 {
            return Rf_ScalarInteger(NA_INTEGER);
        }
        Rf_ScalarInteger(int_total as i32)
    }
}

unsafe fn eval_prod(args: SEXP, shape: SummaryShape, na_rm: bool) -> SEXP {
    unsafe {
        let mut real_total = 1.0;
        let mut complex_total = Rcomplex { r: 1.0, i: 0.0 };
        let mut missing = None;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.int_at(i);
                        if item == NA_INTEGER {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        let item = item as f64;
                        real_total *= item;
                        complex_total.r *= item;
                        complex_total.i *= item;
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.real_at(i);
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        real_total *= item;
                        complex_total.r *= item;
                        complex_total.i *= item;
                    }
                }
                Some(SEXPTYPE::CPLXSXP) => {
                    let vector = Sexp::from_raw_unchecked(value);
                    for (i, item) in vector.iter_complex().enumerate() {
                        poll_vector_cancellation(i as R_xlen_t);
                        if complex_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        let old = complex_total;
                        complex_total.r = old.r * item.r - old.i * item.i;
                        complex_total.i = old.r * item.i + old.i * item.r;
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if shape.saw_complex {
            if missing.is_some() {
                return Rf_ScalarComplex(Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                });
            }
            Rf_ScalarComplex(complex_total)
        } else if let Some(kind) = missing {
            Rf_ScalarReal(missing_real(kind))
        } else {
            Rf_ScalarReal(real_total)
        }
    }
}

unsafe fn eval_minmax(args: SEXP, shape: SummaryShape, na_rm: bool, op: SummaryOp) -> SEXP {
    unsafe {
        let mut seen = false;
        let mut int_best = if op == SummaryOp::Min {
            i32::MAX
        } else {
            i32::MIN
        };
        let mut real_best = if op == SummaryOp::Min {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
        let mut missing = None;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.int_at(i);
                        if item == NA_INTEGER {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        seen = true;
                        if op == SummaryOp::Min {
                            int_best = int_best.min(item);
                            real_best = real_best.min(item as f64);
                        } else {
                            int_best = int_best.max(item);
                            real_best = real_best.max(item as f64);
                        }
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.len() {
                        poll_vector_cancellation(i);
                        let item = vector.real_at(i);
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        seen = true;
                        if op == SummaryOp::Min {
                            real_best = real_best.min(item);
                        } else {
                            real_best = real_best.max(item);
                        }
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if let Some(kind) = missing {
            if shape.saw_real {
                return Rf_ScalarReal(missing_real(kind));
            }
            return Rf_ScalarInteger(NA_INTEGER);
        }
        if !seen {
            warn_empty_minmax(op);
            return Rf_ScalarReal(if op == SummaryOp::Min {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            });
        }
        if shape.saw_real {
            Rf_ScalarReal(real_best)
        } else {
            Rf_ScalarInteger(int_best)
        }
    }
}

unsafe fn eval_range(args: SEXP, shape: SummaryShape, na_rm: bool) -> SEXP {
    unsafe {
        let min = eval_minmax(args, shape, na_rm, SummaryOp::Min);
        let max = eval_minmax(args, shape, na_rm, SummaryOp::Max);
        let min_value = Sexp::from_raw_unchecked(min);
        let max_value = Sexp::from_raw_unchecked(max);
        let result_type = if TYPEOF(min) == SEXPTYPE::REALSXP || TYPEOF(max) == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result = Rf_allocVector3(result_type, 2);
        let result_view = Sexp::from_raw_unchecked(result);
        match result_type {
            SEXPTYPE::REALSXP => {
                result_view.set_real_elt(
                    0,
                    min_value
                        .real_elt(0)
                        .or_else(|| min_value.integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
                result_view.set_real_elt(
                    1,
                    max_value
                        .real_elt(0)
                        .or_else(|| max_value.integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
            }
            SEXPTYPE::INTSXP => {
                result_view.set_integer_elt(0, min_value.integer_elt(0).unwrap_or(NA_INTEGER));
                result_view.set_integer_elt(1, max_value.integer_elt(0).unwrap_or(NA_INTEGER));
            }
            _ => {}
        }
        result
    }
}

fn warn_empty_minmax(op: SummaryOp) {
    match op {
        SummaryOp::Min => {
            warn_simple("no non-missing arguments to min; returning Inf");
        }
        SummaryOp::Max => {
            warn_simple("no non-missing arguments to max; returning -Inf");
        }
        _ => {}
    }
}

fn summary_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
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
            "switch",
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
