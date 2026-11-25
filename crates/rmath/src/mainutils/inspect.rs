#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/inspect.c — inspection/display utilities.
//!
//! This module ports the standalone display utilities used by R's
//! internal inspection functions.
//!
//! Ported standalone functions:
//!   pp (print indentation)

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib,
};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Display utilities
// ---------------------------------------------------------------------------

/// Print indentation (spaces) for nested display.
///
/// Writes `pre` levels of indentation (2 spaces per level).
pub fn pp(pre: i32) -> String {
    let spaces = (pre * 2) as usize;
    " ".repeat(spaces)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn R_LevelsSymbol() -> SEXP {
    crate::sexp::symbol::install_str("levels")
}

/// Checked STRSXP(1) scalar on the active session heap (`Rf_mkString`).
fn mk_string(text: &str) -> SEXP {
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .strings(&[text])
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked CHARSXP on the active session heap (`Rf_mkChar`).
fn mk_char(text: &str) -> SEXP {
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .character(text)
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// GNU `CHAR(x)` text of a CHARSXP, empty when not valid UTF-8.
fn char_text(x: SEXP) -> String {
    String::from_utf8(crate::mainutils::printutils::charsxp_bytes(x)).unwrap_or_default()
}

#[inline(always)]
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// Check if SEXP is a factor.
fn isFactor(s: SEXP) -> c_int {
    if s.is_null() {
        return 0;
    }
    let klass = getAttrib(s, R_ClassSymbol());
    if klass.is_null() || TYPEOF(klass) != SEXPTYPE::STRSXP || LENGTH(klass) < 2 {
        return 0;
    }
    let c1_str = char_text(STRING_ELT(klass, 0));
    let c2_str = char_text(STRING_ELT(klass, 1));
    (c1_str.as_str() == "factor" || c1_str == "ordered") as c_int
}

/// Get the type name for a SEXPTYPE (by raw i32 value).
fn sexptype2char(s: i32) -> &'static str {
    match s {
        x if x == SEXPTYPE::NILSXP => "NULL",
        x if x == SEXPTYPE::SYMSXP => "symbol",
        x if x == SEXPTYPE::LISTSXP => "pairlist",
        x if x == SEXPTYPE::CLOSXP => "closure",
        x if x == SEXPTYPE::ENVSXP => "environment",
        x if x == SEXPTYPE::PROMSXP => "promise",
        x if x == SEXPTYPE::LANGSXP => "language",
        x if x == SEXPTYPE::SPECIALSXP => "special",
        x if x == SEXPTYPE::BUILTINSXP => "builtin",
        x if x == SEXPTYPE::CHARSXP => "character",
        x if x == SEXPTYPE::LGLSXP => "logical",
        x if x == SEXPTYPE::INTSXP => "integer",
        x if x == SEXPTYPE::REALSXP => "double",
        x if x == SEXPTYPE::CPLXSXP => "complex",
        x if x == SEXPTYPE::STRSXP => "character",
        x if x == SEXPTYPE::DOTSXP => "...",
        x if x == SEXPTYPE::ANYSXP => "any",
        x if x == SEXPTYPE::VECSXP => "list",
        x if x == SEXPTYPE::EXPRSXP => "expression",
        x if x == SEXPTYPE::EXTPTRSXP => "externalptr",
        x if x == SEXPTYPE::WEAKREFSXP => "weakref",
        x if x == SEXPTYPE::RAWSXP => "raw",
        x if x == SEXPTYPE::OBJSXP => "S4",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// typeof() — R's typeof() builtin
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// invisible() — mark result as invisible
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// is.null() — null check
// ---------------------------------------------------------------------------

/// do_isnull — R's is.null() function.
///
/// Ported from R's isnull() in inspect.c.
pub fn do_isnull(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(_op, args);
    let s = CAR(args);
    Rf_ScalarLogical(if s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP {
        TRUE
    } else {
        FALSE
    })
}

// ---------------------------------------------------------------------------
// length() — object length
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// formals() — function arguments
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// args() — function arguments (actual)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// environment() — get function environment
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// str() — object structure display
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// names() — object names
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// dim() — dimensions
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// dimnames() — dimension names
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// attributes() — list attributes
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// class() — object class
// ---------------------------------------------------------------------------

/// do_classname — internal class display.
///
/// Returns the class of an object as a character vector.
/// Ported from R's classname() in inspect.c.
pub fn do_classname(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    let x = CAR(args);

    if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP {
        return R_NilValue();
    }

    let klass = getAttrib(x, R_ClassSymbol());
    if klass.is_null() {
        // Return implicit class
        let t = TYPEOF(x);
        let type_name = sexptype2char(t);
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _ans_guard = protect(ans);
        SET_STRING_ELT(ans, 0, mk_char(&type_name));
        ans
    } else {
        klass
    }
}

// ---------------------------------------------------------------------------
// levels() — factor levels
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// structure() — structure display
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// strformat() — str format control
// ---------------------------------------------------------------------------

/// do_strformat — internal str() format control (no-op).
pub fn do_strformat(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn test_pp_zero() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(pp(0), "");
    }

    #[test]
    fn test_pp_one() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(pp(1), "  ");
    }

    #[test]
    fn test_pp_five() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(pp(5), "          ");
    }

    #[test]
    fn test_typeof_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_ScalarInteger(42);
            let _x_guard = protect(x);
            let result = do_typeof(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            let s = CHAR(STRING_ELT(result, 0));
            let type_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
            assert_eq!(type_str, "integer");
        }
    }

    #[test]
    fn test_typeof_real() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_ScalarReal(3.14);
            let _x_guard = protect(x);
            let result = do_typeof(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            let s = CHAR(STRING_ELT(result, 0));
            let type_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
            assert_eq!(type_str, "double");
        }
    }

    #[test]
    fn test_typeof_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_typeof(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            let s = CHAR(STRING_ELT(result, 0));
            let type_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
            assert_eq!(type_str, "NULL");
        }
    }

    #[test]
    fn test_isnull_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_isnull(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn test_isnull_not_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_ScalarInteger(1);
            let _x_guard = protect(x);
            let result = do_isnull(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(result), FALSE);
        }
    }

    #[test]
    fn test_length_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector(SEXPTYPE::INTSXP, 5);
            let _x_guard = protect(x);
            let result = do_length(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(*INTEGER(result), 5);
        }
    }

    #[test]
    fn test_length_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_length(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert_eq!(*INTEGER(result), 0);
        }
    }

    #[test]
    fn test_sexptype2char() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(sexptype2char(SEXPTYPE::INTSXP.into()), "integer");
        assert_eq!(sexptype2char(SEXPTYPE::REALSXP.into()), "double");
        assert_eq!(sexptype2char(SEXPTYPE::STRSXP.into()), "character");
        assert_eq!(sexptype2char(SEXPTYPE::LGLSXP.into()), "logical");
        assert_eq!(sexptype2char(SEXPTYPE::VECSXP.into()), "list");
        assert_eq!(sexptype2char(SEXPTYPE::NILSXP.into()), "NULL");
    }
}
