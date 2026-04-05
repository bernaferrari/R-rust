#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Vector coercion functions: coerceVector, coerceToLogical, coerceToInteger, etc.
//! Also scalar accessors: asLogical, asLogical2, asInteger, asReal, asComplex, asRaw.

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::main::subset::installTrChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::NA_REAL;
use super::helpers::{
    CoercionWarning, R_NaString, SHALLOW_DUPLICATE_ATTRIB, error, errorcall, isFunction,
    isLanguage, isNull, isString, isVector, isVectorAtomic, isVectorizable, xlength,
};
use super::scalar::*;

// ---------------------------------------------------------------------------
// Vector coercion functions
// ---------------------------------------------------------------------------

/// Coerce a vector to logical type.
pub(crate) unsafe fn coerceToLogical(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = LOGICAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::INTSXP.0 => LogicalFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => LogicalFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => LogicalFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => LogicalFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    LogicalFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to integer type.
pub(crate) unsafe fn coerceToInteger(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = INTEGER(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => IntegerFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => IntegerFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => RAW_ELT(v, ii) as c_int,
                _ => NA_INTEGER,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to real (double) type.
pub(crate) unsafe fn coerceToReal(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = REAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => RealFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => RealFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => RealFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => RealFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => RealFromInteger(RAW_ELT(v, ii) as c_int, &mut warn),
                _ => NA_REAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to complex type.
pub(crate) unsafe fn coerceToComplex(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = COMPLEX(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => ComplexFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => ComplexFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => ComplexFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => ComplexFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    ComplexFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                },
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to raw type.
pub(crate) unsafe fn coerceToRaw(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::RAWSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = RAW(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let tmp: c_int = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    let val = IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER {
                        warn |= super::helpers::WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    let val = INTEGER_ELT(v, ii);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= super::helpers::WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    let val = IntegerFromReal(REAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= super::helpers::WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    let val = IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= super::helpers::WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    let val = IntegerFromString(STRING_ELT(v, i), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= super::helpers::WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                _ => 0,
            };
            *pa.add(i as usize) = tmp as Rbyte;
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to string (character) type.
pub(crate) unsafe fn coerceToString(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
        SHALLOW_DUPLICATE_ATTRIB(ans, v);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let s = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => StringFromLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => StringFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => StringFromReal_impl(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => StringFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => StringFromRaw(RAW_ELT(v, ii), &mut warn),
                _ => R_NaString(),
            };
            SET_STRING_ELT(ans, i, s);
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to expression type.
pub(crate) unsafe fn coerceToExpression(v: SEXP) -> SEXP {
    unsafe {
        if !isVectorAtomic(v) {
            let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, 1));
            SET_VECTOR_ELT(ans, 0, v);
            Rf_unprotect(1);
            return ans;
        }

        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP.0 => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP.0 => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP.0 => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP.0 => Rf_ScalarRaw(RAW_ELT(v, ii)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to generic vector (list) type.
pub(crate) unsafe fn coerceToVectorList(v: SEXP) -> SEXP {
    unsafe {
        let n = xlength(v);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, n));

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP.0 => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP.0 => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP.0 => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP.0 => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP.0 => Rf_ScalarRaw(RAW_ELT(v, ii)),
                t if t == SEXPTYPE::LISTSXP.0 || t == SEXPTYPE::LANGSXP.0 => CAR(v.add(i as usize)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a vector to pairlist type.
pub(crate) unsafe fn coerceToPairList(v: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(v);
        let ans = Rf_protect(Rf_allocList(n));
        let mut ansp = ans;

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            match vtype {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::LGLSXP.0, 1);
                    *LOGICAL(elt) = LOGICAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
                    *INTEGER(elt) = INTEGER_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
                    *REAL(elt) = REAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 1);
                    *COMPLEX(elt) = COMPLEX_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    SETCAR(ansp, Rf_ScalarString(STRING_ELT(v, i as R_xlen_t)));
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    let elt = Rf_allocVector3(SEXPTYPE::RAWSXP.0, 1);
                    *RAW(elt) = RAW_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => {
                    SETCAR(ansp, VECTOR_ELT(v, i as R_xlen_t));
                }
                _ => {}
            }
            ansp = CDR(ansp);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        Rf_unprotect(1);
        ans
    }
}

/// Coerce a pairlist (LISTSXP/LANGSXP) to the given type.
pub(crate) unsafe fn coercePairList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::EXPRSXP.0, 1));
            SET_VECTOR_ELT(rval, 0, v);
            Rf_unprotect(1);
            return rval;
        }

        if type_ == SEXPTYPE::STRSXP {
            let n = LENGTH(v);
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
            let mut vp = v;
            for i in 0..n {
                let car = CAR(vp);
                if isString(car) && LENGTH(car) == 1 {
                    SET_STRING_ELT(rval, i as R_xlen_t, STRING_ELT(car, 0));
                } else {
                    // deparse not available; use StringFromLogical as fallback
                    SET_STRING_ELT(rval, i as R_xlen_t, StringFromLogical(0));
                }
                vp = CDR(vp);
            }
            Rf_unprotect(1);
            return rval;
        }

        if type_ == SEXPTYPE::VECSXP {
            // PairToVectorList
            let mut len: c_int = 0;
            let mut xptr = v;
            while !xptr.is_null() && !isNull(xptr) {
                len += 1;
                xptr = CDR(xptr);
            }
            let xnew = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, len as R_xlen_t));
            let mut xptr = v;
            for i in 0..len {
                SET_VECTOR_ELT(xnew, i as R_xlen_t, CAR(xptr));
                xptr = CDR(xptr);
            }
            Rf_unprotect(1);
            return xnew;
        }

        if isVectorizable(v) {
            let n = LENGTH(v);
            let rval = Rf_protect(Rf_allocVector3(type_.0, n as R_xlen_t));
            let mut vp = v;
            for i in 0..n {
                match type_.0 {
                    t if t == SEXPTYPE::LGLSXP.0 => {
                        *LOGICAL(rval).add(i as usize) = asLogical(CAR(vp));
                    }
                    t if t == SEXPTYPE::INTSXP.0 => {
                        *INTEGER(rval).add(i as usize) = asInteger(CAR(vp));
                    }
                    t if t == SEXPTYPE::REALSXP.0 => {
                        *REAL(rval).add(i as usize) = asReal(CAR(vp));
                    }
                    t if t == SEXPTYPE::CPLXSXP.0 => {
                        *COMPLEX(rval).add(i as usize) = asComplex(CAR(vp));
                    }
                    t if t == SEXPTYPE::RAWSXP.0 => {
                        *RAW(rval).add(i as usize) = asInteger(CAR(vp)) as Rbyte;
                    }
                    _ => {}
                }
                vp = CDR(vp);
            }
            Rf_unprotect(1);
            return rval;
        }

        error("cannot coerce type to vector");
        ptr::null_mut() // unreachable
    }
}

/// Coerce a vector list (VECSXP/EXPRSXP) to the given type.
pub(crate) unsafe fn coerceVectorList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;

        // expression -> list: just change the type tag
        if type_ == SEXPTYPE::VECSXP && TYPEOF(v) == SEXPTYPE::EXPRSXP.0 {
            let rval = Rf_allocVector3(SEXPTYPE::VECSXP.0, xlength(v));
            // Copy the data pointers
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> expression: just change the type tag
        if type_ == SEXPTYPE::EXPRSXP && TYPEOF(v) == SEXPTYPE::VECSXP.0 {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP.0, xlength(v));
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> pairlist
        if type_ == SEXPTYPE::LISTSXP {
            // VectorToPairList
            let n = LENGTH(v);
            let x = Rf_protect(Rf_allocList(n));
            let names = Rf_protect(getAttrib(v, R_NamesSymbol()));
            let mut xptr = x;
            for i in 0..n {
                SETCAR(xptr, VECTOR_ELT(v, i as R_xlen_t));
                xptr = CDR(xptr);
            }
            if !isNull(names) {
                let mut xptr2 = x;
                for i in 0..n {
                    let name_elt = STRING_ELT(names, i as R_xlen_t);
                    if !isNull(name_elt) {
                        let pname = CHAR(name_elt);
                        if !pname.is_null() && *pname != 0 {
                            SETTAG(xptr2, Rf_install(pname));
                        }
                    }
                    xptr2 = CDR(xptr2);
                }
            }
            Rf_unprotect(2);
            return x;
        }

        // list -> string
        if type_ == SEXPTYPE::STRSXP {
            let n = xlength(v);
            let rval = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
            for i in 0..n {
                let elt = VECTOR_ELT(v, i);
                if isString(elt) && LENGTH(elt) == 1 {
                    SET_STRING_ELT(rval, i, STRING_ELT(elt, 0));
                } else {
                    // deparse not available; convert via asCharacterFactor-like path
                    SET_STRING_ELT(rval, i, StringFromLogical(0));
                }
            }
            Rf_unprotect(1);
            return rval;
        }

        if isVectorizable(v) {
            let n = xlength(v);
            let rval = Rf_protect(Rf_allocVector3(type_.0, n));
            match type_.0 {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    for i in 0..n {
                        *LOGICAL(rval).add(i as usize) = asLogical(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    for i in 0..n {
                        *INTEGER(rval).add(i as usize) = asInteger(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    for i in 0..n {
                        *REAL(rval).add(i as usize) = asReal(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    for i in 0..n {
                        *COMPLEX(rval).add(i as usize) = asComplex(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    for i in 0..n {
                        let tmp = asInteger(VECTOR_ELT(v, i));
                        if tmp < 0 || tmp > 255 {
                            warn |= super::helpers::WARN_RAW;
                        }
                        *RAW(rval).add(i as usize) = if tmp < 0 || tmp > 255 {
                            0
                        } else {
                            tmp as Rbyte
                        };
                    }
                }
                _ => {}
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            let names = getAttrib(v, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(rval, R_NamesSymbol(), names);
            }
            Rf_unprotect(1);
            return rval;
        }

        error("list object cannot be coerced to type");
        ptr::null_mut() // unreachable
    }
}

/// Coerce to a symbol.
pub(crate) unsafe fn coerceToSymbol(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        if LENGTH(v) <= 0 {
            error("invalid data of mode (too short)");
        }

        let ans = Rf_protect(match TYPEOF(v) {
            t if t == SEXPTYPE::LGLSXP.0 => StringFromLogical(LOGICAL_ELT(v, 0)),
            t if t == SEXPTYPE::INTSXP.0 => StringFromInteger(INTEGER_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::REALSXP.0 => StringFromReal_impl(REAL_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::CPLXSXP.0 => StringFromComplex(COMPLEX_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::STRSXP.0 => STRING_ELT(v, 0),
            t if t == SEXPTYPE::RAWSXP.0 => StringFromRaw(RAW_ELT(v, 0), &mut warn),
            _ => R_NilValue(),
        });

        if warn != 0 {
            CoercionWarning(warn);
        }

        let sym = Rf_install(CHAR(ans));
        Rf_unprotect(1);
        sym
    }
}

/// Coerce a symbol (SYMSXP) to the given type.
/// This matches R's coerceSymbol() from coerce.c.
pub(crate) unsafe fn coerceSymbol(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut rval = R_NilValue();
        if type_ == SEXPTYPE::EXPRSXP {
            rval = Rf_protect(Rf_allocVector3(type_.0, 1));
            SET_VECTOR_ELT(rval, 0, v);
            Rf_unprotect(1);
        } else if type_ == SEXPTYPE::CHARSXP {
            rval = PRINTNAME(v);
        } else if type_ == SEXPTYPE::STRSXP {
            rval = Rf_ScalarString(PRINTNAME(v));
        }
        // else: warning, return R_NilValue
        rval
    }
}

/// Create a tag (symbol) from an SEXP.
/// If x is already a symbol or NULL, return it. If x is a string of length >= 1,
/// install it as a symbol.
pub(crate) unsafe fn CreateTag(x: SEXP) -> SEXP {
    unsafe {
        use super::helpers::isSymbol;
        if isNull(x) || isSymbol(x) {
            return x;
        }
        if isString(x) && LENGTH(x) >= 1 {
            let s = STRING_ELT(x, 0);
            if !isNull(s) {
                let cs = CHAR(s);
                if !cs.is_null() && *cs != 0 {
                    return installTrChar(s);
                }
            }
        }
        // fallback: return NULL
        R_NilValue()
    }
}

/// Convert an SEXP to a function (closure).
/// This matches R's asFunction() from coerce.c.
pub(crate) unsafe fn asFunction(x: SEXP) -> SEXP {
    unsafe {
        if isFunction(x) {
            return x;
        }
        let f = Rf_protect(allocSExp(SEXPTYPE::CLOSXP));
        SET_CLOENV(f, R_GlobalEnv());
        // For simplicity, create a closure with empty formals and body = x
        SET_FORMALS(f, R_NilValue());
        SET_BODY(f, x);
        Rf_unprotect(1);
        f
    }
}

/// Common coercion helper for as.vector / as.XXX dispatch.
/// This matches R's ascommon() from coerce.c.
pub(crate) unsafe fn ascommon(call: SEXP, u: SEXP, type_: c_int) -> SEXP {
    unsafe {
        use super::helpers::{isList, isSymbol};

        let target_type = SEXPTYPE(type_);

        if target_type == SEXPTYPE::CLOSXP {
            return asFunction(u);
        }

        if isVector(u)
            || isList(u)
            || isLanguage(u)
            || (isSymbol(u) && target_type == SEXPTYPE::EXPRSXP)
        {
            let v = if type_ != SEXPTYPE::ANYSXP.0 && TYPEOF(u) != type_ {
                coerceVector(u, type_)
            } else {
                u
            };

            // Drop attributes for certain types (as.pairlist behavior)
            if target_type == SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::LANGSXP.0
                && TYPEOF(u) != SEXPTYPE::LISTSXP.0
                && TYPEOF(u) != SEXPTYPE::EXPRSXP.0
                && TYPEOF(u) != SEXPTYPE::VECSXP.0
            {
                // Clear attributes
                let attr = ATTRIB(v);
                if !isNull(attr) {
                    SET_ATTRIB(v, R_NilValue());
                }
            }
            return v;
        }

        if isSymbol(u) && target_type == SEXPTYPE::STRSXP {
            return Rf_ScalarString(PRINTNAME(u));
        }
        if isSymbol(u) && target_type == SEXPTYPE::SYMSXP {
            return u;
        }
        if isSymbol(u) && target_type == SEXPTYPE::VECSXP {
            let v = Rf_allocVector3(SEXPTYPE::VECSXP.0, 1);
            SET_VECTOR_ELT(v, 0, u);
            return v;
        }

        errorcall(call, "cannot coerce type to vector of type");
        u // unreachable
    }
}

// ---------------------------------------------------------------------------
// coerceVector -- main coercion dispatcher
// ---------------------------------------------------------------------------

/// Coerce a vector from one type to another.
///
/// This is the main entry point for type coercion in R, equivalent to
/// R's `coerceVector()` from coerce.c. It dispatches to the appropriate
/// type-specific coercion function based on the source and target types.
pub unsafe fn coerceVector(v: SEXP, type_: c_int) -> SEXP {
    unsafe {
        if v.is_null() {
            return ptr::null_mut();
        }
        let target = SEXPTYPE(type_);

        // If already the right type, return as-is
        if TYPEOF(v) == type_ {
            return v;
        }

        let _v = Rf_protect(v);

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::SYMSXP.0 => coerceSymbol(v, target),
            t if t == SEXPTYPE::NILSXP.0 || t == SEXPTYPE::LISTSXP.0 => {
                if type_ == SEXPTYPE::LISTSXP.0 {
                    v // already pairlist
                } else {
                    coercePairList(v, target)
                }
            }
            t if t == SEXPTYPE::LANGSXP.0 => {
                if type_ != SEXPTYPE::STRSXP.0 {
                    coercePairList(v, target)
                } else {
                    // LANGSXP -> STRSXP: special handling for operator names
                    let n = LENGTH(v);
                    let ans = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
                    let mut vp = v;
                    for i in 0..n as R_xlen_t {
                        let car = CAR(vp);
                        if isString(car) && LENGTH(car) == 1 {
                            SET_STRING_ELT(ans, i, STRING_ELT(car, 0));
                        } else if super::helpers::isSymbol(car) {
                            SET_STRING_ELT(ans, i, PRINTNAME(car));
                        } else {
                            SET_STRING_ELT(ans, i, StringFromLogical(0));
                        }
                        vp = CDR(vp);
                    }
                    ans
                }
            }
            t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => coerceVectorList(v, target),
            t if t == SEXPTYPE::ENVSXP.0 => {
                error("environments cannot be coerced to other types");
                ptr::null_mut() // unreachable
            }
            // Atomic vector types
            t if t == SEXPTYPE::LGLSXP.0
                || t == SEXPTYPE::INTSXP.0
                || t == SEXPTYPE::REALSXP.0
                || t == SEXPTYPE::CPLXSXP.0
                || t == SEXPTYPE::STRSXP.0
                || t == SEXPTYPE::RAWSXP.0 =>
            {
                match type_ {
                    t if t == SEXPTYPE::SYMSXP.0 => coerceToSymbol(v),
                    t if t == SEXPTYPE::LGLSXP.0 => coerceToLogical(v),
                    t if t == SEXPTYPE::INTSXP.0 => coerceToInteger(v),
                    t if t == SEXPTYPE::REALSXP.0 => coerceToReal(v),
                    t if t == SEXPTYPE::CPLXSXP.0 => coerceToComplex(v),
                    t if t == SEXPTYPE::RAWSXP.0 => coerceToRaw(v),
                    t if t == SEXPTYPE::STRSXP.0 => coerceToString(v),
                    t if t == SEXPTYPE::EXPRSXP.0 => coerceToExpression(v),
                    t if t == SEXPTYPE::VECSXP.0 => coerceToVectorList(v),
                    t if t == SEXPTYPE::LISTSXP.0 => coerceToPairList(v),
                    _ => {
                        error("cannot coerce type to vector of type");
                        ptr::null_mut() // unreachable
                    }
                }
            }
            _ => {
                error("cannot coerce type to vector of type");
                ptr::null_mut() // unreachable
            }
        };

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// asLogical -- coerce first element to logical
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a logical value.
///
/// This is R's `asLogical()` from coerce.c. Returns NA_LOGICAL for
/// empty vectors, and dispatches based on the vector's type.
pub unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { asLogical2(x, 0, R_NilValue()) }
}

/// Convert the first element of a vector to a logical value, with length checking.
///
/// This is R's `asLogical2()` from coerce.c.
pub unsafe fn asLogical2(x: SEXP, checking: c_int, _call: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) {
            if xlength(x) < 1 {
                return NA_LOGICAL;
            }
            if checking != 0 && xlength(x) > 1 {
                // In R this calls errorcall; we just proceed
            }
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => LOGICAL_ELT(x, 0),
                t if t == SEXPTYPE::INTSXP.0 => LogicalFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => LogicalFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => LogicalFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => LogicalFromString(STRING_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::RAWSXP.0 => {
                    LogicalFromInteger(RAW_ELT(x, 0) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            }
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            LogicalFromString(x, &mut warn)
        } else {
            NA_LOGICAL
        }
    }
}

// ---------------------------------------------------------------------------
// asInteger -- coerce first element to integer
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to an integer value.
///
/// This is R's `asInteger()` from coerce.c.
pub unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::RAWSXP.0 => RAW_ELT(x, 0) as c_int,
                t if t == SEXPTYPE::LGLSXP.0 => IntegerFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => INTEGER_ELT(x, 0),
                t if t == SEXPTYPE::REALSXP.0 => IntegerFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP.0 => IntegerFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => IntegerFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_INTEGER,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            let res = IntegerFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// asReal -- coerce first element to real (double)
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a real (double) value.
///
/// This is R's `asReal()` from coerce.c.
pub unsafe fn asReal(x: SEXP) -> c_double {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => RealFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP.0 => RealFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP.0 => REAL_ELT(x, 0),
                t if t == SEXPTYPE::CPLXSXP.0 => RealFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP.0 => RealFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_REAL,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            let res = RealFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_REAL
    }
}

// ---------------------------------------------------------------------------
// asComplex -- coerce first element to complex
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a complex value.
///
/// This is R's `asComplex()` from coerce.c.
pub unsafe fn asComplex(x: SEXP) -> Rcomplex {
    unsafe {
        let mut warn: c_int = 0;
        let mut z = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };

        if isVectorAtomic(x) && xlength(x) >= 1 {
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    z = ComplexFromLogical(LOGICAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    z = ComplexFromInteger(INTEGER_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    z = ComplexFromReal(REAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    z = COMPLEX_ELT(x, 0);
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    z = ComplexFromString(STRING_ELT(x, 0), &mut warn);
                }
                _ => {}
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP.0 {
            z = ComplexFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        }

        z
    }
}

// ---------------------------------------------------------------------------
// asRaw -- coerce first element to raw byte
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a raw byte value.
///
/// This follows the same pattern as asInteger/asReal, returning 0 for
/// out-of-range or NA values.
pub unsafe fn asRaw(x: SEXP) -> Rbyte {
    unsafe {
        if isVectorAtomic(x) && xlength(x) >= 1 {
            let val = asInteger(x);
            if val == NA_INTEGER || val < 0 || val > 255 {
                return 0;
            }
            return val as Rbyte;
        }
        0
    }
}
