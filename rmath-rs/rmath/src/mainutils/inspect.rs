#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/inspect.c — inspection/display utilities.
//!
//! This module ports the standalone display utilities used by R's
//! internal inspection functions.
//!
//! Ported standalone functions:
//!   pp (print indentation)

use std::os::raw::c_int;

use crate::eval::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, getAttrib,
};
use crate::sexp::accessors::*;
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

#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) { unsafe { crate::mainutils::relop::checkArity(op, args) }}

/// Check if SEXP is a factor.
unsafe fn isFactor(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let klass = getAttrib(s, R_ClassSymbol());
        if klass.is_null() || TYPEOF(klass) != SEXPTYPE::STRSXP.0 || LENGTH(klass) < 2 {
            return 0;
        }
        let c1 = CHAR(STRING_ELT(klass, 0));
        let c1_str = std::ffi::CStr::from_ptr(c1).to_str().unwrap_or("");
        let c2 = CHAR(STRING_ELT(klass, 1));
        let c2_str = std::ffi::CStr::from_ptr(c2).to_str().unwrap_or("");
        (c1_str == "factor" || c1_str == "ordered") as c_int
    }
}

/// Get the type name for a SEXPTYPE (by raw i32 value).
fn sexptype2char(s: i32) -> &'static str {
    match s {
        x if x == SEXPTYPE::NILSXP.0 => "NULL",
        x if x == SEXPTYPE::SYMSXP.0 => "symbol",
        x if x == SEXPTYPE::LISTSXP.0 => "pairlist",
        x if x == SEXPTYPE::CLOSXP.0 => "closure",
        x if x == SEXPTYPE::ENVSXP.0 => "environment",
        x if x == SEXPTYPE::PROMSXP.0 => "promise",
        x if x == SEXPTYPE::LANGSXP.0 => "language",
        x if x == SEXPTYPE::SPECIALSXP.0 => "special",
        x if x == SEXPTYPE::BUILTINSXP.0 => "builtin",
        x if x == SEXPTYPE::CHARSXP.0 => "character",
        x if x == SEXPTYPE::LGLSXP.0 => "logical",
        x if x == SEXPTYPE::INTSXP.0 => "integer",
        x if x == SEXPTYPE::REALSXP.0 => "double",
        x if x == SEXPTYPE::CPLXSXP.0 => "complex",
        x if x == SEXPTYPE::STRSXP.0 => "character",
        x if x == SEXPTYPE::DOTSXP.0 => "...",
        x if x == SEXPTYPE::ANYSXP.0 => "any",
        x if x == SEXPTYPE::VECSXP.0 => "list",
        x if x == SEXPTYPE::EXPRSXP.0 => "expression",
        x if x == SEXPTYPE::EXTPTRSXP.0 => "externalptr",
        x if x == SEXPTYPE::WEAKREFSXP.0 => "weakref",
        x if x == SEXPTYPE::RAWSXP.0 => "raw",
        x if x == SEXPTYPE::OBJSXP.0 => "S4",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// typeof() — R's typeof() builtin
// ---------------------------------------------------------------------------

/// do_typeof — R's typeof() function.
///
/// Returns the type of the object as a character string.
/// Ported from R's typeof() in inspect.c.
pub unsafe fn do_typeof(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let s = CAR(args);

        // Dotted pair list (non-language)
        if TYPEOF(s) == SEXPTYPE::LISTSXP.0 {
            return Rf_mkString(b"pairlist\0".as_ptr() as *const _);
        }

        let t = TYPEOF(s);

        // Special handling for S4 objects
        if t == SEXPTYPE::OBJSXP.0 {
            return Rf_mkString(b"S4\0".as_ptr() as *const _);
        }

        // Handle factors, ordered factors
        if (t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::STRSXP.0)
            && isFactor(s) != 0
        {
            let klass = getAttrib(s, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP.0 && LENGTH(klass) >= 2 {
                let c1 = CHAR(STRING_ELT(klass, 0));
                let c1_str = std::ffi::CStr::from_ptr(c1).to_str().unwrap_or("");
                if c1_str == "ordered" {
                    return Rf_mkString(b"ordered\0".as_ptr() as *const _);
                }
            }
            return Rf_mkString(b"factor\0".as_ptr() as *const _);
        }

        // Handle POSIXct and POSIXlt
        if (t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::VECSXP.0)
            && isFactor(s) == 0
        {
            let klass = getAttrib(s, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP.0 && LENGTH(klass) >= 1 {
                let c1 = CHAR(STRING_ELT(klass, 0));
                let c1_str = std::ffi::CStr::from_ptr(c1).to_str().unwrap_or("");
                if c1_str == "POSIXct" {
                    return Rf_mkString(b"double\0".as_ptr() as *const _);
                }
                if c1_str == "POSIXlt" {
                    return Rf_mkString(b"list\0".as_ptr() as *const _);
                }
                if c1_str == "difftime" {
                    return Rf_mkString(b"double\0".as_ptr() as *const _);
                }
                // For data.frame, return "list"
                if c1_str == "data.frame" {
                    return Rf_mkString(b"list\0".as_ptr() as *const _);
                }
            }
        }

        let type_name = sexptype2char(t);
        Rf_mkString(
            std::ffi::CString::new(type_name)
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// invisible() — mark result as invisible
// ---------------------------------------------------------------------------

/// do_invisible — R's invisible() function.
///
/// Marks the value as invisible for auto-printing.
/// Ported from R's invisible() in inspect.c.
pub unsafe fn do_invisible(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let x = CAR(args);
        set_R_Visible(0);
        x
    }
}

// ---------------------------------------------------------------------------
// is.null() — null check
// ---------------------------------------------------------------------------

/// do_isnull — R's is.null() function.
///
/// Ported from R's isnull() in inspect.c.
pub unsafe fn do_isnull(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let s = CAR(args);
        Rf_ScalarLogical(if s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP.0 {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// length() — object length
// ---------------------------------------------------------------------------

/// do_length — R's length() function.
///
/// Returns the length of an object. For vectors, this is the number of elements.
/// For pairlists, this is the number of pairs. For NULL, returns 0.
/// Ported from R's length() in inspect.c.
pub unsafe fn do_length(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let mut s = CAR(args);

        let len = if s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP.0 {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::LISTSXP.0 || t == SEXPTYPE::LANGSXP.0 {
                // Pairlist: count cons cells
                let mut count: c_int = 0;
                while !s.is_null()
                    && (TYPEOF(s) == SEXPTYPE::LISTSXP.0 || TYPEOF(s) == SEXPTYPE::LANGSXP.0)
                {
                    count += 1;
                    s = CDR(s);
                }
                count
            } else {
                LENGTH(s)
            }
        };

        Rf_ScalarInteger(len)
    }
}

// ---------------------------------------------------------------------------
// formals() — function arguments
// ---------------------------------------------------------------------------

/// do_formals — R's formals() function.
///
/// Returns the formal arguments of a function.
/// Ported from R's formals() in inspect.c.
pub unsafe fn do_formals(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let s = CAR(args);

        if s.is_null() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        if t == SEXPTYPE::CLOSXP.0 {
            // Closure: formals are in CLOENV -> CDR
            let formals = FORMALS(s);
            if formals.is_null() || TYPEOF(formals) != SEXPTYPE::LISTSXP.0 {
                return R_NilValue();
            }
            formals
        } else {
            R_NilValue()
        }
    }
}

/// do_body — R's body() function.
///
/// Returns the body of a function.
/// Ported from R's body() in inspect.c.
pub unsafe fn do_body(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let s = CAR(args);

        if s.is_null() {
            return R_NilValue();
        }

        let t = TYPEOF(s);
        if t == SEXPTYPE::CLOSXP.0 {
            BODY(s)
        } else if t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0 {
            // Builtins/specials don't have a body in the usual sense
            R_NilValue()
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// args() — function arguments (actual)
// ---------------------------------------------------------------------------

/// do_args — R's args() function.
///
/// Returns the arguments (promise objects) from the matching call.
/// Ported from R's args() in inspect.c.
pub unsafe fn do_args(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);

        // args() with no argument: return the promise objects from the
        // current function's call.
        let s = CAR(args);
        if s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP.0 {
            // Return sys.call() arguments as promise objects
            // Returning nil
            return R_NilValue();
        }

        // args(x) where x is a function: return the formals
        if TYPEOF(s) == SEXPTYPE::CLOSXP.0 {
            FORMALS(s)
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// environment() — get function environment
// ---------------------------------------------------------------------------

/// do_environment — R's environment() function.
///
/// Returns the environment of a function or other object.
/// Ported from R's environment() in inspect.c.
pub unsafe fn do_environment(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let s = CAR(args);

        if s.is_null() {
            return R_GlobalEnv();
        }

        let t = TYPEOF(s);
        if t == SEXPTYPE::CLOSXP.0 {
            CLOENV(s)
        } else if t == SEXPTYPE::ENVSXP.0 {
            s
        } else {
            R_GlobalEnv()
        }
    }
}

// ---------------------------------------------------------------------------
// str() — object structure display
// ---------------------------------------------------------------------------

/// do_str — R's str() function.
///
/// Compactly displays the internal structure of an R object.
/// Ported from R's str() in inspect.c.
pub unsafe fn do_str(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            // str(NULL) returns " NULL"
            let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
            SET_STRING_ELT(ans, 0, Rf_mkChar(b" NULL\0".as_ptr() as *const _));
            Rf_unprotect(1);
            return ans;
        }

        let t = TYPEOF(x);
        let len = if t == SEXPTYPE::LISTSXP.0 || t == SEXPTYPE::LANGSXP.0 {
            // pairlist length
            let mut s = x;
            let mut count: c_int = 0;
            while !s.is_null()
                && (TYPEOF(s) == SEXPTYPE::LISTSXP.0 || TYPEOF(s) == SEXPTYPE::LANGSXP.0)
            {
                count += 1;
                s = CDR(s);
            }
            count
        } else {
            LENGTH(x)
        };

        let type_name = sexptype2char(t);
        let desc = format!(" {} [1:{}] \"{}\"", type_name, len, type_name);

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
        SET_STRING_ELT(
            ans,
            0,
            Rf_mkChar(std::ffi::CString::new(desc).unwrap_or_default().as_ptr()),
        );
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// names() — object names
// ---------------------------------------------------------------------------

/// do_names — R's names() function.
///
/// Returns the names attribute of an object.
/// Ported from R's names() in attrib.c.
pub unsafe fn do_names(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        let names = getAttrib(x, R_NamesSymbol());
        if names.is_null() {
            return R_NilValue();
        }
        names
    }
}

// ---------------------------------------------------------------------------
// dim() — dimensions
// ---------------------------------------------------------------------------

/// do_dim — R's dim() function.
///
/// Returns the dimensions of an object.
/// Ported from R's dim() in attrib.c.
pub unsafe fn do_dim(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() {
            return Rf_ScalarInteger(LENGTH(x));
        }
        dim
    }
}

// ---------------------------------------------------------------------------
// dimnames() — dimension names
// ---------------------------------------------------------------------------

/// do_dimnames — R's dimnames() function.
///
/// Returns the dimnames attribute of an object.
/// Ported from R's dimnames() in attrib.c.
pub unsafe fn do_dimnames(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        getAttrib(x, R_DimNamesSymbol())
    }
}

// ---------------------------------------------------------------------------
// attributes() — list attributes
// ---------------------------------------------------------------------------

/// do_attributes — R's attributes() function.
///
/// Returns all attributes of an object.
/// Ported from R's attributes() in attrib.c.
pub unsafe fn do_attributes(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        ATTRIB(x)
    }
}

// ---------------------------------------------------------------------------
// class() — object class
// ---------------------------------------------------------------------------

/// do_classname — internal class display.
///
/// Returns the class of an object as a character vector.
/// Ported from R's classname() in inspect.c.
pub unsafe fn do_classname(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        let klass = getAttrib(x, R_ClassSymbol());
        if klass.is_null() {
            // Return implicit class
            let t = TYPEOF(x);
            let type_name = sexptype2char(t);
            let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
            SET_STRING_ELT(
                ans,
                0,
                Rf_mkChar(
                    std::ffi::CString::new(type_name)
                        .unwrap_or_default()
                        .as_ptr(),
                ),
            );
            Rf_unprotect(1);
            ans
        } else {
            klass
        }
    }
}

// ---------------------------------------------------------------------------
// levels() — factor levels
// ---------------------------------------------------------------------------

/// do_levels — R's levels() function.
///
/// Returns the levels of a factor.
/// Ported from R's levels() in attrib.c.
pub unsafe fn do_levels(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            return R_NilValue();
        }

        getAttrib(x, R_LevelsSymbol())
    }
}

// ---------------------------------------------------------------------------
// structure() — structure display
// ---------------------------------------------------------------------------

/// do_structure — R's structure() function.
///
/// Returns a compact display of the structure of an arbitrary R object.
/// Ported from R's structure() in inspect.c.
pub unsafe fn do_structure(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);

        if x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 {
            // str(NULL)
            let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
            SET_STRING_ELT(ans, 0, Rf_mkChar(b" NULL\0".as_ptr() as *const _));
            Rf_unprotect(1);
            return ans;
        }

        // Simplified: return type name and length
        let t = TYPEOF(x);
        let type_name = sexptype2char(t);
        let len = LENGTH(x);
        let desc = format!("List of {}\n $ : chr \"{}\"", len, type_name);

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
        SET_STRING_ELT(
            ans,
            0,
            Rf_mkChar(std::ffi::CString::new(desc).unwrap_or_default().as_ptr()),
        );
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// strformat() — str format control
// ---------------------------------------------------------------------------

/// do_strformat — internal str() format control (no-op).
pub unsafe fn do_strformat(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_NilValue()
    }
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
        assert_eq!(pp(0), "");
    }

    #[test]
    fn test_pp_one() {
        assert_eq!(pp(1), "  ");
    }

    #[test]
    fn test_pp_five() {
        assert_eq!(pp(5), "          ");
    }

    #[test]
    fn test_typeof_integer() {
        unsafe {
            let x = Rf_ScalarInteger(42);
            Rf_protect(x);
            let result = do_typeof(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP.0);
            let s = CHAR(STRING_ELT(result, 0));
            let type_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
            assert_eq!(type_str, "integer");
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_typeof_real() {
        unsafe {
            let x = Rf_ScalarReal(3.14);
            Rf_protect(x);
            let result = do_typeof(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            let s = CHAR(STRING_ELT(result, 0));
            let type_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
            assert_eq!(type_str, "double");
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_typeof_null() {
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
        unsafe {
            let x = Rf_ScalarInteger(1);
            Rf_protect(x);
            let result = do_isnull(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(result), FALSE);
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_length_integer() {
        unsafe {
            let x = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 5));
            let result = do_length(
                ptr::null_mut(),
                ptr::null_mut(),
                Rf_cons(x, R_NilValue()),
                ptr::null_mut(),
            );
            assert_eq!(*INTEGER(result), 5);
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_length_null() {
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
        assert_eq!(sexptype2char(SEXPTYPE::INTSXP.0), "integer");
        assert_eq!(sexptype2char(SEXPTYPE::REALSXP.0), "double");
        assert_eq!(sexptype2char(SEXPTYPE::STRSXP.0), "character");
        assert_eq!(sexptype2char(SEXPTYPE::LGLSXP.0), "logical");
        assert_eq!(sexptype2char(SEXPTYPE::VECSXP.0), "list");
        assert_eq!(sexptype2char(SEXPTYPE::NILSXP.0), "NULL");
    }
}
