//! Vectorized arithmetic and comparison builtin operations.
#![deny(unsafe_op_in_unsafe_fn)]
// Translated C loops write result elements through the SexpMut borrow guard
// (sexp::object); the guard is the sole mutation path.
//!
//! These handle the core numeric operators (+, -, *, /, ^, %%, %/%),
//! comparison operators (<, >, <=, >=, ==, !=), and unary operators (!, -).
//!
//! In R, these are "builtin" functions — arguments are evaluated before
//! the function is called, unlike "special" forms.
//!
//! All binary operations support R's recycling rule: shorter vectors are
//! recycled to match the length of the longer operand.

use std::ffi::{CStr, CString};

use crate::sexp::accessors::{
    CAR, CDR, CHAR, COMPLEX_ELT, INTEGER_ELT, LENGTH, LOGICAL_ELT, PRINTNAME, RAW_ELT, REAL_ELT,
    SET_COMPLEX_ELT, SET_INTEGER_ELT, SET_LOGICAL_ELT, SET_RAW_ELT, SET_REAL_ELT, SET_STRING_ELT,
    SET_VECTOR_ELT, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::constructors::{
    Rf_ScalarComplex, Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_lang2,
    Rf_lang3, Rf_mkChar,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE,
    TRUE,
};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::numeric::NumericVector;
use crate::sexp::object::{Sexp, SexpMut};
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
        if sa.is_null() || sb.is_null() {
            return R_NilValue();
        }
        // stock arithmetic.c: NULL coerces to a zero-length numeric vector;
        // any other non-numeric operand raises the binary-operator error.
        if sa == R_NilValue() || sb == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        if !is_numeric_operand(sa) || !is_numeric_operand(sb) {
            arithmetic_error("non-numeric argument to binary operator");
        }
        let Some(a) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            return R_NilValue();
        };
        let n = a.clone().recycled_len_with(b.clone());
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let use_real = op == "/" || op == "^" || a.clone().needs_real_with(b.clone());
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
        let mut result_mut = SexpMut::from_owned(result);
        warn_if_non_multiple_recycling(a.clone().len(), b.clone().len());
        let mut integer_overflow = false;

        for i in 0..n {
            poll_vector_cancellation(i);
            let x = a.clone().real_at(i);
            let y = b.clone().real_at(i);
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;

            if op == "^" && ((!y_na && y == 0.0) || (!x_na && x == 1.0)) {
                result_mut.set_real_elt(i, 1.0);
                continue;
            }

            if x_na || y_na {
                if use_real {
                    result_mut.set_real_elt(i, NA_REAL);
                } else {
                    result_mut.set_integer_elt(i, NA_INTEGER);
                }
                continue;
            }

            let val = binary_arithmetic_value(op, x, y);

            if use_real {
                result_mut.set_real_elt(i, val);
            } else {
                // Integer path: check for overflow
                if val.is_finite()
                    && val == val.floor()
                    && val >= i32::MIN as f64
                    && val <= i32::MAX as f64
                {
                    let ival = val as i32;
                    result_mut.set_integer_elt(i, ival);
                } else {
                    integer_overflow |= integer_overflow_can_warn;
                    result_mut.set_integer_elt(i, NA_INTEGER);
                }
            }
        }

        if integer_overflow {
            warn_simple("NAs produced by integer overflow");
        }
        let _ = result_mut.freeze();
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
        "^" => crate::nmath::special::mlutils::R_pow(x, y),
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
            arithmetic_error("comparison of these types is not implemented");
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            arithmetic_error("comparison of these types is not implemented");
        };
        let n = a.clone().recycled_len_with(b.clone());
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        warn_if_non_multiple_recycling(a.clone().len(), b.clone().len());

        let use_real = a.clone().needs_real_with(b.clone());

        for i in 0..n {
            poll_vector_cancellation(i);
            let (x_na, y_na, cmp): (bool, bool, bool) = if use_real {
                let x = a.clone().real_at(i);
                let y = b.clone().real_at(i);
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
                let x = a.clone().int_at(i);
                let y = b.clone().int_at(i);
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
            result_mut.set_logical_elt(i, value);
        }

        let _ = result_mut.freeze();
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

/// Propagate attributes onto a binary-operation result, mirroring upstream
/// `R_binary` (r-source/src/main/arithmetic.c, attribute preservation block).
///
/// Decision table:
/// - If both operands carry `dims` they must be conformable, otherwise a
///   'non-conformable arrays' error is raised; the result takes dims from `x`
///   and dimnames from `x` (falling back to `y`).
/// - If exactly one operand is an array (and the length-1 array recycling
///   special case does not apply, nor is the partner a length-0 non-array),
///   the result inherits that operand's dims/dimnames.
/// - Otherwise (plain vectors) only `names` are propagated, from whichever
///   operand matches the result length (`x` wins).
pub(super) unsafe fn propagate_binary_vector_attributes(
    result: SEXP,
    a: SEXP,
    b: SEXP,
    result_len: R_xlen_t,
) {
    unsafe {
        let nil = R_NilValue();
        let a_dims = getAttrib(a, R_DimSymbol());
        let b_dims = getAttrib(b, R_DimSymbol());
        let a_is_array = !a_dims.is_null() && a_dims != nil;
        let b_is_array = !b_dims.is_null() && b_dims != nil;

        if a_is_array || b_is_array {
            let a_len = LENGTH(a) as R_xlen_t;
            let b_len = LENGTH(b) as R_xlen_t;
            // Upstream strips the dims of a length-1 array whose partner is a
            // vector of a different length (deprecated recycling), so such a
            // combination falls through to the plain-vector treatment below.
            let a_counts = !(a_is_array && a_len == 1 && b_len != 1);
            let b_counts = !(b_is_array && b_len == 1 && a_len != 1);
            // An array paired with a length-0 non-array skips array treatment.
            let a_applies = a_is_array && a_counts && (b_len != 0 || a_len == 0);
            let b_applies = b_is_array && b_counts && (a_len != 0 || b_len == 0);

            if a_applies && b_applies {
                if crate::mainutils::relop::conformable(a, b) == 0 {
                    arithmetic_error("non-conformable arrays");
                }
                let dims = crate::mainutils::duplicate::duplicate(a_dims);
                setAttrib(result, R_DimSymbol(), dims);
                if copy_attr_if_present(result, a, R_DimNamesSymbol()) {
                    return;
                }
                copy_attr_if_present(result, b, R_DimNamesSymbol());
            } else if a_applies {
                copy_dims_if_present(result, a, result_len);
            } else if b_applies {
                copy_dims_if_present(result, b, result_len);
            } else {
                // Array treatment skipped: plain-vector names below.
                propagate_plain_names(result, a, b, result_len);
            }
            return;
        }

        propagate_plain_names(result, a, b, result_len);
    }
}

/// Plain-vector tail of the upstream decision table: names only, `x` wins.
unsafe fn propagate_plain_names(result: SEXP, a: SEXP, b: SEXP, result_len: R_xlen_t) {
    unsafe {
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

unsafe fn set_posixct_attributes_from(result: SEXP, source: SEXP) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            setAttrib(result, Rf_install(c"class".as_ptr()), class);
        }

        let tzone = getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if !tzone.is_null() && tzone != R_NilValue() {
            setAttrib(
                result,
                Rf_install(c"tzone".as_ptr()),
                crate::mainutils::duplicate::duplicate(tzone),
            );
        }
    }
}

unsafe fn string_attribute_value(source: SEXP, name: &CStr) -> Option<String> {
    unsafe {
        let attr = getAttrib(source, Rf_install(name.as_ptr()));
        let attr = Sexp::from_raw(attr)?;
        if attr.clone().typeof_() != SEXPTYPE::STRSXP || attr.clone().len() == 0 {
            return None;
        }
        let value = STRING_ELT(attr.as_raw(), 0);
        if value.is_null() || value == R_NaString() {
            return None;
        }
        CStr::from_ptr(CHAR(value))
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

fn difftime_unit_scale(unit: &str) -> f64 {
    match unit {
        "secs" => 1.0,
        "mins" => 60.0,
        "hours" => 3_600.0,
        "days" => 86_400.0,
        "weeks" => 604_800.0,
        _ => 1.0,
    }
}

unsafe fn difftime_units(source: SEXP) -> String {
    unsafe { string_attribute_value(source, c"units").unwrap_or_else(|| "secs".to_string()) }
}

unsafe fn coerce_difftime_operand(
    source: SEXP,
    target_unit_seconds: f64,
    round_result: bool,
) -> SEXP {
    unsafe {
        if !crate::mainutils::essentials::sexp_has_class(source, "difftime") {
            return source;
        }
        let unit = string_attribute_value(source, c"units").unwrap_or_else(|| "secs".to_string());
        let scale = difftime_unit_scale(&unit) / target_unit_seconds;
        let Some(input) = NumericVector::from_raw(source) else {
            return source;
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, input.clone().len());
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..input.clone().len() {
            let value = input.clone().real_at(i);
            let converted = if value.to_bits() == R_NA_BIT_PATTERN {
                NA_REAL
            } else if round_result {
                (value * scale).round_ties_even()
            } else {
                value * scale
            };
            result_mut.set_real_elt(i, converted);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

unsafe fn coerce_difftime_to_units(source: SEXP, target_units: &str) -> SEXP {
    unsafe {
        let target_unit_seconds = difftime_unit_scale(target_units);
        coerce_difftime_operand(source, target_unit_seconds, false)
    }
}

unsafe fn has_class_attribute(source: SEXP) -> bool {
    unsafe {
        let class = getAttrib(source, Rf_install(c"class".as_ptr()));
        !class.is_null() && class != R_NilValue()
    }
}

unsafe fn coerce_character_comparison_operand(
    source: SEXP,
    parser: fn(&str) -> Option<f64>,
) -> SEXP {
    unsafe {
        if TYPEOF(source) != SEXPTYPE::STRSXP {
            return source;
        }
        let Some(input) = Sexp::from_raw(source) else {
            return R_NilValue();
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, input.clone().len());
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let input_len = input.len();
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..input_len {
            let value = STRING_ELT(source, i);
            let parsed = if value.is_null() || value == R_NaString() {
                NA_REAL
            } else {
                let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                parser(text).unwrap_or_else(|| {
                    arithmetic_error("character string is not in a standard unambiguous format");
                })
            };
            result_mut.set_real_elt(i, parsed);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

unsafe fn difftime_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_difftime = crate::mainutils::essentials::sexp_has_class(a, "difftime");
        let b_is_difftime = crate::mainutils::essentials::sexp_has_class(b, "difftime");
        if !a_is_difftime && !b_is_difftime {
            return None;
        }

        if a_is_difftime && b_is_difftime {
            let a = coerce_difftime_to_units(a, "secs");
            let b = coerce_difftime_to_units(b, "secs");
            let _a_guard = protect(a);
            let _b_guard = protect(b);
            return Some(binary_compare(op, a, b));
        }

        Some(binary_compare(op, a, b))
    }
}

fn parse_posixct_comparison_seconds(text: &str) -> Option<f64> {
    crate::mainutils::essentials::parse_iso_datetime_seconds(text)
}

unsafe fn date_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_date = crate::mainutils::essentials::sexp_has_class(a, "Date");
        let b_is_date = crate::mainutils::essentials::sexp_has_class(b, "Date");
        if !a_is_date && !b_is_date {
            return None;
        }

        let a = coerce_character_comparison_operand(
            a,
            crate::mainutils::essentials::parse_iso_date_days,
        );
        let b = coerce_character_comparison_operand(
            b,
            crate::mainutils::essentials::parse_iso_date_days,
        );
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        Some(binary_compare(op, a, b))
    }
}

unsafe fn posixct_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_posixct = crate::mainutils::essentials::sexp_has_class(a, "POSIXct");
        let b_is_posixct = crate::mainutils::essentials::sexp_has_class(b, "POSIXct");
        if !a_is_posixct && !b_is_posixct {
            return None;
        }

        let a = coerce_character_comparison_operand(a, parse_posixct_comparison_seconds);
        let b = coerce_character_comparison_operand(b, parse_posixct_comparison_seconds);
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        Some(binary_compare(op, a, b))
    }
}

fn auto_difftime_units(seconds: &[f64]) -> (&'static str, f64) {
    let min_abs = seconds
        .iter()
        .copied()
        .filter(|value| value.to_bits() != R_NA_BIT_PATTERN && value.is_finite())
        .map(f64::abs)
        .fold(f64::INFINITY, f64::min);
    if !min_abs.is_finite() || min_abs < 60.0 {
        ("secs", 1.0)
    } else if min_abs < 3_600.0 {
        ("mins", 60.0)
    } else if min_abs < 86_400.0 {
        ("hours", 3_600.0)
    } else {
        ("days", 86_400.0)
    }
}

unsafe fn set_difftime_attributes(result: SEXP, units: &str) {
    unsafe {
        set_string_attribute(result, "units", units);
        set_single_string_class(result, "difftime");
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
                let a = coerce_difftime_operand(a, 86_400.0, true);
                let b = coerce_difftime_operand(b, 86_400.0, true);
                let _a_guard = protect(a);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_single_string_class(result, "Date");
                Some(result)
            }
            "-" if a_is_date && b_is_date => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, "days");
                Some(result)
            }
            "-" if a_is_date => {
                if has_class_attribute(b)
                    && !crate::mainutils::essentials::sexp_has_class(b, "difftime")
                {
                    arithmetic_error("can only subtract numbers from \"Date\" objects");
                }
                let b = coerce_difftime_operand(b, 86_400.0, true);
                let _b_guard = protect(b);
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

unsafe fn difftime_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_difftime = crate::mainutils::essentials::sexp_has_class(a, "difftime");
        let b_is_difftime = crate::mainutils::essentials::sexp_has_class(b, "difftime");
        if !a_is_difftime && !b_is_difftime {
            return None;
        }

        match op {
            "+" | "-" if a_is_difftime && b_is_difftime => {
                let a_units = difftime_units(a);
                let b_units = difftime_units(b);
                let (a, b, result_units) = if a_units == b_units {
                    (a, b, a_units)
                } else {
                    let a = coerce_difftime_to_units(a, "secs");
                    let b = coerce_difftime_to_units(b, "secs");
                    (a, b, "secs".to_string())
                };
                let _a_guard = protect(a);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &result_units);
                Some(result)
            }
            "+" | "-" if a_is_difftime || b_is_difftime => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(if a_is_difftime { a } else { b }));
                Some(result)
            }
            "*" if a_is_difftime && b_is_difftime => {
                arithmetic_error("both arguments of * cannot be \"difftime\" objects");
            }
            "*" if a_is_difftime || b_is_difftime => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(if a_is_difftime { a } else { b }));
                Some(result)
            }
            "/" if b_is_difftime => {
                arithmetic_error("second argument of / cannot be a \"difftime\" object");
            }
            "/" if a_is_difftime => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(a));
                Some(result)
            }
            "^" | "%%" | "%/%" => {
                arithmetic_error(format!("'{op}' not defined for \"difftime\" objects"));
            }
            _ => None,
        }
    }
}

unsafe fn posixct_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_posixct = crate::mainutils::essentials::sexp_has_class(a, "POSIXct");
        let b_is_posixct = crate::mainutils::essentials::sexp_has_class(b, "POSIXct");
        if !a_is_posixct && !b_is_posixct {
            return None;
        }

        match op {
            "+" if a_is_posixct && b_is_posixct => {
                arithmetic_error("binary '+' is not defined for \"POSIXt\" objects");
            }
            "+" if a_is_posixct || b_is_posixct => {
                let posixct_source = if a_is_posixct { a } else { b };
                let a = coerce_difftime_operand(a, 1.0, false);
                let b = coerce_difftime_operand(b, 1.0, false);
                let _a_guard = protect(a);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_posixct_attributes_from(result, posixct_source);
                Some(result)
            }
            "-" if a_is_posixct && b_is_posixct => {
                let result = real_binary(op, a, b);
                let Some(result_sexp) = Sexp::from_raw(result) else {
                    return Some(result);
                };
                let values_len = result_sexp.len();
                let values: Vec<f64> = (0..values_len)
                    .filter_map(|i| result_sexp.try_real_elt(i).ok())
                    .collect();
                let (units, scale) = auto_difftime_units(&values);
                let mut result_mut = SexpMut::from_owned(result_sexp);
                for i in 0..result_mut.len() {
                    if let Ok(value) = result_mut.try_real_elt(i)
                        && value.to_bits() != R_NA_BIT_PATTERN
                    {
                        result_mut.set_real_elt(i, value / scale);
                    }
                }
                let _ = result_mut.freeze();
                set_difftime_attributes(result, units);
                Some(result)
            }
            "-" if a_is_posixct => {
                if has_class_attribute(b)
                    && !crate::mainutils::essentials::sexp_has_class(b, "difftime")
                {
                    arithmetic_error("can only subtract numbers from \"POSIXt\" objects");
                }
                let b = coerce_difftime_operand(b, 1.0, false);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_posixct_attributes_from(result, a);
                Some(result)
            }
            "-" if b_is_posixct => {
                arithmetic_error("can only subtract from \"POSIXt\" objects");
            }
            "*" | "/" | "^" | "%%" | "%/%" => {
                arithmetic_error(format!("'{op}' not defined for \"POSIXt\" objects"));
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
        let n = x.clone().len();
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            poll_vector_cancellation(i);
            let value = x.clone().real_at(i);
            if value.to_bits() == R_NA_BIT_PATTERN {
                result_mut.set_real_elt(i, NA_REAL);
            } else {
                let r = f(value);
                // Preserve incoming NaN (don't replace with NA_REAL)
                if r.is_nan() && !value.is_nan() {
                    // Newly produced NaN from valid input → leave as NaN
                }
                result_mut.set_real_elt(i, r);
            }
        }
        let _ = result_mut.freeze();
        propagate_unary_vector_attributes(result_raw, sa, n);
        result_raw
    }
}


// ---------------------------------------------------------------------------
// Factor operands and stock group-generic behavior
// ---------------------------------------------------------------------------

/// True for logical/integer/real vectors — stock's `isNumeric` minus NULL.
fn is_numeric_operand(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP
    }
}

/// An unordered factor: carries the "factor" class but not "ordered".
fn is_plain_factor(x: SEXP) -> bool {
    unsafe { has_class(x, "factor") && !has_class(x, "ordered") }
}

/// Emit a group-generic warning attributed to the S3 method call the way
/// stock dispatch does (`In Ops.factor(e1, e2) : ...`). `e1`/`e2` are the
/// unevaluated operand expressions of the applied call.
unsafe fn factor_not_meaningful_warning(method: &'static str, message: &str, e1: SEXP, e2: SEXP) {
    unsafe {
        // method is a fixed interned name; Rf_install needs a NUL suffix
        let f = if method == "Ops.ordered" {
            Rf_install(c"Ops.ordered".as_ptr())
        } else {
            Rf_install(c"Ops.factor".as_ptr())
        };
        let method_call = if e2.is_null() || e2 == R_NilValue() {
            Rf_lang2(f, e1)
        } else {
            Rf_lang3(f, e1, e2)
        };
        let _call_guard = protect(method_call);
        let c_msg = std::ffi::CString::new(message).unwrap_or_default();
        crate::mainutils::errors::Rf_warningcall1(method_call, c_msg.as_ptr());
    }
}

/// `rep.int(NA, max(length(e1), if (!missing(e2)) length(e2)))` — the
/// logical NA vector stock Ops.factor/Ops.ordered return for operators
/// that are not meaningful for factors.
unsafe fn factor_na_result(e1: SEXP, e2: SEXP) -> SEXP {
    unsafe {
        let n1 = if e1.is_null() || e1 == R_NilValue() {
            0
        } else {
            XLENGTH(e1)
        };
        let n2 = if e2.is_null() || e2 == R_NilValue() {
            0
        } else {
            XLENGTH(e2)
        };
        let n = n1.max(n2);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            result_mut.set_logical_elt(i, NA_LOGICAL);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

/// Handle factor/ordered operands for arithmetic the way stock's Ops group
/// dispatch does: warn via the method call and return logical NAs. The
/// unevaluated operand expressions come from `call` so the warning is
/// attributed exactly like stock. Returns None when neither operand is a
/// factor.
unsafe fn factor_arith(op: &str, call: SEXP, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_factor = has_class(a, "factor");
        let b_factor = has_class(b, "factor");
        if !a_factor && !b_factor {
            return None;
        }
        let ordered = has_class(a, "ordered") || has_class(b, "ordered");
        let message = if ordered {
            format!("'{op}' is not meaningful for ordered factors")
        } else {
            format!("'{op}' not meaningful for factors")
        };
        let method = if ordered { "Ops.ordered" } else { "Ops.factor" };
        let e1_expr = CAR(CDR(call));
        let e2_expr = CAR(CDR(CDR(call)));
        factor_not_meaningful_warning(method, &message, e1_expr, e2_expr);
        Some(factor_na_result(a, b))
    }
}

/// as.character(<factor>): level strings by 1-based code; NA and 0 codes
/// stay NA.
unsafe fn factor_as_character(f: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(f);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if out.is_null() {
            return R_NilValue();
        }
        let _out_guard = protect(out);
        let levels = getAttrib(f, Rf_install(c"levels".as_ptr()));
        let levels_ok =
            !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP;
        let n_levels = if levels_ok { LENGTH(levels) } else { 0 };
        for i in 0..n {
            let code = INTEGER_ELT(f, i as i32);
            let s = if code == NA_INTEGER || code <= 0 || code as R_xlen_t > n_levels as R_xlen_t {
                R_NaString()
            } else {
                STRING_ELT(levels, (code - 1) as i64)
            };
            SET_STRING_ELT(out, i as i64, s);
        }
        out
    }
}

/// Coerce a relop operand to character following the stock ladder; only
/// logical/integer/real/complex/raw make it (the rest error like stock's
/// final else).
unsafe fn relop_operand_to_character(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            return x;
        }
        if is_numeric_operand(x) || TYPEOF(x) == SEXPTYPE::CPLXSXP || TYPEOF(x) == SEXPTYPE::RAWSXP
        {
            return crate::mainutils::coerce::coerceVector(x, SEXPTYPE::STRSXP.into());
        }
        arithmetic_error("comparison of these types is not implemented")
    }
}

/// stock Ops.factor for unordered factors: `==`/`!=` compare level strings
/// (with the scalar-character fast paths and the level-set check stock
/// applies when both sides are converted), any other operator warns and
/// returns logical NAs. Returns None when neither operand is an unordered
/// factor.
unsafe fn unordered_factor_compare(op: &str, call: SEXP, sa: SEXP, sb: SEXP) -> Option<SEXP> {
    unsafe {
        let a_factor = is_plain_factor(sa);
        let b_factor = is_plain_factor(sb);
        if !a_factor && !b_factor {
            return None;
        }
        if op != "==" && op != "!=" {
            let e1_expr = CAR(CDR(call));
            let e2_expr = CAR(CDR(CDR(call)));
            factor_not_meaningful_warning(
                "Ops.factor",
                &format!("'{op}' not meaningful for factors"),
                e1_expr,
                e2_expr,
            );
            return Some(factor_na_result(sa, sb));
        }

        // stock converts the left factor to its level strings first; when
        // the right side is then a factor with no NA levels and the
        // converted left is a scalar string, the levels table is compared
        // directly (bypassing the level-set check, like stock).
        let scalar_string = |s: SEXP| -> Option<String> {
            if TYPEOF(s) == SEXPTYPE::STRSXP && LENGTH(s) == 1 {
                charsxp_to_string(STRING_ELT(s, 0))
            } else {
                None
            }
        };
        let levels_of = |f: SEXP| -> SEXP { getAttrib(f, Rf_install(c"levels".as_ptr())) };
        let levels_have_no_na = |l: SEXP| -> bool {
            (0..LENGTH(l) as R_xlen_t).all(|i| {
                let s = STRING_ELT(l, i as i64);
                !s.is_null() && s != R_NaString()
            })
        };
        let levels_vec = |l: SEXP| -> Option<Vec<String>> {
            let mut out = Vec::with_capacity(LENGTH(l) as usize);
            for i in 0..LENGTH(l) as R_xlen_t {
                out.push(charsxp_to_string(STRING_ELT(l, i as i64))?);
            }
            Some(out)
        };

        let a_cmp = if a_factor {
            factor_as_character(sa)
        } else {
            relop_operand_to_character(sa)
        };
        if b_factor {
            let b_levels = levels_of(sb);
            if TYPEOF(b_levels) == SEXPTYPE::STRSXP && levels_have_no_na(b_levels) {
                if let Some(other) = scalar_string(a_cmp) {
                    if let Some(levels) = levels_vec(b_levels) {
                        return Some(factor_scalar_string_compare(op, sb, &levels, &other));
                    }
                }
            }
        }
        let b_cmp = if b_factor {
            factor_as_character(sb)
        } else {
            relop_operand_to_character(sb)
        };
        if a_factor && b_factor {
            // stock: when both sides convert, the level sets must match
            let mut la = factor_levels(sa).unwrap_or_default();
            let mut lb = factor_levels(sb).unwrap_or_default();
            la.sort();
            lb.sort();
            if la != lb {
                arithmetic_error("level sets of factors are different");
            }
        }
        Some(character_compare(op, a_cmp, b_cmp))
    }
}

/// stock fastpath: `leq <- (levels == other)` indexed by the factor codes.
unsafe fn factor_scalar_string_compare(op: &str, f: SEXP, levels: &[String], other: &str) -> SEXP {
    unsafe {
        let n = XLENGTH(f);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let code = INTEGER_ELT(f, i as i32);
            let value = if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                NA_LOGICAL
            } else {
                let eq = levels[(code - 1) as usize] == other;
                if (op == "==") == eq { TRUE } else { FALSE }
            };
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

/// Unary minus for complex values (stock `-(z)`).
fn complex_negate(c: Rcomplex) -> Rcomplex {
    Rcomplex { r: -c.r, i: -c.i }
}

/// stock complex_relop: only `==`/`!=` are defined; ordering comparisons
/// error. Equality compares component-wise with NA propagation.
unsafe fn complex_relop(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        if op != "==" && op != "!=" {
            arithmetic_error("invalid comparison with complex values");
        }
        let a = crate::mainutils::coerce::coerceVector(sa, SEXPTYPE::CPLXSXP.into());
        let b = crate::mainutils::coerce::coerceVector(sb, SEXPTYPE::CPLXSXP.into());
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        let n1 = XLENGTH(a);
        let n2 = XLENGTH(b);
        let n = n1.max(n2);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let x = crate::sexp::accessors::COMPLEX_ELT(a, i as i32);
            let y = crate::sexp::accessors::COMPLEX_ELT(b, i as i32);
            // stock ISNAN covers both NA and NaN components
            let value = if x.r.is_nan() || x.i.is_nan() || y.r.is_nan() || y.i.is_nan() {
                NA_LOGICAL
            } else {
                let eq = x.r == y.r && x.i == y.i;
                if (op == "==") == eq { TRUE } else { FALSE }
            };
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();
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
                if a.is_null() {
                    return R_NilValue();
                }
                let b_cdr = CDR(args);
                if b_cdr.is_null() || b_cdr == R_NilValue() {
                    // Unary +/-
                    if op_name == "-" || op_name == "+" {
                        if let Some(result) = factor_arith(op_name, call, a, R_NilValue()) {
                            return result;
                        }
                        if TYPEOF(a) == SEXPTYPE::CPLXSXP {
                            if op_name == "-" {
                                return super::complex_arith::complex_unary_vec(a, complex_negate);
                            }
                            return a;
                        }
                        // stock: unary +/- require a numeric argument
                        // (NULL included); anything else errors.
                        if !is_numeric_operand(a) {
                            arithmetic_error("invalid argument to unary operator");
                        }
                        if op_name == "+" {
                            return a;
                        }
                        let result = unary_minus(a);
                        if crate::mainutils::essentials::sexp_has_class(a, "difftime") {
                            set_difftime_attributes(result, &difftime_units(a));
                        }
                        return result;
                    }
                    return R_NilValue();
                }
                let b = CAR(b_cdr);
                if let Some(result) = date_binary_arithmetic(op_name, a, b) {
                    return result;
                }
                if let Some(result) = posixct_binary_arithmetic(op_name, a, b) {
                    return result;
                }
                if let Some(result) = difftime_binary_arithmetic(op_name, a, b) {
                    return result;
                }
                if let Some(result) = factor_arith(op_name, call, a, b) {
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
                // stock relop.c: either operand NULL yields logical(0)
                if a == R_NilValue() || b == R_NilValue() {
                    return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
                }
                if let Some(result) = data_frame_compare(op_name, call, a, b) {
                    return result;
                }
                compare_values(op_name, call, a, b)
            }
            _ => R_NilValue(),
        }
    }
}

/// Compare two non-data-frame values through the ordinary relational ladder.
///
/// `Ops.data.frame` applies the primitive separately to each column, so keeping
/// this path factored lets data-frame dispatch reuse Date/factor/character and
/// numeric behavior instead of growing a second, subtly different comparator.
unsafe fn compare_values(op_name: &str, call: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        if let Some(result) = date_binary_comparison(op_name, a, b) {
            return result;
        }
        if let Some(result) = posixct_binary_comparison(op_name, a, b) {
            return result;
        }
        if let Some(result) = difftime_binary_comparison(op_name, a, b) {
            return result;
        }
        if let Some(result) = ordered_factor_compare(op_name, a, b) {
            return result;
        }
        if let Some(result) = unordered_factor_compare(op_name, call, a, b) {
            return result;
        }
        // stock relop.c: symbols and calls deparse to strings first
        let a = if TYPEOF(a) == SEXPTYPE::SYMSXP || TYPEOF(a) == SEXPTYPE::LANGSXP {
            crate::mainutils::deparse::deparse1s(a)
        } else {
            a
        };
        let b = if TYPEOF(b) == SEXPTYPE::SYMSXP || TYPEOF(b) == SEXPTYPE::LANGSXP {
            crate::mainutils::deparse::deparse1s(b)
        } else {
            b
        };
        // stock coercion ladder: string involvement compares as
        // character, then complex, then raw/numeric coercion.
        let a_str = TYPEOF(a) == SEXPTYPE::STRSXP;
        let b_str = TYPEOF(b) == SEXPTYPE::STRSXP;
        if a_str || b_str {
            let a = if a_str {
                a
            } else {
                relop_operand_to_character(a)
            };
            let b = if b_str {
                b
            } else {
                relop_operand_to_character(b)
            };
            return character_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
            return complex_relop(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::RAWSXP || TYPEOF(b) == SEXPTYPE::RAWSXP {
            let a = if TYPEOF(a) == SEXPTYPE::RAWSXP {
                crate::mainutils::coerce::coerceVector(a, SEXPTYPE::REALSXP.into())
            } else {
                a
            };
            let b = if TYPEOF(b) == SEXPTYPE::RAWSXP {
                crate::mainutils::coerce::coerceVector(b, SEXPTYPE::REALSXP.into())
            } else {
                b
            };
            return binary_compare(op_name, a, b);
        }
        binary_compare(op_name, a, b)
    }
}

/// Port the comparison branch of GNU R's `Ops.data.frame`.
///
/// Atomic non-scalar operands are laid out column-major across the frame;
/// lists supply one operand per column; scalars are broadcast. Comparisons
/// return a logical matrix rather than leaking the frame's underlying VECSXP.
unsafe fn data_frame_compare(op_name: &str, call: SEXP, lhs: SEXP, rhs: SEXP) -> Option<SEXP> {
    unsafe {
        let lhs_frame = has_class(lhs, "data.frame") && TYPEOF(lhs) == SEXPTYPE::VECSXP;
        let rhs_frame = has_class(rhs, "data.frame") && TYPEOF(rhs) == SEXPTYPE::VECSXP;
        if !lhs_frame && !rhs_frame {
            return None;
        }

        let frame = if lhs_frame { lhs } else { rhs };
        let ncol = XLENGTH(frame);
        let nrow = crate::mainutils::essentials::data_frame_row_count(frame);

        if lhs_frame
            && rhs_frame
            && (XLENGTH(lhs) != XLENGTH(rhs)
                || crate::mainutils::essentials::data_frame_row_count(lhs)
                    != crate::mainutils::essentials::data_frame_row_count(rhs))
        {
            arithmetic_error(format!(
                "'{op_name}' only defined for equally-sized data frames"
            ));
        }

        validate_data_frame_operand(lhs, lhs_frame, ncol);
        validate_data_frame_operand(rhs, rhs_frame, ncol);

        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, nrow.saturating_mul(ncol));
        let _result_guard = protect(result);
        for column in 0..ncol {
            let (left, _left_guard) = data_frame_column_operand(lhs, lhs_frame, column, nrow);
            let (right, _right_guard) = data_frame_column_operand(rhs, rhs_frame, column, nrow);
            let compared = compare_values(op_name, call, left, right);
            let _compared_guard = protect(compared);
            if TYPEOF(compared) != SEXPTYPE::LGLSXP || XLENGTH(compared) != nrow {
                arithmetic_error("dimension mismatch in data frame comparison");
            }
            for row in 0..nrow {
                SET_LOGICAL_ELT(
                    result,
                    (column * nrow + row) as i32,
                    LOGICAL_ELT(compared, row as i32),
                );
            }
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _dim_guard = protect(dim);
        SET_INTEGER_ELT(dim, 0, nrow as i32);
        SET_INTEGER_ELT(dim, 1, ncol as i32);
        setAttrib(result, R_DimSymbol(), dim);

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, explicit_data_frame_row_names(frame));
        SET_VECTOR_ELT(dimnames, 1, getAttrib(frame, R_NamesSymbol()));
        setAttrib(result, R_DimNamesSymbol(), dimnames);
        Some(result)
    }
}

unsafe fn validate_data_frame_operand(value: SEXP, is_frame: bool, ncol: R_xlen_t) {
    unsafe {
        if is_frame || TYPEOF(value) != SEXPTYPE::VECSXP {
            return;
        }
        let len = XLENGTH(value);
        if len > 1 && len != ncol {
            arithmetic_error(format!("list of length {len} not meaningful"));
        }
        if len == 0 {
            arithmetic_error("subscript out of bounds");
        }
    }
}

unsafe fn data_frame_column_operand(
    value: SEXP,
    is_frame: bool,
    column: R_xlen_t,
    nrow: R_xlen_t,
) -> (SEXP, Option<crate::sexp::protect::ProtectGuard>) {
    unsafe {
        if is_frame {
            return (VECTOR_ELT(value, column), None);
        }
        if TYPEOF(value) == SEXPTYPE::VECSXP {
            let index = if XLENGTH(value) == 1 { 0 } else { column };
            return (VECTOR_ELT(value, index), None);
        }
        if XLENGTH(value) <= 1 {
            return (value, None);
        }

        let segment = recycled_atomic_segment(value, column * nrow, nrow);
        let guard = protect(segment);
        (segment, Some(guard))
    }
}

unsafe fn recycled_atomic_segment(value: SEXP, offset: R_xlen_t, len: R_xlen_t) -> SEXP {
    unsafe {
        let source_len = XLENGTH(value);
        let result = Rf_allocVector3(TYPEOF(value), len);
        if source_len == 0 {
            return result;
        }
        for i in 0..len {
            let source = ((offset + i) % source_len) as i32;
            let target = i as i32;
            match TYPEOF(value) {
                t if t == SEXPTYPE::LGLSXP => {
                    SET_LOGICAL_ELT(result, target, LOGICAL_ELT(value, source))
                }
                t if t == SEXPTYPE::INTSXP => {
                    SET_INTEGER_ELT(result, target, INTEGER_ELT(value, source))
                }
                t if t == SEXPTYPE::REALSXP => {
                    SET_REAL_ELT(result, target, REAL_ELT(value, source))
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    SET_COMPLEX_ELT(result, target, COMPLEX_ELT(value, source))
                }
                t if t == SEXPTYPE::STRSXP => {
                    SET_STRING_ELT(result, i, STRING_ELT(value, source as R_xlen_t))
                }
                t if t == SEXPTYPE::RAWSXP => SET_RAW_ELT(result, target, RAW_ELT(value, source)),
                _ => arithmetic_error("comparison of these types is not implemented"),
            }
        }
        result
    }
}

unsafe fn explicit_data_frame_row_names(frame: SEXP) -> SEXP {
    unsafe {
        let row_names = getAttrib(frame, Rf_install(c"row.names".as_ptr()));
        let compact = TYPEOF(row_names) == SEXPTYPE::INTSXP
            && XLENGTH(row_names) == 2
            && INTEGER_ELT(row_names, 0) == NA_INTEGER
            && INTEGER_ELT(row_names, 1) < 0;
        if compact { R_NilValue() } else { row_names }
    }
}

/// Unary minus — negate each element of a numeric vector.
unsafe fn unary_minus(x: SEXP) -> SEXP {
    unsafe {
        let Some(input) = NumericVector::from_raw(x) else {
            return R_NilValue();
        };
        let n = input.clone().len();
        let result_type = if input.clone().typeof_() == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result_raw = Rf_allocVector3(result_type, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        let mut result_mut = SexpMut::from_owned(result);
        if result_type == SEXPTYPE::REALSXP {
            for i in 0..n {
                result_mut.set_real_elt(i, -input.clone().real_at(i));
            }
        } else {
            for i in 0..n {
                let value = input.clone().int_at(i);
                let negated = if value == NA_INTEGER {
                    NA_INTEGER
                } else {
                    -value
                };
                result_mut.set_integer_elt(i, negated);
            }
        }
        let _ = result_mut.freeze();
        propagate_unary_vector_attributes(result_raw, x, n);
        result_raw

    }
}

unsafe fn ordered_factor_compare(op: &str, sa: SEXP, sb: SEXP) -> Option<SEXP> {
    unsafe {
        let a_ordered = has_class(sa, "ordered");
        let b_ordered = has_class(sb, "ordered");
        if !a_ordered && !b_ordered {
            return None;
        }

        let levels_owner = if a_ordered { sa } else { sb };
        let levels = factor_levels(levels_owner)?;
        if (a_ordered && !factor_levels_match(sa, &levels))
            || (b_ordered && !factor_levels_match(sb, &levels))
        {
            std::panic::panic_any(RError {
                message: "level sets of factors are different".to_string(),
            });
        }

        let Some(a) = Sexp::from_raw(sa) else {
            return Some(R_NilValue());
        };
        let Some(b) = Sexp::from_raw(sb) else {
            return Some(R_NilValue());
        };
        let a_len = a.len();
        let b_len = b.len();
        let n = match (a_len, b_len) {
            (0, _) | (_, 0) => 0,
            _ => a_len.max(b_len),
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return Some(R_NilValue());
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a_len, b_len);
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let lhs = ordered_operand_code(sa, i % a_len, &levels);
            let rhs = ordered_operand_code(sb, i % b_len, &levels);
            result_mut.set_logical_elt(i, ordered_code_compare(op, lhs, rhs));
        }
        let _ = result_mut.freeze();
        Some(result_raw)
    }
}

#[derive(Clone, Copy)]
enum OrderedOperand {
    Code(i32),
    Missing,
    UnknownLevel,
}

fn ordered_code_compare(op: &str, lhs: OrderedOperand, rhs: OrderedOperand) -> i32 {
    match (lhs, rhs) {
        (OrderedOperand::Missing, _) | (_, OrderedOperand::Missing) => NA_LOGICAL,
        (OrderedOperand::UnknownLevel, _) | (_, OrderedOperand::UnknownLevel) => match op {
            "==" => FALSE,
            "!=" => TRUE,
            _ => NA_LOGICAL,
        },
        (OrderedOperand::Code(lhs), OrderedOperand::Code(rhs)) => match op {
            "<" if lhs < rhs => TRUE,
            ">" if lhs > rhs => TRUE,
            "<=" if lhs <= rhs => TRUE,
            ">=" if lhs >= rhs => TRUE,
            "==" if lhs == rhs => TRUE,
            "!=" if lhs != rhs => TRUE,
            _ => FALSE,
        },
    }
}

unsafe fn ordered_operand_code(value: SEXP, index: R_xlen_t, levels: &[String]) -> OrderedOperand {
    unsafe {
        if has_class(value, "ordered") {
            let code = INTEGER_ELT(value, index as i32);
            if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                OrderedOperand::Missing
            } else {
                OrderedOperand::Code(code)
            }
        } else if TYPEOF(value) == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(value, index);
            let Some(label) = charsxp_to_string(charsxp) else {
                return OrderedOperand::Missing;
            };
            levels
                .iter()
                .position(|level| level == &label)
                .map(|position| OrderedOperand::Code(position as i32 + 1))
                .unwrap_or(OrderedOperand::UnknownLevel)
        } else {
            OrderedOperand::UnknownLevel
        }
    }
}

unsafe fn factor_levels_match(value: SEXP, expected: &[String]) -> bool {
    unsafe { factor_levels(value).is_some_and(|levels| levels == expected) }
}

unsafe fn factor_levels(value: SEXP) -> Option<Vec<String>> {
    unsafe {
        let levels = getAttrib(value, Rf_install(c"levels".as_ptr()));
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let mut out = Vec::with_capacity(LENGTH(levels) as usize);
        for i in 0..LENGTH(levels) as R_xlen_t {
            out.push(charsxp_to_string(STRING_ELT(levels, i))?);
        }
        Some(out)
    }
}

unsafe fn has_class(value: SEXP, class_name: &str) -> bool {
    unsafe {
        let class = getAttrib(value, Rf_install(c"class".as_ptr()));
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..LENGTH(class) as R_xlen_t)
            .any(|i| charsxp_to_string(STRING_ELT(class, i)).as_deref() == Some(class_name))
    }
}

unsafe fn charsxp_to_string(charsxp: SEXP) -> Option<String> {
    unsafe {
        if charsxp.is_null() || charsxp == R_NaString() {
            return None;
        }
        Sexp::from_raw(charsxp).and_then(|s| s.try_as_str().ok().map(str::to_string))
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
        let mut result_mut = SexpMut::from_owned(result);

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
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();

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

/// stock `math2(x, base, logbase)`: `log(x)/log(base)` with Math2
/// recycling, zero-length handling, attributes, and NaN warnings.
unsafe fn log_with_base(call: SEXP, sx: SEXP, sbase: SEXP) -> SEXP {
    unsafe {
        let x = NumericVector::from_raw(sx);
        let base = NumericVector::from_raw(sbase);
        let (Some(x), Some(base)) = (x, base) else {
            return R_NilValue();
        };
        let nx = x.clone().len();
        let nb = base.clone().len();
        if nx == 0 {
            // SETUP_Math2: empty x gives an empty result with x's attributes
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            if empty.is_null() {
                return R_NilValue();
            }
            copy_all_attrib(empty, sx);
            return empty;
        }
        let n = nx.max(nb);
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(nx, nb);
        let mut result_mut = SexpMut::from_owned(result);
        let mut naflag = false;
        for i in 0..n {
            let xv = x.clone().real_at(i % nx);
            let bv = base.clone().real_at(i % nb);
            // if_NA_Math2_set: NA in -> NA, NaN in -> NaN
            let out = if xv.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || bv.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                crate::sexp::ffi::NA_REAL
            } else if xv.is_nan() || bv.is_nan() {
                f64::NAN
            } else {
                let v = libm::log(xv) / libm::log(bv);
                if v.is_nan() {
                    naflag = true;
                }
                v
            };
            result_mut.set_real_elt(i, out);
        }
        let _ = result_mut.freeze();
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        // FINISH_Math2: attributes from the first operand of length n
        if n == nx {
            copy_all_attrib(result_raw, sx);
        } else if n == nb {
            copy_all_attrib(result_raw, sbase);
        }
        result_raw
    }
}

/// stock complex_math2 `logbase`: `Clog(x) / Clog(base)` element-wise.
unsafe fn complex_log_with_base(sx: SEXP, sbase: SEXP) -> SEXP {
    unsafe {
        let lx = super::complex_arith::complex_unary_vec(sx, super::complex_arith::complex_log);
        let _lx_guard = protect(lx);
        let lb = super::complex_arith::complex_unary_vec(sbase, super::complex_arith::complex_log);
        let _lb_guard = protect(lb);
        super::complex_arith::complex_binary("/", lx, lb)
    }
}

/// SHALLOW_DUPLICATE_ATTRIB: copy every attribute of `src` onto `dst`.
unsafe fn copy_all_attrib(dst: SEXP, src: SEXP) {
    unsafe {
        if src.is_null() || src == R_NilValue() {
            return;
        }
        let mut attr = crate::sexp::accessors::ATTRIB(src);
        while !attr.is_null() && attr != R_NilValue() {
            setAttrib(dst, TAG(attr), CAR(attr));
            attr = CDR(attr);
        }
    }
}

/// Handle unary math functions: abs, sqrt, log, log2, log10, exp,
/// ceiling, floor, trunc, round, sign.
pub unsafe fn do_math1(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(call);
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if x == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            arithmetic_error("non-numeric argument to mathematical function");
        }
        if has_class(x, "factor") {
            // stock Math.factor stops, attributed to the method call
            let f = Rf_install(c"Math.factor".as_ptr());
            let method_call = Rf_lang2(f, CAR(CDR(call)));
            let _method_guard = protect(method_call);
            crate::mainutils::errors::errorcall_str(
                method_call,
                &format!("'{op_name}' not meaningful for factors"),
            );
        }
        // stock do_log: `log(x, base)` recycles both arguments
        if op_name == "log" {
            let rest = CDR(args);
            if !rest.is_null() && rest != R_NilValue() {
                let mut base = CAR(rest);
                if base == crate::sexp::globals::R_MissingArg() {
                    base = Rf_ScalarReal(std::f64::consts::E);
                }
                if x == crate::sexp::globals::R_MissingArg() {
                    arithmetic_error("argument \"x\" is missing, with no default");
                }
                if XLENGTH(base) == 0 {
                    arithmetic_error("invalid argument 'base' of length 0");
                }
                if TYPEOF(x) == SEXPTYPE::CPLXSXP || TYPEOF(base) == SEXPTYPE::CPLXSXP {
                    return complex_log_with_base(x, base);
                }
                if !is_numeric_operand(x) || !is_numeric_operand(base) {
                    arithmetic_error("non-numeric argument to mathematical function");
                }
                return log_with_base(call, x, base);
            }
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
                "abs" => {
                    // stock Math1: complex abs is Mod, returned as double
                    let n = XLENGTH(x);
                    let out = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                    let Some(result) = Sexp::from_raw(out) else {
                        return R_NilValue();
                    };
                    let _out_guard = protect(out);
                    let mut result_mut = SexpMut::from_owned(result);
                    for i in 0..n {
                        let c = crate::sexp::accessors::COMPLEX_ELT(x, i as i32);
                        let value = if c.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                            || c.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                        {
                            crate::sexp::ffi::NA_REAL
                        } else {
                            libm::sqrt(c.r * c.r + c.i * c.i)
                        };
                        result_mut.set_real_elt(i, value);
                    }
                    let _ = result_mut.freeze();
                    return out;
                }
                "log10" => super::complex_arith::complex_unary_vec(x, |c| {
                    let l = super::complex_arith::complex_log(c);
                    Rcomplex {
                        r: l.r * std::f64::consts::LOG10_E,
                        i: l.i * std::f64::consts::LOG10_E,
                    }
                }),
                "log2" => super::complex_arith::complex_unary_vec(x, |c| {
                    let l = super::complex_arith::complex_log(c);
                    Rcomplex {
                        r: l.r * std::f64::consts::LOG2_E,
                        i: l.i * std::f64::consts::LOG2_E,
                    }
                }),
                _ => arithmetic_error("unimplemented complex function"),
            };
        }

        // stock Math1: non-numeric arguments error (lowercase message from
        // arithmetic.c, unlike the capital-N distn.c variant).
        if !is_numeric_operand(x) {
            arithmetic_error("non-numeric argument to mathematical function");
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
            let n = result_vec.clone().len();
            let mut all_int = true;
            for i in 0..n {
                let v = result_vec.clone().real_at(i);
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
                let mut iresult_mut = SexpMut::from_owned(iresult);
                for i in 0..n {
                    let v = result_vec.clone().real_at(i);
                    let value = if v.to_bits() == R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        v as i32
                    };
                    iresult_mut.set_integer_elt(i, value);
                }
                let _ = iresult_mut.freeze();
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
        Rf_ScalarInteger(crate::sexp::constructors::Rf_length(x))
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
        if arg.clone().len() == 0 {
            summary_error("invalid 'na.rm' value");
        }
        match arg.clone().typeof_() {
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
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
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
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
        let mut result_mut = SexpMut::from_owned(result_view);
        match result_type {
            SEXPTYPE::REALSXP => {
                result_mut.set_real_elt(
                    0,
                    min_value
                        .clone()
                        .real_elt(0)
                        .or_else(|| min_value.clone().integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
                result_mut.set_real_elt(
                    1,
                    max_value
                        .clone()
                        .real_elt(0)
                        .or_else(|| max_value.clone().integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
            }
            SEXPTYPE::INTSXP => {
                result_mut
                    .set_integer_elt(0, min_value.clone().integer_elt(0).unwrap_or(NA_INTEGER));
                result_mut
                    .set_integer_elt(1, max_value.clone().integer_elt(0).unwrap_or(NA_INTEGER));
            }
            _ => {}
        }
        let _ = result_mut.freeze();
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
        unsafe fn logical_arg_is_true(x: SEXP) -> bool {
            return false;
        }
        match arg.clone().typeof_() {
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
        for i in 0..vector.clone().len() {
            let value = vector.clone().real_at(i);
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
/// is.complex, is.logical, is.character, is.null, is.raw.
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
            "is.complex" => t == SEXPTYPE::CPLXSXP,
            "is.logical" => t == SEXPTYPE::LGLSXP,
            "is.character" => t == SEXPTYPE::STRSXP,
            "is.null" => false,
            "is.raw" => t == SEXPTYPE::RAWSXP,
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
                "is.complex" => "is.complex",
                "is.logical" => "is.logical",
                "is.character" => "is.character",
                "is.null" => "is.null",
                "is.raw" => "is.raw",
                _ => "",
            },
            Err(_) => "",
        }
    }
}

// ---------------------------------------------------------------------------
// Registration functions
// ---------------------------------------------------------------------------

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
    fn data_frame_comparison_is_columnwise_and_preserves_labels() {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(
            "x <- matrix(1:4, 2, 2, dimnames=list(c('abc','ab'), c('cde','cd'))); \
             y <- as.data.frame(x); \
             row <- y['ab',] == c(2L, 4L); \
             reverse <- c(2L, 4L) == y['ab',]; \
             listwise <- data.frame(a=1:2, b=3:4) > list(1L, 3L); \
             splitwise <- data.frame(a=1:2, b=3:4) == 1:4; \
             identical(row, matrix(c(TRUE, TRUE), 1L, 2L, \
                       dimnames=list('ab', c('cde','cd')))) && \
             identical(reverse, row) && \
             identical(listwise, matrix(c(FALSE, TRUE, FALSE, TRUE), 2L, 2L, \
                       dimnames=list(NULL, c('a','b')))) && \
             identical(splitwise, matrix(rep(TRUE, 4L), 2L, 2L, \
                       dimnames=list(NULL, c('a','b'))))",
        );
        let result = result.expect("data-frame comparisons should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
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
