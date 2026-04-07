#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R-level entry points: do_coerce, do_ascoerce, do_is, do_isna, etc.

use std::ffi::CStr;
use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::main::relop::PRIMVAL;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::helpers::{
    CLEAR_ATTRIB, IS_S4_OBJECT, R_NaString, error, errorcall, isArray, isFunction, isLogical,
    isNull, isNumeric, isString, isVector, xlength,
};
use super::vector::{asLogical2, ascommon, coerceVector};

// ---------------------------------------------------------------------------
// asRbool / asBool -- coerce to boolean (error on NA)
// ---------------------------------------------------------------------------

/// Coerce to Rboolean (c_int), erroring on NA_LOGICAL.
/// This matches R's asRboolean() from coerce.c.
pub unsafe fn asRbool(x: SEXP, call: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, call);
        if ans == NA_LOGICAL {
            errorcall(call, "NA in coercion to boolean");
        }
        ans
    }
}

/// Coerce to bool, erroring on NA_LOGICAL.
/// This matches R's asBool() from coerce.c.
pub unsafe fn asBool(x: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, R_NilValue());
        if ans == NA_LOGICAL {
            error("NA in coercion to boolean");
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// asCharacterFactor -- convert factor to character
// ---------------------------------------------------------------------------

/// Convert a factor to a character vector using its levels.
///
/// This is R's `asCharacterFactor()` from coerce.c.
pub unsafe fn asCharacterFactor(x: SEXP) -> SEXP {
    unsafe {
        let n = xlength(x);
        let labels = getAttrib(x, R_LevelsSymbol());
        if TYPEOF(labels) != SEXPTYPE::STRSXP.0 {
            error("malformed factor");
        }
        let nl = LENGTH(labels);

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
        for i in 0..n {
            let ii = INTEGER_ELT(x, i as c_int);
            if ii == NA_INTEGER {
                SET_STRING_ELT(ans, i, R_NaString());
            } else if ii >= 1 && ii <= nl {
                SET_STRING_ELT(ans, i, STRING_ELT(labels, (ii - 1) as R_xlen_t));
            } else {
                error("malformed factor");
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// R-level entry points (do_* functions)
// ---------------------------------------------------------------------------

/// R-level `as.character()` for factors (internal).
pub unsafe fn do_asCharacterFactor(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        asCharacterFactor(x)
    }
}

/// R-level coercion entry point (`as.logical`, `as.integer`, etc.).
///
/// This is the `do_asatomic()` function from coerce.c, handling
/// `as.character`, `as.integer`, `as.double`, `as.complex`, `as.logical`, `as.raw`.
pub unsafe fn do_asatomic(call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let op0 = PRIMVAL(op);
        let mut type_: c_int = SEXPTYPE::STRSXP.0;

        match op0 {
            0 => {}                           // as.character
            1 => type_ = SEXPTYPE::INTSXP.0,  // as.integer
            2 => type_ = SEXPTYPE::REALSXP.0, // as.double
            3 => type_ = SEXPTYPE::CPLXSXP.0, // as.complex
            4 => type_ = SEXPTYPE::LGLSXP.0,  // as.logical
            5 => type_ = SEXPTYPE::RAWSXP.0,  // as.raw
            _ => {}
        }

        let x = CAR(args);
        if TYPEOF(x) == type_ {
            if isNull(ATTRIB(x)) {
                return x;
            }
            // Duplicate and clear attributes
            let ans = Rf_protect(Rf_allocVector3(type_, xlength(x)));
            // Copy data
            let src = DATAPTR(x);
            let dst = DATAPTR(ans);
            let byte_len = xlength(x) as usize
                * match SEXPTYPE(type_) {
                    SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                    SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                    SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                    SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                    _ => std::mem::size_of::<SEXP>(),
                };
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, byte_len);
            }
            CLEAR_ATTRIB(ans);
            Rf_unprotect(1);
            return ans;
        }

        let ans = coerceVector(x, type_);
        CLEAR_ATTRIB(ans);
        ans
    }
}

/// R-level `as.vector()` entry point.
///
/// This is the `do_asvector()` function from coerce.c.
pub unsafe fn do_asvector(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // For now, handle the simple case of coercing to the same type
        // or to a specified type via the second argument
        if args.is_null() || isNull(CDR(args)) {
            return x;
        }
        let mode_str = CADR(args);
        if !isString(mode_str) || LENGTH(mode_str) != 1 {
            error("invalid 'mode' argument");
        }

        let mode_chars = CHAR(STRING_ELT(mode_str, 0));
        if mode_chars.is_null() {
            error("invalid 'mode' argument");
        }
        let mode = CStr::from_ptr(mode_chars).to_str().unwrap_or("");

        let type_: c_int = match mode {
            "logical" => SEXPTYPE::LGLSXP.0,
            "integer" => SEXPTYPE::INTSXP.0,
            "double" | "numeric" => SEXPTYPE::REALSXP.0,
            "complex" => SEXPTYPE::CPLXSXP.0,
            "character" => SEXPTYPE::STRSXP.0,
            "raw" => SEXPTYPE::RAWSXP.0,
            "list" => SEXPTYPE::VECSXP.0,
            "expression" => SEXPTYPE::EXPRSXP.0,
            "pairlist" => SEXPTYPE::LISTSXP.0,
            "symbol" | "name" => SEXPTYPE::SYMSXP.0,
            "function" => SEXPTYPE::CLOSXP.0,
            "any" => return x,
            _ => {
                error("invalid 'mode' argument");
                0 // unreachable
            }
        };

        // If already the right type
        if TYPEOF(x) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    if isNull(ATTRIB(x)) {
                        return x;
                    }
                    let ans = Rf_protect(Rf_allocVector3(type_, xlength(x)));
                    let src = DATAPTR(x);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x) as usize * elem_size,
                        );
                    }
                    CLEAR_ATTRIB(ans);
                    Rf_unprotect(1);
                    return ans;
                }
                _ => return x,
            }
        }

        let ans = ascommon(call, x, type_);
        // Keep attributes for list/expression/pairlist types
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::LISTSXP
            | SEXPTYPE::LANGSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => {}
            _ => {
                CLEAR_ATTRIB(ans);
            }
        }
        ans
    }
}

/// R-level `typeof()` entry point.
///
/// This is the `do_typeof()` function from coerce.c.
/// Note: canonical version lives in inspect.rs; this is kept as
/// coerce_typeof for internal use.
pub(crate) unsafe fn coerce_typeof(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::OBJSXP.0 && IS_S4_OBJECT(x) == 0 {
            return Rf_mkString(c"object".as_ptr());
        }
        let type_name = match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::NILSXP => "NULL",
            SEXPTYPE::SYMSXP => "symbol",
            SEXPTYPE::LISTSXP => "pairlist",
            SEXPTYPE::CLOSXP => "closure",
            SEXPTYPE::ENVSXP => "environment",
            SEXPTYPE::PROMSXP => "promise",
            SEXPTYPE::LANGSXP => "language",
            SEXPTYPE::SPECIALSXP => "special",
            SEXPTYPE::BUILTINSXP => "builtin",
            SEXPTYPE::CHARSXP => "character",
            SEXPTYPE::LGLSXP => "logical",
            SEXPTYPE::INTSXP => "integer",
            SEXPTYPE::REALSXP => "double",
            SEXPTYPE::CPLXSXP => "complex",
            SEXPTYPE::STRSXP => "character",
            SEXPTYPE::DOTSXP => "...",
            SEXPTYPE::ANYSXP => "any",
            SEXPTYPE::VECSXP => "list",
            SEXPTYPE::EXPRSXP => "expression",
            SEXPTYPE::RAWSXP => "raw",
            SEXPTYPE::OBJSXP => "object",
            _ => "unknown",
        };
        Rf_mkString(std::ffi::CString::new(type_name).expect("CString::new failed: contains null byte").as_ptr())
    }
}

/// R-level `is.*` predicate dispatcher.
///
/// This is the `do_is()` function from coerce.c, implementing is.null,
/// is.logical, is.integer, is.double, is.complex, is.character, etc.
pub unsafe fn do_is(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let ans = Rf_protect(Rf_ScalarLogical(0));
        let pa = LOGICAL(ans);

        match PRIMVAL(op) {
            0 => {
                // is.null
                *pa = isNull(x) as c_int;
            }
            10 => {
                // is.logical
                *pa = (TYPEOF(x) == SEXPTYPE::LGLSXP.0) as c_int;
            }
            13 => {
                // is.integer
                *pa = (TYPEOF(x) == SEXPTYPE::INTSXP.0) as c_int;
            }
            14 => {
                // is.double
                *pa = (TYPEOF(x) == SEXPTYPE::REALSXP.0) as c_int;
            }
            15 => {
                // is.complex
                *pa = (TYPEOF(x) == SEXPTYPE::CPLXSXP.0) as c_int;
            }
            16 => {
                // is.character
                *pa = (TYPEOF(x) == SEXPTYPE::STRSXP.0) as c_int;
            }
            1 => {
                // is.symbol / is.name
                *pa = (TYPEOF(x) == SEXPTYPE::SYMSXP.0) as c_int;
            }
            4 => {
                // is.environment
                *pa = (TYPEOF(x) == SEXPTYPE::ENVSXP.0) as c_int;
            }
            19 => {
                // is.list
                *pa =
                    (TYPEOF(x) == SEXPTYPE::VECSXP.0 || TYPEOF(x) == SEXPTYPE::LISTSXP.0) as c_int;
            }
            2 => {
                // is.pairlist
                *pa =
                    (TYPEOF(x) == SEXPTYPE::LISTSXP.0 || TYPEOF(x) == SEXPTYPE::NILSXP.0) as c_int;
            }
            20 => {
                // is.expression
                *pa = (TYPEOF(x) == SEXPTYPE::EXPRSXP.0) as c_int;
            }
            24 => {
                // is.raw
                *pa = (TYPEOF(x) == SEXPTYPE::RAWSXP.0) as c_int;
            }
            6 => {
                // is.call
                *pa = (TYPEOF(x) == SEXPTYPE::LANGSXP.0) as c_int;
            }
            100 => {
                // is.numeric
                *pa = (isNumeric(x) && !isLogical(x)) as c_int;
            }
            101 => {
                // is.matrix
                *pa = super::helpers::isMatrix(x) as c_int;
            }
            102 => {
                // is.array
                *pa = isArray(x) as c_int;
            }
            300 => {
                // is.language
                *pa = (TYPEOF(x) == SEXPTYPE::SYMSXP.0
                    || TYPEOF(x) == SEXPTYPE::LANGSXP.0
                    || TYPEOF(x) == SEXPTYPE::EXPRSXP.0) as c_int;
            }
            302 => {
                // is.function
                *pa = isFunction(x) as c_int;
            }
            200 => {
                // is.atomic
                let t = TYPEOF(x);
                *pa = (t == SEXPTYPE::CHARSXP.0
                    || t == SEXPTYPE::LGLSXP.0
                    || t == SEXPTYPE::INTSXP.0
                    || t == SEXPTYPE::REALSXP.0
                    || t == SEXPTYPE::CPLXSXP.0
                    || t == SEXPTYPE::STRSXP.0
                    || t == SEXPTYPE::RAWSXP.0) as c_int;
            }
            _ => {
                *pa = 0;
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.vector()` entry point.
///
/// This is the `do_isvector()` function from coerce.c.
pub unsafe fn do_isvector(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mode_arg = CADR(args);
        let ans = Rf_protect(Rf_ScalarLogical(0));
        let pa = LOGICAL(ans);

        if !isString(mode_arg) || LENGTH(mode_arg) != 1 {
            error("invalid 'mode' argument");
        }

        let mode_chars = CHAR(STRING_ELT(mode_arg, 0));
        let mode = if mode_chars.is_null() {
            ""
        } else {
            CStr::from_ptr(mode_chars).to_str().unwrap_or("")
        };

        let is_vec = if mode == "any" {
            isVector(x)
        } else if mode == "numeric" {
            isNumeric(x) && !isLogical(x)
        } else {
            // Check if the type name matches
            let type_name = match SEXPTYPE(TYPEOF(x)) {
                SEXPTYPE::LGLSXP => "logical",
                SEXPTYPE::INTSXP => "integer",
                SEXPTYPE::REALSXP => "double",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::STRSXP => "character",
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::VECSXP => "list",
                SEXPTYPE::EXPRSXP => "expression",
                SEXPTYPE::LISTSXP => "pairlist",
                _ => "",
            };
            mode == type_name || (mode == "name" && type_name == "symbol")
        };

        if is_vec {
            // Check that only a "names" attribute is present
            let mut has_non_name_attr = false;
            let mut a = ATTRIB(x);
            while !isNull(a) {
                if !isNull(TAG(a)) && TAG(a) != R_NamesSymbol() {
                    has_non_name_attr = true;
                    break;
                }
                a = CDR(a);
            }
            *pa = (!has_non_name_attr) as c_int;
        } else {
            *pa = 0;
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.na()` entry point.
///
/// This is the `do_isna()` function from coerce.c.
pub unsafe fn do_isna(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (LOGICAL_ELT(x, i as c_int) == NA_LOGICAL) as c_int;
                }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) == NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = super::ISNAN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (super::ISNAN(v.r) || super::ISNAN(v.i)) as c_int;
                }
            }
            t if t == SEXPTYPE::STRSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (STRING_ELT(x, i) == R_NaString()) as c_int;
                }
            }
            t if t == SEXPTYPE::RAWSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = 0; // no raw NA
                }
            }
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        // Copy dim and names
        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.nan()` entry point.
///
/// This is the `do_isnan()` function from coerce.c.
pub unsafe fn do_isnan(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = super::R_IsNaN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (super::R_IsNaN(v.r) || super::R_IsNaN(v.i)) as c_int;
                }
            }
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.finite()` entry point.
///
/// This is the `do_isfinite()` function from coerce.c.
pub unsafe fn do_isfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP.0 || t == SEXPTYPE::RAWSXP.0 || t == SEXPTYPE::NILSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) != NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    *pa.add(i as usize) = super::R_FINITE(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (super::R_FINITE(v.r) && super::R_FINITE(v.i)) as c_int;
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R-level `is.infinite()` entry point.
///
/// This is the `do_isinfinite()` function from coerce.c.
pub unsafe fn do_isinfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP.0
                || t == SEXPTYPE::RAWSXP.0
                || t == SEXPTYPE::NILSXP.0
                || t == SEXPTYPE::LGLSXP.0
                || t == SEXPTYPE::INTSXP.0 =>
            {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                for i in 0..n {
                    let xr = REAL_ELT(x, i as c_int);
                    *pa.add(i as usize) = if super::ISNAN(xr) || super::R_FINITE(xr) {
                        0
                    } else {
                        1
                    };
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = if (super::ISNAN(v.r) || super::R_FINITE(v.r))
                        && (super::ISNAN(v.i) || super::R_FINITE(v.i))
                    {
                        0
                    } else {
                        1
                    };
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_coerce -- R-level coercion entry point
// ---------------------------------------------------------------------------

/// R-level coercion entry point (`do_coerce`).
///
/// This dispatches to `ascommon` for the actual coercion, matching R's
/// behavior for `as.vector()`, `as.expression()`, `as.list()`, etc.
pub unsafe fn do_coerce(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if args.is_null() || isNull(CDR(args)) {
            return x;
        }
        let mode_str = CADR(args);
        if !isString(mode_str) || LENGTH(mode_str) != 1 {
            error("invalid 'mode' argument");
        }

        let mode_chars = CHAR(STRING_ELT(mode_str, 0));
        if mode_chars.is_null() {
            error("invalid 'mode' argument");
        }
        let mode = CStr::from_ptr(mode_chars).to_str().unwrap_or("");

        let type_: c_int = match mode {
            "logical" => SEXPTYPE::LGLSXP.0,
            "integer" => SEXPTYPE::INTSXP.0,
            "double" | "numeric" => SEXPTYPE::REALSXP.0,
            "complex" => SEXPTYPE::CPLXSXP.0,
            "character" => SEXPTYPE::STRSXP.0,
            "raw" => SEXPTYPE::RAWSXP.0,
            "list" => SEXPTYPE::VECSXP.0,
            "expression" => SEXPTYPE::EXPRSXP.0,
            "pairlist" => SEXPTYPE::LISTSXP.0,
            "any" => return x,
            "symbol" | "name" => SEXPTYPE::SYMSXP.0,
            _ => {
                error("invalid 'mode' argument");
                0 // unreachable
            }
        };

        if TYPEOF(x) == type_ {
            // Same type: strip attributes for atomic types
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    if isNull(ATTRIB(x)) {
                        return x;
                    }
                    let ans = Rf_protect(Rf_allocVector3(type_, xlength(x)));
                    // Copy data
                    let src = DATAPTR(x);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x) as usize * elem_size,
                        );
                    }
                    Rf_unprotect(1);
                    return ans;
                }
                _ => return x,
            }
        }

        let ans = ascommon(call, x, type_);
        // Clear attributes for atomic types (matching R's behavior)
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP => {
                CLEAR_ATTRIB(ans);
            }
            _ => {}
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_ascoerce -- primitive as.character/as.integer/... dispatcher
// ---------------------------------------------------------------------------

/// Dispatcher for the primitive `as.character`, `as.integer`, `as.double`,
/// `as.numeric`, `as.complex`, `as.logical`, `as.raw` builtins.
///
/// Uses the FunTab offset field to determine the target SEXPTYPE:
///   0 -> STRSXP, 1 -> INTSXP, 2 -> REALSXP, 3 -> CPLXSXP,
///   4 -> LGLSXP, 5 -> RAWSXP
pub unsafe fn do_ascoerce(call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _ = call;
        if args.is_null() {
            return R_NilValue();
        }
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }

        let offset = PRIMVAL(op) as usize;
        let target_type: c_int = match offset {
            0 => SEXPTYPE::STRSXP.0,  // as.character
            1 => SEXPTYPE::INTSXP.0,  // as.integer
            2 => SEXPTYPE::REALSXP.0, // as.double / as.numeric
            3 => SEXPTYPE::CPLXSXP.0, // as.complex
            4 => SEXPTYPE::LGLSXP.0,  // as.logical
            5 => SEXPTYPE::RAWSXP.0,  // as.raw
            _ => return x,
        };

        if TYPEOF(x) == target_type {
            // Same type: strip attributes for atomic types
            match SEXPTYPE(target_type) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    if isNull(ATTRIB(x)) {
                        return x;
                    }
                    let ans = Rf_protect(Rf_allocVector3(target_type, xlength(x)));
                    let src = DATAPTR(x);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(target_type) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x) as usize * elem_size,
                        );
                    }
                    Rf_unprotect(1);
                    return ans;
                }
                _ => return x,
            }
        }

        let ans = coerceVector(x, target_type);
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP => {
                CLEAR_ATTRIB(ans);
            }
            _ => {}
        }
        ans
    }
}
