#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Local SEXP/option helper shims (GetOption1, type predicates, attribute
//! wrappers) used by the error paths.

use super::*;

// ---------------------------------------------------------------------------
// GetOption1 helper (simplified)
// ---------------------------------------------------------------------------

/// Delegates to the real GetOption1 implementation in options.rs.
pub(super) unsafe fn GetOption1(sym: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::GetOption1(sym) }
}

/// Check if an SEXP is a function (CLOSXP or BUILTINSXP).
pub(super) unsafe fn isFunction(s: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(s);
        (t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP) as c_int
    }
}

/// Check if an SEXP is a language object.
pub(super) unsafe fn isLanguage(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LANGSXP) as c_int }
}

/// Check if an SEXP is an expression.
pub(super) unsafe fn isExpression(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::EXPRSXP) as c_int }
}

/// Check if an SEXP is a string vector.
pub(super) unsafe fn isString(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::STRSXP) as c_int }
}

/// Check if an SEXP is a logical vector.
pub(super) unsafe fn isLogical(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LGLSXP) as c_int }
}

/// Check if an SEXP is an integer vector.
pub(super) unsafe fn isInteger(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::INTSXP) as c_int }
}

/// Check if an SEXP is a real vector.
pub(super) unsafe fn isReal(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::REALSXP) as c_int }
}

/// Convert SEXP to logical (simplified).
pub(super) unsafe fn asLogical(s: SEXP) -> c_int {
    unsafe {
        if isLogical(s) != 0 && LENGTH(s) >= 1 {
            *LOGICAL(s)
        } else if isInteger(s) != 0 && LENGTH(s) >= 1 {
            *INTEGER(s)
        } else if isReal(s) != 0 && LENGTH(s) >= 1 {
            if *REAL(s) == 0.0_f64 { 0 } else { 1 }
        } else {
            crate::sexp::ffi::NA_INTEGER
        }
    }
}

/// Check if an SEXP is NULL.
pub(super) unsafe fn isNull(s: SEXP) -> c_int {
    unsafe { (s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP) as c_int }
}

/// Check if a string SEXP is valid (non-NA).
pub(super) unsafe fn isValidString(s: SEXP) -> c_int {
    unsafe {
        if isString(s) == 0 || LENGTH(s) < 1 {
            return 0;
        }
        let elt = STRING_ELT(s, 0);
        if elt.is_null() {
            return 0;
        }
        1 // Simplified — full version checks for NA_STRING
    }
}

/// Get C string from CHARSXP (simplified).
pub(super) unsafe fn CHAR_local(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() || TYPEOF(s) != SEXPTYPE::CHARSXP {
            return b"\0" as *const u8 as *const c_char;
        }
        crate::sexp::accessors::CHAR(s)
    }
}

pub(super) unsafe fn translateChar(s: SEXP) -> *const c_char {
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
pub(super) unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

pub(super) unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(x) }
}

pub(super) unsafe fn ScalarLogical(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarLogical(x) }
}

/// Get/set class attribute (simplified).
pub(super) unsafe fn classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, R_ClassSymbol(), klass); // Uses imported R_ClassSymbol
        x
    }
}

/// Wrapper for getAttrib using the real implementation.
#[inline]
pub(super) unsafe fn getAttrib_wrap(x: SEXP, which: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, which) }
}

/// Wrapper for setAttrib using the real implementation.
#[inline]
pub(super) unsafe fn setAttrib_wrap(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, which, value);
    }
}

/// Get the number of arguments (length of pairlist).
pub(super) unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        let mut count: c_int = 0;
        let mut p = x;
        while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP {
            count += 1;
            p = CDR(p);
        }
        count
    }
}
