//! nchar/substr/substring family, case conversion, file extensions

use super::*;

// ---------------------------------------------------------------------------
// do_nchar — string length
// ---------------------------------------------------------------------------

/// R's `nchar(x, type = "chars", allowNA = FALSE, keepNA = NA)`.
///
/// GNU default `type="chars"` counts Unicode code points, not bytes.
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        super::vector_text_entry::evaluate(args, rho, super::vector_text_owned::Operation::Count)
    }
}

fn file_ext_of(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some(dot) = base.rfind('.') else {
        return String::new();
    };
    let stem = &base[..dot];
    let ext = &base[dot + 1..];
    if ext.is_empty()
        || !ext.chars().all(|c| c.is_ascii_alphanumeric())
        || !stem.chars().any(|c| c != '.')
    {
        return String::new();
    }
    ext.to_string()
}

/// GNU `tools::file_ext(x)`.
pub unsafe fn do_file_ext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let ext = file_ext_of(&s);
            let c = CString::new(ext).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(c.as_ptr()));
        }
        result
    }
}

/// GNU `tools::file_path_sans_ext(x)`.
pub unsafe fn do_file_path_sans_ext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let ext = file_ext_of(&s);
            let out = if ext.is_empty() {
                s
            } else {
                let suffix = format!(".{ext}");
                match s.rfind(&suffix) {
                    Some(idx) => s[..idx].to_string(),
                    None => s,
                }
            };
            let c = CString::new(out).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(c.as_ptr()));
        }
        result
    }
}

pub(crate) fn unicode_display_width(ch: char) -> usize {
    crate::mainutils::unicode_width::columns(ch)
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { substring_entry(args, _rho, super::substring_owned::Operation::Extract) }
}

pub unsafe fn do_substrgets(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { substring_entry(args, _rho, super::substring_owned::Operation::Replace) }
}

unsafe fn substring_entry(
    args: SEXP,
    rho: SEXP,
    operation: super::substring_owned::Operation,
) -> SEXP {
    use crate::sexp::{
        object::Sexp,
        owner::{OwnerToken, with_runtime},
    };
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let own = |raw| {
        owner
            .sexp(raw)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    };
    let args = own(args);
    let environment = own(rho);
    let na = own(unsafe { crate::sexp::globals::R_NaString() });
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| crate::sexp::context::r_error("substring requires a managed runtime"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&weak, |access| {
            super::substring_owned::evaluate(
                access,
                &args,
                &na,
                operation,
                |access, value, kind| {
                    access.with_native(|original| {
                        let raw = if kind == SEXPTYPE::STRSXP {
                            let factory = original.node_factory();
                            let arguments =
                                factory.pairlist_cell(value, &factory.nil(), &factory.nil())?;
                            unsafe {
                                crate::mainutils::essentials_basic::do_as_character(
                                    factory.nil().as_raw(),
                                    factory.nil().as_raw(),
                                    arguments.as_raw(),
                                    environment.as_raw(),
                                )
                            }
                        } else {
                            unsafe {
                                crate::main::coerce::coerceVector(value.as_raw(), kind.as_c_int())
                            }
                        };
                        original.sexp(raw)?.into_owned()
                    })
                },
            )
        })
    }));
    owner
        .require_active()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    match outcome {
        Ok(Ok(Ok(value))) => value.as_raw(),
        Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// GNU `strrep(x, times)` — recycle `x` and `times` to a common length.
pub unsafe fn do_strrep(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        super::vector_text_entry::evaluate(args, rho, super::vector_text_owned::Operation::Repeat)
    }
}

/// R's `substring(text, first, last=NULL)` — base::substring is an R
/// wrapper over the same internal that rep_lens `text` to the common
/// length max(len(text), len(first), len(last)) first (character.R);
/// zero-length text stays zero-length.
pub unsafe fn do_substring(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { substring_entry(args, _rho, super::substring_owned::Operation::Recycle) }
}

// ---------------------------------------------------------------------------
// String case conversion
// ---------------------------------------------------------------------------

/// R's `tolower(x)`.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, true) }
}

/// R's `toupper(x)`.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, false) }
}

/// GNU `casefold(x, upper=FALSE)`.
pub unsafe fn do_casefold(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut upper = false;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let u = CAR(rest);
            if !u.is_null() && u != R_NilValue() {
                if TYPEOF(u) == SEXPTYPE::LGLSXP && XLENGTH(u) > 0 {
                    upper = *LOGICAL(u) == TRUE;
                } else if TYPEOF(u) == SEXPTYPE::INTSXP && XLENGTH(u) > 0 {
                    upper = *INTEGER(u) != 0 && *INTEGER(u) != NA_INTEGER;
                }
            }
        }
        if upper {
            do_toupper(call, op, args, rho)
        } else {
            do_tolower(call, op, args, rho)
        }
    }
}

unsafe fn do_case_convert(args: SEXP, to_lower: bool) -> SEXP {
    unsafe { crate::mainutils::character::case_entry(args, !to_lower) }
}

pub(crate) unsafe fn as_character_element_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => STRING_ELT(x, i) == crate::sexp::globals::R_NaString(),
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) == NA_LOGICAL,
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(x, i as c_int) == NA_INTEGER,
            t if t == SEXPTYPE::REALSXP => {
                REAL_ELT(x, i as c_int).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// String manipulation: trimws, sprintf, gsub, sub, strsplit
// ---------------------------------------------------------------------------
