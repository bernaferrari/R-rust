#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! complete.cases() implementation
//! Port of r-source/src/library/stats/src/complete_cases.c

use std::os::raw::c_int;

use crate::attrib_core::{R_RowNamesSymbol, getAttrib};
use crate::main::errors::Rf_error;
use crate::main::relop::NA_STRING;
use crate::main::relop::R_DimSymbol;
use crate::main::relop::R_NamesSymbol;
use crate::sexp::accessors::{
    CAR, CDR, COMPLEX, INTEGER, LENGTH, REAL, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_isList;
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

unsafe fn isMatrix(x: SEXP) -> bool {
    let u = getAttrib(x, R_DimSymbol());
    !R_NilValue().is_null() && !u.is_null() && LENGTH(u) >= 2
}

unsafe fn isVector(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
}

#[unsafe(no_mangle)]
unsafe fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP
}

unsafe fn length_sexp(x: SEXP) -> c_int {
    let t = TYPEOF(x);
    if t == SEXPTYPE::NILSXP {
        return 0;
    }
    if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
        let mut count = 0i32;
        let mut cur = x;
        while !cur.is_null() {
            count += 1;
            cur = CDR(cur);
        }
        return count;
    }
    LENGTH(x)
}

unsafe fn check_vector_na(u: SEXP, rval: SEXP, len: c_int) {
    let n = LENGTH(u);
    let rval_int = INTEGER(rval);
    let t = TYPEOF(u);
    let mut i: c_int = 0;
    while i < n {
        match t {
            x if x == SEXPTYPE::INTSXP || x == SEXPTYPE::LGLSXP => {
                if *INTEGER(u).add(i as usize) == NA_INTEGER {
                    *rval_int.add((i % len) as usize) = 0;
                }
            }
            x if x == SEXPTYPE::REALSXP => {
                if (*REAL(u).add(i as usize)).is_nan() {
                    *rval_int.add((i % len) as usize) = 0;
                }
            }
            x if x == SEXPTYPE::CPLXSXP => {
                let c = *COMPLEX(u).add(i as usize);
                if c.r.is_nan() || c.i.is_nan() {
                    *rval_int.add((i % len) as usize) = 0;
                }
            }
            x if x == SEXPTYPE::STRSXP => {
                if STRING_ELT(u, i as i64) == NA_STRING() {
                    *rval_int.add((i % len) as usize) = 0;
                }
            }
            _ => {
                Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
            }
        }
        i += 1;
    }
}

pub unsafe fn compcases(args: SEXP) -> SEXP {
    let mut s: SEXP;
    let mut t: SEXP;
    let mut u: SEXP;
    let mut len: c_int = -1;

    let mut args_iter = CDR(args);

    // First pass: determine length
    s = args_iter;
    while !s.is_null() && s != R_NilValue() {
        let car = CAR(s);
        if Rf_isList(car) != 0 {
            t = car;
            while !t.is_null() && t != R_NilValue() {
                u = CAR(t);
                if isMatrix(u) {
                    let dim = getAttrib(u, R_DimSymbol());
                    if len < 0 {
                        len = *INTEGER(dim);
                    } else if len != *INTEGER(dim) {
                        Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                        return R_NilValue();
                    }
                } else if isVector(u) {
                    if len < 0 {
                        len = LENGTH(u);
                    } else if len != LENGTH(u) {
                        Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                        return R_NilValue();
                    }
                } else {
                    Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
                    return R_NilValue();
                }
                t = CDR(t);
            }
        } else if isNewList(car) {
            t = car;
            let nt = length_sexp(t);
            if nt > 0 {
                let mut it: c_int = 0;
                while it < nt {
                    u = VECTOR_ELT(t, it as i64);
                    if isMatrix(u) {
                        let dim = getAttrib(u, R_DimSymbol());
                        if len < 0 {
                            len = *INTEGER(dim);
                        } else if len != *INTEGER(dim) {
                            Rf_error(
                                b"not all arguments have the same length\0".as_ptr() as *const _
                            );
                            return R_NilValue();
                        }
                    } else if isVector(u) {
                        if len < 0 {
                            len = LENGTH(u);
                        } else if len != LENGTH(u) {
                            Rf_error(
                                b"not all arguments have the same length\0".as_ptr() as *const _
                            );
                            return R_NilValue();
                        }
                    } else {
                        Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
                        return R_NilValue();
                    }
                    it += 1;
                }
            } else {
                u = getAttrib(t, R_RowNamesSymbol());
                if !u.is_null() && u != R_NilValue() {
                    if len < 0 {
                        len = LENGTH(u);
                    } else if len != *INTEGER(u) {
                        Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                        return R_NilValue();
                    }
                }
            }
        } else if isMatrix(car) {
            let dim = getAttrib(car, R_DimSymbol());
            if len < 0 {
                len = *INTEGER(dim);
            } else if len != *INTEGER(dim) {
                Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                return R_NilValue();
            }
        } else if isVector(car) {
            if len < 0 {
                len = LENGTH(car);
            } else if len != LENGTH(car) {
                Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                return R_NilValue();
            }
        } else {
            Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
            return R_NilValue();
        }
        s = CDR(s);
    }

    if len < 0 {
        Rf_error(b"no input has determined the number of cases\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let rval = Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP, len));
    let rval_int = INTEGER(rval);
    let mut i: c_int = 0;
    while i < len {
        *rval_int.add(i as usize) = 1;
        i += 1;
    }

    // Second pass: check for NAs
    s = args_iter;
    while !s.is_null() && s != R_NilValue() {
        let car = CAR(s);
        if Rf_isList(car) != 0 {
            t = car;
            while !t.is_null() && t != R_NilValue() {
                u = CAR(t);
                check_vector_na(u, rval, len);
                t = CDR(t);
            }
        } else if isNewList(car) {
            t = car;
            let nt = length_sexp(t);
            let mut it: c_int = 0;
            while it < nt {
                u = VECTOR_ELT(t, it as i64);
                check_vector_na(u, rval, len);
                it += 1;
            }
        } else {
            check_vector_na(car, rval, len);
        }
        s = CDR(s);
    }

    Rf_unprotect(1);
    rval
}
