#![allow(non_snake_case)]
#![deny(unsafe_op_in_unsafe_fn)]

//! A compact, native core for base `all.equal()`.
//!
//! GNU R implements `all.equal` mostly in R code.  Until the complete base
//! package is sourced at startup, keep the common numeric comparison here so
//! callers get tolerance-aware comparison rather than an `identical()` alias.

use std::ffi::{CStr, CString};

use crate::mainutils::identical::{R_IsNA, R_compute_identical};
use crate::sexp::accessors::{
    ATTRIB, CAR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME, RAW, REAL, STRING_ELT,
    TAG, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_mkString};
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NaString, R_NilValue};

const DEFAULT_TOLERANCE: f64 = 1.490_116_119_384_765_6e-8; // sqrt(DBL_EPSILON)
const MAX_COMPARE_DEPTH: usize = 64;

struct AllEqualArgs {
    target: SEXP,
    current: SEXP,
    tolerance: f64,
    scale: Option<f64>,
    check_attributes: bool,
}

/// Evaluated builtin backing the public `all.equal()` binding.
pub unsafe fn do_all_equal(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let Some(matched) = match_args(args) else {
            return mismatch("all.equal requires target and current");
        };

        match compare(
            matched.target,
            matched.current,
            matched.tolerance,
            matched.scale,
            matched.check_attributes,
            0,
        ) {
            Ok(()) => Rf_ScalarLogical(TRUE),
            Err(message) => mismatch(&message),
        }
    }
}

unsafe fn match_args(args: SEXP) -> Option<AllEqualArgs> {
    unsafe {
        let mut target = None;
        let mut current = None;
        let mut tolerance = DEFAULT_TOLERANCE;
        let mut scale = None;
        let mut check_attributes = true;
        let mut positional = 0usize;
        let mut cell = args;

        while !is_nil(cell) {
            let value = CAR(cell);
            match tag_name(TAG(cell)).as_deref() {
                Some("target") => target = Some(value),
                Some("current") => current = Some(value),
                Some("tolerance") | Some("tol") => {
                    if let Some(value) = scalar_number(value) {
                        tolerance = value.abs();
                    }
                }
                Some("scale") => scale = scalar_number(value).map(f64::abs),
                Some("check.attributes") | Some("check.attr") => {
                    check_attributes = scalar_logical(value).unwrap_or(true)
                }
                _ => {
                    if positional == 0 && target.is_none() {
                        target = Some(value);
                    } else if positional <= 1 && current.is_none() {
                        current = Some(value);
                    }
                    positional += 1;
                }
            }
            cell = CDR(cell);
        }

        Some(AllEqualArgs {
            target: target?,
            current: current?,
            tolerance,
            scale,
            check_attributes,
        })
    }
}

unsafe fn compare(
    target: SEXP,
    current: SEXP,
    tolerance: f64,
    scale: Option<f64>,
    check_attributes: bool,
    depth: usize,
) -> Result<(), String> {
    unsafe {
        if target == current {
            return Ok(());
        }
        if depth >= MAX_COMPARE_DEPTH {
            return Err("objects are nested too deeply".into());
        }
        if is_nil(target) || is_nil(current) {
            return Err("target and current differ in nullness".into());
        }

        // GNU all.equal.default: is.language → all.equal.language (deparse
        // only). S4 formula subclasses are LANGSXP with class "mForm", not
        // "formula", so the class check alone misses them.
        if TYPEOF(target) == SEXPTYPE::LANGSXP || TYPEOF(current) == SEXPTYPE::LANGSXP {
            if crate::mainutils::essentials::sexp_has_class(target, "formula")
                || crate::mainutils::essentials::sexp_has_class(current, "formula")
            {
                return compare_formula(target, current);
            }
            return compare_language(target, current);
        }
        if crate::mainutils::essentials::sexp_has_class(target, "formula")
            || crate::mainutils::essentials::sexp_has_class(current, "formula")
        {
            return compare_formula(target, current);
        }


        if crate::mainutils::essentials::sexp_has_class(target, "POSIXt")
            || crate::mainutils::essentials::sexp_has_class(current, "POSIXt")
        {
            let t = crate::mainutils::essentials::do_as_POSIXct(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                crate::sexp::constructors::Rf_cons(target, R_NilValue()),
                std::ptr::null_mut(),
            );
            let _t = crate::sexp::protect::protect(t);
            let c = crate::mainutils::essentials::do_as_POSIXct(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                crate::sexp::constructors::Rf_cons(current, R_NilValue()),
                std::ptr::null_mut(),
            );
            let _c = crate::sexp::protect::protect(c);
            return compare_numeric(t, c, tolerance, scale);
        }
        if LENGTH(target) != LENGTH(current) {
            return Err(format!(
                "Lengths ({}, {}) differ",
                LENGTH(target),
                LENGTH(current)
            ));
        }

        if check_attributes {
            compare_attributes(target, current, tolerance, depth + 1)?;
        }

        let target_type = TYPEOF(target);
        let current_type = TYPEOF(current);
        if is_numeric_type(target_type) && is_numeric_type(current_type) {
            return compare_numeric(target, current, tolerance, scale);
        }
        if target_type == SEXPTYPE::CLOSXP || current_type == SEXPTYPE::CLOSXP {
            return compare_language(target, current);
        }
        if target_type != current_type {
            return Err(format!(
                "Modes differ: target is {}, current is {}",
                type_name(target_type),
                type_name(current_type)
            ));
        }


        if target_type == SEXPTYPE::STRSXP {
            for index in 0..LENGTH(target) as usize {
                if !strings_equal(
                    STRING_ELT(target, index as i64),
                    STRING_ELT(current, index as i64),
                ) {
                    return Err(format!("{} string mismatch", index + 1));
                }
            }
            return Ok(());
        }
        if target_type == SEXPTYPE::RAWSXP {
            for index in 0..LENGTH(target) as usize {
                if *RAW(target).add(index) != *RAW(current).add(index) {
                    return Err(format!("{} raw mismatch", index + 1));
                }
            }
            return Ok(());
        }
        if target_type == SEXPTYPE::VECSXP || target_type == SEXPTYPE::EXPRSXP {
            for index in 0..LENGTH(target) as usize {
                compare(
                    VECTOR_ELT(target, index as i64),
                    VECTOR_ELT(current, index as i64),
                    tolerance,
                    None,
                    check_attributes,
                    depth + 1,
                )
                .map_err(|message| format!("Component {}: {message}", index + 1))?;
            }
            return Ok(());
        }

        if R_compute_identical(target, current, 0) != 0 {
            Ok(())
        } else {
            Err("target and current differ".into())
        }
    }
}

/// GNU `all.equal.language`: compare deparsed source.
unsafe fn compare_language(target: SEXP, current: SEXP) -> Result<(), String> {
    unsafe {
        let ttxt = deparse_joined(target);
        let ctxt = deparse_joined(current);
        if ttxt == ctxt {
            Ok(())
        } else {
            Err("target, current do not match when deparsed".into())
        }
    }
}

unsafe fn compare_formula(target: SEXP, current: SEXP) -> Result<(), String> {
    unsafe {
        if !crate::mainutils::essentials::sexp_has_class(target, "formula")
            || !crate::mainutils::essentials::sexp_has_class(current, "formula")
        {
            return Err("target is formula, current is not (or vice versa)".into());
        }
        let lt = formula_length(target);
        let lc = formula_length(current);
        if lt != lc {
            return Err(format!(
                "target, current differ in having response: {}, {}",
                lt == 3,
                lc == 3
            ));
        }
        compare_language(target, current)
    }
}

unsafe fn formula_length(x: SEXP) -> i32 {
    unsafe {
        let mut n = 0;
        let mut p = x;
        while !p.is_null()
            && p != R_NilValue()
            && (TYPEOF(p) == SEXPTYPE::LANGSXP || TYPEOF(p) == SEXPTYPE::LISTSXP)
        {
            n += 1;
            p = CDR(p);
        }
        n
    }
}



unsafe fn deparse_joined(x: SEXP) -> String {
    unsafe {
        let dumped = crate::mainutils::deparse::deparse1(
            x,
            false,
            crate::mainutils::deparse::DEFAULTDEPARSE,
        );
        let _dumped = crate::sexp::protect::protect(dumped);
        if dumped.is_null() || dumped == R_NilValue() || TYPEOF(dumped) != SEXPTYPE::STRSXP {
            return String::new();
        }
        let mut text = String::new();
        for i in 0..LENGTH(dumped) {
            if i > 0 {
                text.push('\n');
            }
            let elt = STRING_ELT(dumped, i as i64);
            if !elt.is_null() && elt != R_NilValue() {
                text.push_str(&CStr::from_ptr(CHAR(elt)).to_string_lossy());
            }
        }
        text
    }
}


unsafe fn compare_attributes(
    target: SEXP,
    current: SEXP,
    tolerance: f64,
    depth: usize,
) -> Result<(), String> {
    unsafe {
        let target_attrs = attributes(ATTRIB(target));
        let current_attrs = attributes(ATTRIB(current));
        if target_attrs.len() != current_attrs.len() {
            return Err(format!(
                "Attributes: target has {}, current has {}",
                target_attrs.len(),
                current_attrs.len()
            ));
        }
        for (name, target_value) in target_attrs {
            let Some((_, current_value)) = current_attrs.iter().find(|(other, _)| *other == name)
            else {
                return Err(format!("Attributes: current is missing {name}"));
            };
            compare(
                target_value,
                *current_value,
                tolerance,
                None,
                true,
                depth + 1,
            )
            .map_err(|message| format!("Attributes: <{name}>: {message}"))?;
        }
        Ok(())
    }
}

unsafe fn attributes(mut attrs: SEXP) -> Vec<(String, SEXP)> {
    unsafe {
        let mut result = Vec::new();
        while !is_nil(attrs) {
            let name = tag_name(TAG(attrs)).unwrap_or_else(|| "<unnamed>".into());
            result.push((name, CAR(attrs)));
            attrs = CDR(attrs);
        }
        result
    }
}

unsafe fn compare_numeric(
    target: SEXP,
    current: SEXP,
    tolerance: f64,
    explicit_scale: Option<f64>,
) -> Result<(), String> {
    unsafe {
        let len = LENGTH(target) as usize;
        let mut absolute_error = 0.0;
        let mut target_magnitude = 0.0;
        let mut compared = 0usize;

        for index in 0..len {
            let left = numeric_components(target, index);
            let right = numeric_components(current, index);
            match (left, right) {
                (Numeric::Missing, Numeric::Missing) | (Numeric::Nan, Numeric::Nan) => continue,
                (Numeric::Finite(lr, li), Numeric::Finite(rr, ri)) => {
                    if lr == rr && li == ri {
                        continue;
                    }
                    if !lr.is_finite() || !li.is_finite() || !rr.is_finite() || !ri.is_finite() {
                        return Err(format!("{} non-finite value mismatch", index + 1));
                    }
                    absolute_error += (lr - rr).hypot(li - ri);
                    target_magnitude += lr.hypot(li);
                    compared += 1;
                }
                _ => return Err(format!("{} value has different missingness", index + 1)),
            }
        }
        if compared == 0 {
            return Ok(());
        }

        let mean_error = absolute_error / compared as f64;
        let scale = explicit_scale.unwrap_or(target_magnitude / compared as f64);
        let (error, kind) = if scale.is_finite() && scale > tolerance {
            (mean_error / scale, "relative")
        } else {
            (mean_error, "absolute")
        };
        if error <= tolerance {
            Ok(())
        } else {
            Err(format!("Mean {kind} difference: {error}"))
        }
    }
}

enum Numeric {
    Missing,
    Nan,
    Finite(f64, f64),
}

unsafe fn numeric_components(value: SEXP, index: usize) -> Numeric {
    unsafe {
        let (real, imaginary) = match TYPEOF(value) {
            t if t == SEXPTYPE::LGLSXP => {
                let value = *LOGICAL(value).add(index);
                if value == NA_INTEGER {
                    return Numeric::Missing;
                }
                (value as f64, 0.0)
            }
            t if t == SEXPTYPE::INTSXP => {
                let value = *INTEGER(value).add(index);
                if value == NA_INTEGER {
                    return Numeric::Missing;
                }
                (value as f64, 0.0)
            }
            t if t == SEXPTYPE::REALSXP => (*REAL(value).add(index), 0.0),
            t if t == SEXPTYPE::CPLXSXP => {
                let value = *COMPLEX(value).add(index);
                (value.r, value.i)
            }
            _ => unreachable!("numeric_components requires a numeric SEXP"),
        };
        if R_IsNA(real) || R_IsNA(imaginary) {
            Numeric::Missing
        } else if real.is_nan() || imaginary.is_nan() {
            Numeric::Nan
        } else {
            Numeric::Finite(real, imaginary)
        }
    }
}

fn is_numeric_type(value: i32) -> bool {
    value == SEXPTYPE::LGLSXP
        || value == SEXPTYPE::INTSXP
        || value == SEXPTYPE::REALSXP
        || value == SEXPTYPE::CPLXSXP
}

unsafe fn scalar_number(value: SEXP) -> Option<f64> {
    unsafe {
        if value.is_null() || LENGTH(value) < 1 || !is_numeric_type(TYPEOF(value)) {
            return None;
        }
        match numeric_components(value, 0) {
            Numeric::Finite(real, imaginary) if imaginary == 0.0 && real.is_finite() => Some(real),
            _ => None,
        }
    }
}

unsafe fn scalar_logical(value: SEXP) -> Option<bool> {
    unsafe { scalar_number(value).map(|number| number != FALSE as f64) }
}

unsafe fn strings_equal(left: SEXP, right: SEXP) -> bool {
    unsafe {
        if left == right {
            return true;
        }
        if left == R_NaString() || right == R_NaString() || left.is_null() || right.is_null() {
            return false;
        }
        CStr::from_ptr(CHAR(left)).to_bytes() == CStr::from_ptr(CHAR(right)).to_bytes()
    }
}

unsafe fn tag_name(tag: SEXP) -> Option<String> {
    unsafe {
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let chars = CHAR(PRINTNAME(tag));
        (!chars.is_null()).then(|| CStr::from_ptr(chars).to_string_lossy().into_owned())
    }
}

fn type_name(value: i32) -> &'static str {
    match value {
        t if t == SEXPTYPE::NILSXP => "NULL",
        t if t == SEXPTYPE::LGLSXP => "logical",
        t if t == SEXPTYPE::INTSXP => "integer",
        t if t == SEXPTYPE::REALSXP => "numeric",
        t if t == SEXPTYPE::CPLXSXP => "complex",
        t if t == SEXPTYPE::STRSXP => "character",
        t if t == SEXPTYPE::VECSXP => "list",
        _ => "object",
    }
}

fn is_nil(value: SEXP) -> bool {
    value.is_null() || unsafe { value == R_NilValue() }
}

unsafe fn mismatch(message: &str) -> SEXP {
    unsafe {
        let text =
            CString::new(message).unwrap_or_else(|_| CString::new("objects differ").unwrap());
        Rf_mkString(text.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    fn logical_result(session: &mut RSession, code: &str) -> i32 {
        let (result, _, _) = session.eval_code_with_output_capture(code);
        result.expect("evaluation result").logical_elt(0).unwrap()
    }

    #[test]
    fn numeric_vectors_use_relative_tolerance() {
        let mut session = RSession::new();
        assert_eq!(
            logical_result(&mut session, "all.equal(c(1, 2), c(1, 2 + 1e-9))"),
            TRUE
        );
        assert_eq!(
            logical_result(&mut session, "is.character(all.equal(1, 1.1))"),
            TRUE
        );
    }

    #[test]
    fn numeric_attributes_are_compared_with_tolerance() {
        let mut session = RSession::new();
        assert_eq!(
            logical_result(
                &mut session,
                "all.equal(structure(1, foo=1), structure(1, foo=1+1e-9))"
            ),
            TRUE
        );
        assert_eq!(
            logical_result(
                &mut session,
                "is.character(all.equal(structure(1, foo=1), structure(1, foo=2)))"
            ),
            TRUE
        );
    }

    #[test]
    fn named_tolerance_and_missing_values_match_base_contract() {
        let mut session = RSession::new();
        assert_eq!(
            logical_result(&mut session, "all.equal(1, 1.01, tolerance=.02)"),
            TRUE
        );
        assert_eq!(
            logical_result(&mut session, "all.equal(c(NA, NaN), c(NA, NaN))"),
            TRUE
        );
        assert_eq!(
            logical_result(&mut session, "is.character(all.equal(NA, NaN))"),
            TRUE
        );
    }
}
