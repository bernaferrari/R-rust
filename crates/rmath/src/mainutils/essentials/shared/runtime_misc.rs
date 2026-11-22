//! Misc runtime scalar/arg/string/date helpers.

use super::*;
use crate::sexp::ffi::{Rbyte, Rcomplex};
use crate::sexp::object::{Sexp, SexpMut};
use crate::sexp::owner::OwnerToken;
use std::os::raw::c_double;

pub(crate) use crate::sexp::symbol::install_str;

/// Capture the active original owner for checked element access, raising an
/// R error without one. The `'static` token is sound to retain: every token
/// operation re-validates the owner generation before dereferencing it.
pub(crate) fn active_owner() -> OwnerToken<'static> {
    current_owner().unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Capture the active original owner, reporting a missing owner as an error.
pub(crate) fn current_owner() -> crate::sexp::object::SexpResult<OwnerToken<'static>> {
    OwnerToken::current()
}

/// Root a raw SEXP in `owner` for checked element access.
pub(crate) fn owner_sexp<'s>(owner: OwnerToken<'s>, value: SEXP) -> Sexp<'s> {
    owner
        .sexp(value)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Root a raw SEXP in the active owner for checked element writes.
pub(crate) fn owner_sexp_mut(value: SEXP) -> SexpMut<'static> {
    active_owner()
        .sexp(value)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked REALSXP element read of a raw SEXP through the active owner.
pub(crate) fn real_elt_checked(x: SEXP, i: R_xlen_t) -> c_double {
    owner_sexp(active_owner(), x)
        .try_real_elt(i)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked LGLSXP element read of a raw SEXP through the active owner.
pub(crate) fn logical_elt_checked(x: SEXP, i: R_xlen_t) -> c_int {
    owner_sexp(active_owner(), x)
        .try_logical_elt(i)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked CPLXSXP element read of a raw SEXP through the active owner.
pub(crate) fn complex_elt_checked(x: SEXP, i: R_xlen_t) -> Rcomplex {
    owner_sexp(active_owner(), x)
        .try_complex_elt(i)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked RAWSXP element read of a raw SEXP through the active owner.
pub(crate) fn raw_elt_checked(x: SEXP, i: R_xlen_t) -> Rbyte {
    owner_sexp(active_owner(), x)
        .try_raw_elt(i)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked INTSXP element read of a raw SEXP through the active owner.
pub(crate) fn integer_elt_checked(x: SEXP, i: R_xlen_t) -> c_int {
    owner_sexp(active_owner(), x)
        .try_integer_elt(i)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked STRSXP element read of a raw SEXP through the active owner.
pub(crate) fn string_elt_checked(x: SEXP, i: R_xlen_t) -> SEXP {
    owner_sexp(active_owner(), x)
        .try_string_elt(i)
        .map(|s| s.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked VECSXP element read of a raw SEXP through the active owner.
pub(crate) fn vector_elt_checked(x: SEXP, i: R_xlen_t) -> SEXP {
    owner_sexp(active_owner(), x)
        .try_vector_elt(i)
        .map(|s| s.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked REALSXP element write of a raw SEXP through the active owner.
pub(crate) fn set_real_checked(x: SEXP, i: R_xlen_t, v: c_double) {
    owner_sexp_mut(x)
        .try_set_real_elt(i, v)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked INTSXP element write of a raw SEXP through the active owner.
pub(crate) fn set_integer_checked(x: SEXP, i: R_xlen_t, v: c_int) {
    owner_sexp_mut(x)
        .try_set_integer_elt(i, v)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked LGLSXP element write of a raw SEXP through the active owner.
pub(crate) fn set_logical_checked(x: SEXP, i: R_xlen_t, v: c_int) {
    owner_sexp_mut(x)
        .try_set_logical_elt(i, v)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked CPLXSXP element write of a raw SEXP through the active owner.
pub(crate) fn set_complex_checked(x: SEXP, i: R_xlen_t, v: Rcomplex) {
    owner_sexp_mut(x)
        .try_set_complex_elt(i, v)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked RAWSXP element write of a raw SEXP through the active owner.
pub(crate) fn set_raw_checked(x: SEXP, i: R_xlen_t, v: Rbyte) {
    owner_sexp_mut(x)
        .try_set_raw_elt(i, v)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Lossy text of a CHARSXP reached through the active owner.
pub(crate) fn charsxp_text(x: SEXP) -> Option<String> {
    charsxp_utf8_lossy(&owner_sexp(active_owner(), x))
}

/// Allocate a scalar logical through the runtime constructor.
///
/// Centralizes the single unsafe `Rf_ScalarLogical` FFI call so callers keep
/// their bodies on the safe side of the boundary.
pub(crate) fn scalar_logical(value: c_int) -> SEXP {
    Rf_ScalarLogical(value)
}

/// Read one integer-like element (`INTSXP` or `LGLSXP`) through typed cells.
pub(crate) fn integer_like_elt(x: &Sexp<'_>, i: R_xlen_t) -> c_int {
    let result = if x.typeof_() == SEXPTYPE::LGLSXP {
        x.try_logical_elt(i)
    } else {
        x.try_integer_elt(i)
    };
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// CHARSXP bytes decoded like `charsxp_as_utf8`: declared Latin-1 payloads
/// are transcoded; anything else is interpreted as (possibly invalid) UTF-8,
/// lossily.
pub(crate) fn charsxp_utf8_lossy(chars: &Sexp<'_>) -> Option<String> {
    if chars.typeof_() != SEXPTYPE::CHARSXP {
        return None;
    }
    if crate::sexp::accessors::getCharCE(chars.as_raw()) == crate::sexp::encoding::CE_LATIN1 {
        let bytes = chars.header().char_bytes()?;
        Some(bytes.iter().map(|&byte| byte as char).collect())
    } else {
        match chars.try_as_string() {
            Ok(text) => Some(text),
            Err(_) => Some(String::from_utf8_lossy(&chars.header().char_bytes()?).into_owned()),
        }
    }
}

/// Read a scalar real from a numeric SEXP, with default.
pub(crate) fn real_or_default(x: SEXP, default: f64) -> f64 {
    if x.is_null() || x == R_NilValue() || XLENGTH(x) < 1 {
        return default;
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        return default;
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    if t == SEXPTYPE::REALSXP {
        x.try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        let v = integer_like_elt(&x, 0);
        if v == NA_INTEGER { NA_REAL } else { v as f64 }
    }
}

pub(crate) fn base_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

pub(crate) fn tag_name(cell: SEXP) -> Option<String> {
    let tag = TAG(cell);
    if tag.is_null() || tag == R_NilValue() {
        return None;
    }
    let owner = active_owner();
    let tag = owner.sexp(tag).ok()?;
    let pname = tag.try_printname().ok()?;
    charsxp_utf8_lossy(&pname)
}

pub(crate) fn real_elt_or_default(x: SEXP, i: R_xlen_t, default: f64) -> f64 {
    if x.is_null() || x == R_NilValue() {
        return default;
    }
    let n = XLENGTH(x);
    if n == 0 {
        return default;
    }
    let idx = i % n;
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        return default;
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    if t == SEXPTYPE::REALSXP {
        x.try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        let v = integer_like_elt(&x, idx);
        if v == NA_INTEGER { default } else { v as f64 }
    }
}

pub(crate) fn numeric_elt_as_count(x: SEXP, i: R_xlen_t) -> usize {
    let value = real_elt_or_default(x, i, 0.0);
    if value.is_finite() {
        (value as i64).max(0) as usize
    } else {
        0
    }
}

pub(crate) fn named_logical_arg(args: SEXP, name: &str) -> Option<bool> {
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        if tag_name(current).as_deref() == Some(name) {
            let value = CAR(current);
            if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
                return None;
            }
            let t = TYPEOF(value);
            if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
                return None;
            }
            let owner = active_owner();
            let value = owner_sexp(owner, value);
            let raw = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                integer_like_elt(&value, 0)
            } else {
                value
                    .try_real_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as c_int
            };
            return (raw != NA_INTEGER).then_some(raw != 0);
        }
        current = CDR(current);
    }
    None
}

pub(crate) fn logical_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<bool> {
    let value = arg_by_name_or_position(args, &[name], position);
    if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
        return None;
    }
    let t = TYPEOF(value);
    if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
        return None;
    }
    let owner = active_owner();
    let value = owner_sexp(owner, value);
    let raw = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
        integer_like_elt(&value, 0)
    } else {
        let v = value
            .try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if ISNAN(v) { NA_LOGICAL } else { v as c_int }
    };
    (raw != NA_INTEGER).then_some(raw != 0)
}

pub(crate) fn integer_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<c_int> {
    let value = arg_by_name_or_position(args, &[name], position);
    if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
        return None;
    }
    let t = TYPEOF(value);
    if t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP && t != SEXPTYPE::REALSXP {
        return None;
    }
    let owner = active_owner();
    let value = owner_sexp(owner, value);
    let raw = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        integer_like_elt(&value, 0)
    } else {
        let v = value
            .try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
            NA_INTEGER
        } else {
            v as c_int
        }
    };
    Some(raw)
}

pub(crate) fn arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if names.iter().any(|name| tag == *name) {
                    return CAR(current);
                }
            }
            current = CDR(current);
        }

        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    return CAR(current);
                }
                positional += 1;
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

pub(crate) fn is_string_na(x: SEXP, i: R_xlen_t) -> bool {
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
        return false;
    }
    let n = XLENGTH(x);
    if n == 0 {
        return false;
    }
    let owner = active_owner();
    owner
        .sexp(x)
        .and_then(|x| x.try_string_elt(i % n))
        .is_ok_and(|chars| chars.is_na_string())
}

pub(crate) fn element_coerces_to_character_na(x: SEXP, i: R_xlen_t) -> bool {
    let n = XLENGTH(x);
    if x.is_null() || x == R_NilValue() || n == 0 {
        return false;
    }
    let idx = i % n;
    let ty = TYPEOF(x);
    if ty == SEXPTYPE::STRSXP {
        is_string_na(x, idx)
    } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
        let owner = active_owner();
        let x = owner_sexp(owner, x);
        integer_like_elt(&x, idx) == NA_INTEGER
    } else if ty == SEXPTYPE::REALSXP {
        let owner = active_owner();
        let x = owner_sexp(owner, x);
        let value = x
            .try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
    } else {
        false
    }
}

pub(crate) fn string_contains(text: &str, pattern: &str, ignore_case: bool) -> bool {
    fixed_find(text, pattern, ignore_case).is_some()
}

pub(crate) fn fixed_find(
    text: &str,
    pattern: &str,
    ignore_case: bool,
) -> Option<crate::mainutils::grep::RegexMatch> {
    let hay = text.as_bytes();
    let needle = pattern.as_bytes();
    if needle.is_empty() {
        return Some(crate::mainutils::grep::RegexMatch { start: 0, end: 0 });
    }
    if hay.len() < needle.len() {
        return None;
    }
    for start in 0..=(hay.len() - needle.len()) {
        let matched = needle.iter().enumerate().all(|(idx, expected)| {
            let actual = hay[start + idx];
            if ignore_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == *expected
            }
        });
        if matched {
            return Some(crate::mainutils::grep::RegexMatch {
                start,
                end: start + needle.len(),
            });
        }
    }
    None
}

pub(crate) fn grep_value_matches(
    text: &str,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
) -> bool {
    if fixed {
        string_contains(text, pattern, ignore_case)
    } else if perl {
        crate::mainutils::grep::perl_find(pattern, text, ignore_case).is_some()
    } else {
        crate::mainutils::grep::ere_is_match(pattern, text, ignore_case)
    }
}

/// GNU grep.c: `fixed = TRUE` wins, warns, and then runs as a fixed match.
pub(crate) fn ignore_perl_when_fixed(perl: &mut bool, fixed: bool) {
    if fixed && *perl {
        let msg = CString::new("argument 'perl = TRUE' will be ignored").unwrap_or_default();

        crate::mainutils::errors::Rf_warning_cstr(&msg);

        *perl = false;
    }
}

pub(crate) fn grep_match_indices(
    x: SEXP,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
    invert: bool,
) -> crate::mainutils::grep::RegexIndices {
    let mut matches = crate::mainutils::grep::regex_workspace().buffer(0);

    if x.is_null() || x == R_NilValue() {
        return matches;
    }
    let n = XLENGTH(x);
    let mut utf8_warned = false;
    let owner = active_owner();
    for i in 0..n {
        if is_string_na(x, i) {
            if invert {
                matches.push(i);
            }
            continue;
        }
        if !utf8_warned && TYPEOF(x) == SEXPTYPE::STRSXP {
            let invalid_utf8 = owner
                .sexp(x)
                .and_then(|x| x.try_string_elt(i))
                .ok()
                .is_some_and(|elt| {
                    crate::sexp::accessors::getCharCE(elt.as_raw())
                        == crate::sexp::encoding::CE_UTF8
                        && elt.try_as_string().is_err()
                });
            if invalid_utf8 {
                utf8_warned = true;
                let msg = format!("input string {} is invalid UTF-8\0", i + 1);
                crate::mainutils::errors::warningcall_cstr(
                    crate::sexp::globals::R_NilValue(),
                    std::ffi::CStr::from_bytes_until_nul(msg.as_bytes()).unwrap_or_default(),
                );
            }
        }
        let matched = grep_value_matches(&elt_to_string(x, i), pattern, ignore_case, perl, fixed);
        if if invert { !matched } else { matched } {
            matches.push(i);
        }
    }
    matches
}

pub(crate) fn environment_arg_or_default(
    args: SEXP,
    names: &[&str],
    position: usize,
    default: SEXP,
) -> SEXP {
    {
        let arg = arg_by_name_or_position(args, names, position);
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::ENVSXP {
            arg
        } else {
            default
        }
    }
}

pub(crate) fn copy_vector_elt(dst: SEXP, dst_idx: R_xlen_t, src: SEXP, src_idx: R_xlen_t) {
    let owner = active_owner();
    let src = owner_sexp(owner, src);
    let Ok(mut dst) = owner.sexp(dst).and_then(SexpMut::try_from_checked) else {
        return;
    };
    match src.typeof_() {
        SEXPTYPE::REALSXP => {
            if let Ok(v) = src.try_real_elt(src_idx) {
                let _ = dst.try_set_real_elt(dst_idx, v);
            }
        }
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
            let v = integer_like_elt(&src, src_idx);
            let result = if dst.typeof_() == SEXPTYPE::LGLSXP {
                dst.try_set_logical_elt(dst_idx, v)
            } else {
                dst.try_set_integer_elt(dst_idx, v)
            };
            let _ = result;
        }
        SEXPTYPE::STRSXP => {
            if let Ok(chars) = src.try_string_elt(src_idx) {
                let _ = dst.try_set_string_elt(dst_idx, chars);
            }
        }
        SEXPTYPE::CPLXSXP => {
            if let Ok(v) = src.try_complex_elt(src_idx) {
                let _ = dst.try_set_complex_elt(dst_idx, v);
            }
        }
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
            if let Ok(child) = src.try_vector_elt(src_idx) {
                let _ = dst.try_set_vector_elt(dst_idx, child);
            }
        }
        SEXPTYPE::RAWSXP => {
            if let Ok(v) = src.try_raw_elt(src_idx) {
                let _ = dst.try_set_raw_elt(dst_idx, v);
            }
        }
        _ => {}
    }
}

pub(crate) fn map_path_strings(x: SEXP, f: fn(&str) -> String) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if is_string_na(x, idx) {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let value = f(&elt_to_string(x, i));
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) fn trim_trailing_separators(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        &path[..1]
    } else {
        trimmed
    }
}

pub(crate) fn r_basename(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

pub(crate) fn r_dirname(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    match trimmed.rfind(['/', '\\']) {
        Some(0) => trimmed[..1].to_string(),
        Some(pos) => trimmed[..pos].to_string(),
        None => ".".to_string(),
    }
}

/// Convert an element of a vector to a String.
pub(crate) fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    if x.is_null() {
        return "NULL".to_string();
    }
    let t = TYPEOF(x);
    let n = match SEXPTYPE(t) {
        SEXPTYPE::NILSXP => 0,
        SEXPTYPE::REALSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::VECSXP
        | SEXPTYPE::EXPRSXP => XLENGTH(x),
        _ => 1,
    };
    if n == 0 {
        return String::new();
    }
    let idx = i.rem_euclid(n);
    if !matches!(
        SEXPTYPE(t),
        SEXPTYPE::REALSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP
    ) {
        return format!("{:?}", t);
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    if t == SEXPTYPE::REALSXP {
        let v = x
            .try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            "NA".to_string()
        } else if v.is_nan() {
            "NaN".to_string()
        } else if v.is_infinite() {
            if v.is_sign_negative() { "-Inf" } else { "Inf" }.to_string()
        } else {
            format!("{}", v)
        }
    } else if t == SEXPTYPE::INTSXP {
        let v = x
            .try_integer_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if v == NA_INTEGER {
            "NA".to_string()
        } else if let Some(label) = factor_label_at(x.as_raw(), v) {
            label
        } else {
            format!("{}", v)
        }
    } else if t == SEXPTYPE::LGLSXP {
        let v = x
            .try_logical_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if v == NA_INTEGER {
            "NA".to_string()
        } else if v == TRUE {
            "TRUE".to_string()
        } else {
            "FALSE".to_string()
        }
    } else if t == SEXPTYPE::CPLXSXP {
        let c = x
            .try_complex_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if c.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            || c.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        {
            "NA".to_string()
        } else {
            let imag = if c.i == 0.0 { 0.0 } else { c.i };
            if imag.is_sign_negative() {
                format!("{}{}i", c.r, imag)
            } else {
                format!("{}+{}i", c.r, imag)
            }
        }
    } else if t == SEXPTYPE::RAWSXP {
        let v = x
            .try_raw_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        format!("{:02x}", v)
    } else if t == SEXPTYPE::STRSXP {
        match x.try_string_elt(idx) {
            Ok(chars) if !chars.is_na_string() => {
                charsxp_utf8_lossy(&chars).unwrap_or_else(|| "NA".to_string())
            }
            _ => "NA".to_string(),
        }
    } else if t == SEXPTYPE::SYMSXP {
        x.try_printname()
            .ok()
            .and_then(|pname| charsxp_utf8_lossy(&pname))
            .unwrap_or_else(|| "symbol".to_string())
    } else {
        match x.try_vector_elt(idx) {
            Ok(elt) => sexp_key(elt.as_raw()),
            Err(error) => crate::sexp::context::r_error(error.to_string()),
        }
    }
}

/// Stable key for one SEXP, used by duplicated() on lists of rows.
pub(crate) fn sexp_key(x: SEXP) -> String {
    if x.is_null() || x == crate::sexp::globals::R_NilValue() {
        return "NULL".to_string();
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::CLOSXP {
        let text = crate::mainutils::deparse::deparse1line(x, false);
        if !text.is_null() && text != crate::sexp::globals::R_NilValue() && XLENGTH(text) > 0 {
            let owner = active_owner();
            if let Ok(chars) = owner.sexp(text).and_then(|text| text.try_string_elt(0))
                && let Some(text) = charsxp_utf8_lossy(&chars)
            {
                return text;
            }
        }
    }
    if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
        let owner = active_owner();
        let n = XLENGTH(x);
        let mut parts = Vec::with_capacity(n as usize);
        for i in 0..n {
            let part = match owner.sexp(x).and_then(|x| x.try_vector_elt(i)) {
                Ok(elt) => sexp_key(elt.as_raw()),
                Err(error) => crate::sexp::context::r_error(error.to_string()),
            };
            parts.push(part);
        }
        return format!("L({})", parts.join("\x1f"));
    }
    let n = XLENGTH(x).max(1);
    let mut parts = Vec::with_capacity(n as usize);
    for i in 0..n {
        parts.push(elt_to_string(x, i));
    }
    format!("V:{}", parts.join("\x1f"))
}

pub(crate) fn factor_label_at(x: SEXP, code: i32) -> Option<String> {
    if code <= 0 {
        return None;
    }
    let levels = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
    if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
        return None;
    }
    let index = (code - 1) as R_xlen_t;
    if index >= XLENGTH(levels) {
        return None;
    }
    let owner = active_owner();
    let chars = owner
        .sexp(levels)
        .and_then(|v| v.try_string_elt(index))
        .ok()?;
    if chars.is_na_string() {
        Some("NA".to_string())
    } else {
        charsxp_utf8_lossy(&chars)
    }
}

/// `cat()` rendering: GNU print digits for real/complex/raw. paste/as.character
/// keep using `elt_to_string` (15-digit-style conversion).
pub(crate) fn cat_elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if n <= 0 {
        return elt_to_string(x, i);
    }
    let idx = i.rem_euclid(n);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::CPLXSXP && t != SEXPTYPE::RAWSXP {
        return elt_to_string(x, i);
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    if t == SEXPTYPE::REALSXP {
        let v = x
            .try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        crate::sexp::output::format_cat_real(v)
    } else if t == SEXPTYPE::CPLXSXP {
        let v = x
            .try_complex_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        crate::sexp::output::format_complex_value(v)
    } else {
        let v = x
            .try_raw_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        crate::sexp::output::format_raw_value(v)
    }
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(crate) fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_prime = (5 * doy + 2) / 153;
    let day = doy - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub(crate) fn parse_iso_date_days(text: &str) -> Option<f64> {
    let text = text.trim();
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    // GNU accepts '/' when it is the only separator.
    let sep = if text.contains('/') && !text.contains('-') {
        '/'
    } else {
        '-'
    };
    let mut parts = text.split(sep);
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day_tok = parts.next()?;
    let (day_digits, rest) = day_tok
        .split_once(char::is_whitespace)
        .unwrap_or((day_tok, ""));
    let day = day_digits.parse::<i64>().ok()?;
    let rest = rest.trim();
    if !rest.is_empty() && rest.split(':').any(|piece| piece.parse::<u32>().is_err()) {
        return None;
    }
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64)
}

pub(crate) fn date_days_to_iso(days: f64) -> Option<String> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
        return Some("NA".to_string());
    }
    if days.is_nan() {
        return Some("NaN".to_string());
    }
    if days.is_infinite() {
        return Some(if days > 0.0 { "Inf" } else { "-Inf" }.to_string());
    }
    let (year, month, day) = date_days_to_civil(days)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

pub(crate) fn date_days_to_civil(days: f64) -> Option<(i64, i64, i64)> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !days.is_finite() {
        return None;
    }
    Some(civil_from_days(days.floor() as i64))
}

pub(crate) fn parse_iso_datetime_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    let mut fields = text.split_whitespace();
    let date = fields.next()?;
    let time = fields.next().unwrap_or("00:00:00");
    if fields.next().is_some() {
        return None;
    }
    let days = parse_iso_date_days(date)?;
    if !days.is_finite() {
        return Some(days);
    }
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next().unwrap_or("0").parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    Some(days * 86_400.0 + (hour * 3_600 + minute * 60 + second) as f64)
}

pub(crate) fn posix_seconds_to_iso(seconds: f64, include_tz: bool) -> Option<String> {
    posix_seconds_to_iso_with_time(seconds, include_tz, false)
}

pub(crate) fn posix_seconds_to_iso_with_time(
    seconds: f64,
    include_tz: bool,
    force_time: bool,
) -> Option<String> {
    if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let days = whole.div_euclid(86_400);
    let rem = whole.rem_euclid(86_400);
    let date = date_days_to_iso(days as f64)?;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let mut out = if !force_time && hour == 0 && minute == 0 && second == 0 {
        date
    } else {
        format!("{date} {hour:02}:{minute:02}:{second:02}")
    };
    if include_tz {
        out.push_str(" UTC");
    }
    Some(out)
}

pub(crate) fn sexp_has_class(x: SEXP, class_name: &str) -> bool {
    if x.is_null() || x == R_NilValue() {
        return false;
    }
    let class = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
    if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
        return false;
    }
    (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
}

pub(crate) fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    if x.is_null() || x == R_NilValue() {
        return NA_REAL;
    }
    let n = XLENGTH(x);
    if n == 0 {
        return NA_REAL;
    }
    let idx = if n == 0 { 0 } else { i % n };
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        return NA_REAL;
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    if t == SEXPTYPE::REALSXP {
        x.try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        let v = integer_like_elt(&x, idx);
        if v == NA_INTEGER { NA_REAL } else { v as f64 }
    }
}

#[cfg(test)]
mod grep_indices_memory_tests {
    use super::*;
    #[test]
    fn owned_grep_indices_stay_admitted_until_publication_finishes() {
        let session = crate::sexp::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let source = owner.node_factory().strings(&["é", "éa", "é"]).unwrap();
        let indices = grep_match_indices(source.as_raw(), "é", false, false, true, false);
        assert_eq!(&*indices, &[0, 1, 2]);
        let required = indices.len() * std::mem::size_of::<R_xlen_t>();
        assert!(owner.with_arena(|a| a.workspace_bytes()).unwrap() >= required);
        drop(indices);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
    #[test]
    fn owned_grep_indices_growth_denial_recovers_at_original_cap() {
        use crate::sexp::memory::ArenaBudget;
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let session = crate::sexp::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let source = owner.node_factory().strings(&["é"; 16]).unwrap();
        let used = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let cap = used + 12 * std::mem::size_of::<R_xlen_t>();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
            .unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| grep_match_indices(
                source.as_raw(),
                "é",
                false,
                false,
                true,
                false
            )))
            .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let indices = grep_match_indices(source.as_raw(), "x", false, false, true, false);
        assert!(indices.is_empty());
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
        drop(indices);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn owned_grep_indices_consuming_iterator_keeps_original_charge_after_teardown() {
        let first = crate::sexp::RSession::new_for_gc_tests();
        let owner = first.owner_token().unwrap();
        let source = owner.node_factory().strings(&["é", "a", "é"]).unwrap();
        let ledger = source.allocation().unwrap().heap_identity().byte_ledger();
        let indices = grep_match_indices(source.as_raw(), "é", false, false, true, false);
        let charge = ledger.workspace_bytes_for_test();
        assert!(charge >= 2 * std::mem::size_of::<R_xlen_t>());
        let second = crate::sexp::RSession::new_for_gc_tests();
        let other = second.owner_token().unwrap();
        let before = other.with_arena(|a| a.workspace_bytes()).unwrap();
        let mut iter = indices.into_iter();
        assert_eq!(iter.next(), Some(0));
        drop(source);
        drop(first);
        assert_eq!(iter.next(), Some(2));
        assert_eq!(ledger.workspace_bytes_for_test(), charge);
        assert_eq!(other.with_arena(|a| a.workspace_bytes()).unwrap(), before);
        drop(iter);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }
}

/// Scalar `CHARSXP` from a borrowed C string (`Rf_mkChar`).
///
/// Taking `&CStr` keeps the pointer contract on the type: the borrow is a
/// live, NUL-terminated buffer for the whole call.
pub(crate) fn mk_char_cstr(text: &std::ffi::CStr) -> SEXP {
    // SAFETY: `text` is a live NUL-terminated borrow for the duration of the
    // call; `Rf_mkChar` only copies the bytes before the terminator.
    unsafe { Rf_mkChar(text.as_ptr()) }
}

/// Scalar `STRSXP` from a borrowed C string (`Rf_mkString`).
pub(crate) fn mk_string_cstr(text: &std::ffi::CStr) -> SEXP {
    // SAFETY: `text` is a live NUL-terminated borrow for the duration of the
    // call; `Rf_mkString` only copies the bytes before the terminator.
    unsafe { Rf_mkString(text.as_ptr()) }
}

/// Intern a borrowed C string as a symbol (`Rf_install`); non-UTF-8 names
/// yield a null SEXP exactly like `Rf_install`.
pub(crate) fn install_cstr(name: &std::ffi::CStr) -> SEXP {
    match name.to_str() {
        Ok(name) => crate::sexp::symbol::install_str(name),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frame-local lookup (`R_findVarInFrame`), `R_UnboundValue` when absent.
pub(crate) fn find_var_in_frame(rho: SEXP, symbol: SEXP) -> SEXP {
    let unbound = R_UnboundValue();
    if rho.is_null() || symbol.is_null() {
        return unbound;
    }
    let owner = active_owner();
    match (owner.sexp(rho), owner.sexp(symbol)) {
        (Ok(rho), Ok(symbol)) => crate::sexp::envir::find_var_in_frame_safe(rho, symbol)
            .map(|value| value.as_raw())
            .unwrap_or(unbound),
        _ => unbound,
    }
}

/// Inherited variable lookup (`R_findVar`), `R_UnboundValue` when absent.
pub(crate) fn find_var(symbol: SEXP, rho: SEXP) -> SEXP {
    let unbound = R_UnboundValue();
    if symbol.is_null() {
        return unbound;
    }
    let owner = active_owner();
    match (owner.sexp(symbol), owner.sexp(rho)) {
        (Ok(symbol), Ok(rho)) => crate::sexp::envir::find_var_safe(symbol, rho)
            .map(|value| value.as_raw())
            .unwrap_or(unbound),
        _ => unbound,
    }
}

/// Function lookup (`findFun`), `R_UnboundValue` when absent.
pub(crate) fn find_fun(symbol: SEXP, rho: SEXP) -> SEXP {
    let unbound = R_UnboundValue();
    if symbol.is_null() {
        return unbound;
    }
    let owner = active_owner();
    match (owner.sexp(symbol), owner.sexp(rho)) {
        (Ok(symbol), Ok(rho)) => crate::sexp::envir::find_fun_safe(symbol, rho)
            .map(|value| value.as_raw())
            .unwrap_or(unbound),
        _ => unbound,
    }
}

/// Force a promise (`forcePromise`), `R_NilValue` for null/invalid input.
pub(crate) fn force_promise(prom: SEXP) -> SEXP {
    if prom.is_null() {
        return R_NilValue();
    }
    match active_owner().sexp(prom) {
        Ok(prom) => crate::sexp::envir::force_promise_safe(prom)
            .map(|value| value.as_raw())
            .unwrap_or_else(R_NilValue),
        Err(_) => R_NilValue(),
    }
}

/// Bind `symbol` to `value` in `rho`'s frame (`defineVar`).
pub(crate) fn define_var(symbol: SEXP, value: SEXP, rho: SEXP) {
    let _ = crate::sexp::envir::define_var_updates(symbol, value, rho);
}

/// Owned copy of a CHARSXP's bytes up to the first NUL: the value that
/// `charsxp_cstring(x)` used to borrow, without an unbounded borrow.
pub(crate) fn charsxp_cstring(x: SEXP) -> std::ffi::CString {
    let mut bytes = crate::mainutils::printutils::charsxp_bytes(x);
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(nul);
    }
    std::ffi::CString::new(bytes).unwrap_or_default()
}

/// Allocate a matrix (`allocMatrix`).
///
/// `allocMatrix` has no pointer contract: it only takes integer extents,
/// validates them and allocates through the checked vector constructor.
pub(crate) fn alloc_matrix(mode: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    // SAFETY: integer-only arguments; allocMatrix validates extents and mode.
    crate::mainutils::array::allocMatrix(mode, nrow, ncol)
}

/// Build a closure over live `formals`, `body` and `env` (`mkCLOSXP`).
pub(crate) fn mk_closure(formals: SEXP, body: SEXP, env: SEXP) -> SEXP {
    // SAFETY: the three nodes are live SEXPs for the call; mkCLOSXP only
    // allocates the closure cell that captures them.
    unsafe { crate::mainutils::dstruct::mkCLOSXP(formals, body, env) }
}

/// Copy all attributes except names/dim/dimnames (`copyMostAttrib`).
pub(crate) fn copy_most_attrib(src: SEXP, dest: SEXP) {
    // SAFETY: both arguments are live SEXPs; the copy only walks `src`'s
    // attribute pairlist and installs attributes on `dest` through the
    // checked attribute API.
    crate::mainutils::array::copyMostAttrib(src, dest)
}

/// `isFactor(x)` on a live SEXP.
pub(crate) fn is_factor(x: SEXP) -> c_int {
    crate::mainutils::apply::isFactor(x)
}

/// `R_compute_identical(x, y, flags)` on live SEXPs.
pub(crate) fn compute_identical(x: SEXP, y: SEXP, flags: c_int) -> c_int {
    crate::mainutils::identical::R_compute_identical(x, y, flags)
}

/// Unwrap a checked object result, raising the error as an R error.
pub(crate) fn sexp_ok<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Owned CHARSXP text standing in for a `CHAR(x)` pointer: `None` plays the
/// role of the null pointer `CHAR` returns for a null handle.
pub(crate) struct CharsxpText(Option<std::ffi::CString>);

impl CharsxpText {
    /// Whether the source handle was null.
    pub(crate) fn is_null(&self) -> bool {
        self.0.is_none()
    }

    /// The text up to the first NUL (empty for a null handle).
    pub(crate) fn as_cstr(&self) -> &std::ffi::CStr {
        self.0.as_deref().unwrap_or(c"")
    }
}

/// Owned replacement for reading `CHAR(x)` through a raw pointer.
pub(crate) fn charsxp_text_of(x: SEXP) -> CharsxpText {
    if x.is_null() {
        CharsxpText(None)
    } else {
        CharsxpText(Some(charsxp_cstring(x)))
    }
}

/// `Rf_mkCharLen` over the first `len` bytes of a borrowed slice; a negative
/// `len` yields a null SEXP exactly like `Rf_mkCharLen`.
pub(crate) fn mk_char_len(bytes: &[u8], len: c_int) -> SEXP {
    let Ok(count) = usize::try_from(len) else {
        return std::ptr::null_mut();
    };
    let bytes = &bytes[..count];
    // SAFETY: `bytes` is a live borrowed slice of exactly `len` bytes.
    unsafe { crate::sexp::constructors::Rf_mkCharLen(bytes.as_ptr().cast(), len) }
}

/// `R_data_class(x)` on a live SEXP.
pub(crate) fn data_class(x: SEXP) -> SEXP {
    // SAFETY: `x` is a live SEXP; R_data_class only reads its attributes and
    // allocates the class vector it returns.
    crate::eval::attrib_core::R_data_class(x)
}

/// `check1arg(args, call, name)` with the formal name as a borrowed C string.
pub(crate) fn check1arg_cstr(args: SEXP, call: SEXP, name: &std::ffi::CStr) {
    // SAFETY: `name` is a live NUL-terminated borrow for the whole call and
    // `args`/`call` are live SEXPs supplied by the builtin dispatcher.
    unsafe { crate::mainutils::seq::check1arg(args, call, name.as_ptr()) }
}

/// `R_NewHashedEnv(enclos, size)` on a live enclosure.
pub(crate) fn new_hashed_env(enclos: SEXP, size: c_int) -> SEXP {
    // SAFETY: `enclos` is a live environment; R_NewHashedEnv only allocates a
    // fresh environment and promotes it to a hashed frame.
    crate::sexp::envir::R_NewHashedEnv(enclos, size)
}

/// Lossy text of `type2char(t)`, or `None` when it has no name.
pub(crate) fn type2char_text(t: c_int) -> Option<String> {
    let raw = crate::mainutils::util_main::type2char(t);
    if raw.is_null() {
        return None;
    }
    // SAFETY: type2char returns null or a static NUL-terminated name.
    Some(
        unsafe { std::ffi::CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// Name of a primitive as a C string (`PRIMNAME`), empty for non-primitives.
pub(crate) fn primname_cstr(op: SEXP) -> &'static std::ffi::CStr {
    crate::eval::primitive::raw_primitive_entry(op)
        .and_then(|entry| std::ffi::CStr::from_bytes_until_nul(entry.name).ok())
        .unwrap_or(c"")
}

/// Elementwise complex map (`complex_unary_vec`) over a live CPLXSXP.
pub(crate) fn complex_unary(
    x: SEXP,
    f: fn(crate::sexp::ffi::Rcomplex) -> crate::sexp::ffi::Rcomplex,
) -> SEXP {
    // SAFETY: `x` is a live SEXP; complex_unary_vec reads it through checked
    // cells and allocates the result in the active owner.
    unsafe { crate::eval::complex_arith::complex_unary_vec(x, f) }
}

/// Closure-style argument matching against `names` (`match_formal_slots`).
pub(crate) fn match_formals(call: SEXP, args: SEXP, names: &[&str]) -> Vec<SEXP> {
    // SAFETY: `call` and `args` are live SEXPs supplied by the dispatcher;
    // matching only reads them and allocates the formals pairlist.
    crate::mainutils::match_mod::match_formal_slots(call, args, names)
}

/// Primitive binding for `name` (`make_primitive_binding`).
pub(crate) fn primitive_binding(name: &str, fallback_kind: SEXPTYPE) -> SEXP {
    // SAFETY: allocates a fresh PRIMSXP (or fallback) for a safe name.
    unsafe { crate::eval::primitive::make_primitive_binding(name, fallback_kind) }
}
