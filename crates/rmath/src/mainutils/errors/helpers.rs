#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Local SEXP/option helper shims (GetOption1, type predicates, attribute
//! wrappers) used by the error paths.

use super::*;

/// Wrap a live, rooted argument pointer for checked reads. Callers are raw
/// error-path entry points inside the active session, so the token's
/// retain/exclusion contract matches the raw accessors this replaces.
fn owned(pointer: SEXP) -> crate::sexp::object::Sexp<'static> {
    // SAFETY: argument pointers name live, rooted R objects in the active
    // session — the ambient contract of the raw accessors below.
    unsafe { crate::sexp::owner::OwnerToken::current() }
        .and_then(|owner| owner.sexp(pointer))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

// ---------------------------------------------------------------------------
// GetOption1 helper (simplified)
// ---------------------------------------------------------------------------

/// Delegates to the real GetOption1 implementation in options.rs.
pub(super) fn GetOption1(sym: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::GetOption1(sym) }
}

/// Check if an SEXP is a function (CLOSXP or BUILTINSXP).
pub(super) fn isFunction(s: SEXP) -> c_int {
    {
        let t = TYPEOF(s);
        (t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP) as c_int
    }
}

/// Check if an SEXP is a language object.
pub(super) fn isLanguage(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::LANGSXP) as c_int
}

/// Check if an SEXP is an expression.
pub(super) fn isExpression(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::EXPRSXP) as c_int
}

/// Check if an SEXP is a string vector.
pub(super) fn isString(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::STRSXP) as c_int
}

/// Check if an SEXP is a logical vector.
pub(super) fn isLogical(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::LGLSXP) as c_int
}

/// Check if an SEXP is an integer vector.
pub(super) fn isInteger(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::INTSXP) as c_int
}

/// Check if an SEXP is a real vector.
pub(super) fn isReal(s: SEXP) -> c_int {
    (TYPEOF(s) == SEXPTYPE::REALSXP) as c_int
}

/// Convert SEXP to logical (simplified).
pub(super) fn asLogical(s: SEXP) -> c_int {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let value = owned(s);
    if isLogical(s) != 0 && LENGTH(s) >= 1 {
        value.try_logical_elt(0).unwrap_or_else(|e| fail(e))
    } else if isInteger(s) != 0 && LENGTH(s) >= 1 {
        value.try_integer_elt(0).unwrap_or_else(|e| fail(e))
    } else if isReal(s) != 0 && LENGTH(s) >= 1 {
        if value.try_real_elt(0).unwrap_or_else(|e| fail(e)) == 0.0_f64 {
            0
        } else {
            1
        }
    } else {
        crate::sexp::ffi::NA_INTEGER
    }
}

/// Check if an SEXP is NULL.
pub(super) fn isNull(s: SEXP) -> c_int {
    (s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP) as c_int
}

/// Check if a string SEXP is valid (non-NA).
pub(super) fn isValidString(s: SEXP) -> c_int {
    {
        if isString(s) == 0 || LENGTH(s) < 1 {
            return 0;
        }
        let elt = owned(s)
            .try_string_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if elt.is_nil() {
            return 0;
        }
        1 // Simplified — full version checks for NA_STRING
    }
}

/// Get C string from CHARSXP (simplified).
pub(super) fn CHAR_local(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() || TYPEOF(s) != SEXPTYPE::CHARSXP {
            return b"\0" as *const u8 as *const c_char;
        }
        crate::sexp::accessors::CHAR(s)
    }
}

pub(super) fn translateChar(s: SEXP) -> *const c_char {
    unsafe {
        let r = crate::sexp::accessors::translateChar(s);
        if r.is_null() {
            b"\0" as *const u8 as *const c_char
        } else {
            r
        }
    }
}

/// Check argument arity (simplified).
pub(super) fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

pub(super) fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(x) }
}

pub(super) fn ScalarLogical(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarLogical(x) }
}

/// Get/set class attribute (simplified).
pub(super) fn classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, R_ClassSymbol(), klass); // Uses imported R_ClassSymbol
        x
    }
}

/// Wrapper for getAttrib using the real implementation.
#[inline]
pub(super) fn getAttrib_wrap(x: SEXP, which: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, which) }
}

/// Wrapper for setAttrib using the real implementation.
#[inline]
pub(super) fn setAttrib_wrap(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, which, value);
    }
}

/// Get the number of arguments (length of pairlist).
pub(super) fn length(x: SEXP) -> c_int {
    {
        let mut count: c_int = 0;
        let mut p = x;
        while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP {
            count += 1;
            p = CDR(p);
        }
        count
    }
}
