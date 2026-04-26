//! complete.cases() implementation
//! Port of r-source/src/library/stats/src/complete_cases.c

use std::os::raw::c_int;
use std::slice;

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
use crate::sexp::protect::protect as protect_sexp;

fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let u = getAttrib(x, R_DimSymbol());
        !u.is_null() && LENGTH(u) >= 2
    }
}

fn isVector(x: SEXP) -> bool {
    unsafe {
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
}

fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::VECSXP }
}

fn length_sexp(x: SEXP) -> c_int {
    unsafe {
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
}

fn check_vector_na(u: SEXP, rval: &mut [c_int], len: c_int) {
    let len = len as usize;
    unsafe {
        let n = LENGTH(u) as usize;
        let t = TYPEOF(u);
        match t {
            x if x == SEXPTYPE::INTSXP || x == SEXPTYPE::LGLSXP => {
                let values = slice::from_raw_parts(INTEGER(u), n);
                for (i, &value) in values.iter().enumerate() {
                    if value == NA_INTEGER {
                        rval[i % len] = 0;
                    }
                }
            }
            x if x == SEXPTYPE::REALSXP => {
                let values = slice::from_raw_parts(REAL(u), n);
                for (i, &value) in values.iter().enumerate() {
                    if value.is_nan() {
                        rval[i % len] = 0;
                    }
                }
            }
            x if x == SEXPTYPE::CPLXSXP => {
                let values = slice::from_raw_parts(COMPLEX(u), n);
                for (i, value) in values.iter().enumerate() {
                    if value.r.is_nan() || value.i.is_nan() {
                        rval[i % len] = 0;
                    }
                }
            }
            x if x == SEXPTYPE::STRSXP => {
                for i in 0..n {
                    if STRING_ELT(u, i as i64) == NA_STRING() {
                        rval[i % len] = 0;
                    }
                }
            }
            _ => {
                Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
            }
        }
    }
}

pub unsafe fn compcases(args: SEXP) -> SEXP {
    let mut len: c_int = -1;
    let nil = unsafe { R_NilValue() };

    let mut args_iter = unsafe { CDR(args) };

    // First pass: determine length
    let mut s = args_iter;
    while !s.is_null() && s != nil {
        let car = unsafe { CAR(s) };
        if unsafe { Rf_isList(car) } != 0 {
            let mut t = car;
            while !t.is_null() && t != nil {
                let u = unsafe { CAR(t) };
                if isMatrix(u) {
                    let dim = unsafe { getAttrib(u, R_DimSymbol()) };
                    if len < 0 {
                        len = unsafe { *INTEGER(dim) };
                    } else if len != unsafe { *INTEGER(dim) } {
                        unsafe {
                            Rf_error(
                                b"not all arguments have the same length\0".as_ptr() as *const _
                            );
                        }
                        return nil;
                    }
                } else if isVector(u) {
                    if len < 0 {
                        len = unsafe { LENGTH(u) };
                    } else if len != unsafe { LENGTH(u) } {
                        unsafe {
                            Rf_error(
                                b"not all arguments have the same length\0".as_ptr() as *const _
                            );
                        }
                        return nil;
                    }
                } else {
                    unsafe {
                        Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
                    }
                    return nil;
                }
                t = unsafe { CDR(t) };
            }
        } else if isNewList(car) {
            let mut t = car;
            let nt = length_sexp(t);
            if nt > 0 {
                let mut it: c_int = 0;
                while it < nt {
                    let u = unsafe { VECTOR_ELT(t, it as i64) };
                    if isMatrix(u) {
                        let dim = unsafe { getAttrib(u, R_DimSymbol()) };
                        if len < 0 {
                            len = unsafe { *INTEGER(dim) };
                        } else if len != unsafe { *INTEGER(dim) } {
                            unsafe {
                                Rf_error(b"not all arguments have the same length\0".as_ptr()
                                    as *const _);
                            }
                            return nil;
                        }
                    } else if isVector(u) {
                        if len < 0 {
                            len = unsafe { LENGTH(u) };
                        } else if len != unsafe { LENGTH(u) } {
                            unsafe {
                                Rf_error(b"not all arguments have the same length\0".as_ptr()
                                    as *const _);
                            }
                            return nil;
                        }
                    } else {
                        unsafe {
                            Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
                        }
                        return nil;
                    }
                    it += 1;
                }
            } else {
                let u = unsafe { getAttrib(t, R_RowNamesSymbol()) };
                if !u.is_null() && u != nil {
                    if len < 0 {
                        len = unsafe { LENGTH(u) };
                    } else if len != unsafe { *INTEGER(u) } {
                        unsafe {
                            Rf_error(
                                b"not all arguments have the same length\0".as_ptr() as *const _
                            );
                        }
                        return nil;
                    }
                }
            }
        } else if isMatrix(car) {
            let dim = unsafe { getAttrib(car, R_DimSymbol()) };
            if len < 0 {
                len = unsafe { *INTEGER(dim) };
            } else if len != unsafe { *INTEGER(dim) } {
                unsafe {
                    Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                }
                return nil;
            }
        } else if isVector(car) {
            if len < 0 {
                len = unsafe { LENGTH(car) };
            } else if len != unsafe { LENGTH(car) } {
                unsafe {
                    Rf_error(b"not all arguments have the same length\0".as_ptr() as *const _);
                }
                return nil;
            }
        } else {
            unsafe {
                Rf_error(b"invalid 'type' of argument\0".as_ptr() as *const _);
            }
            return nil;
        }
        s = unsafe { CDR(s) };
    }

    if len < 0 {
        unsafe {
            Rf_error(b"no input has determined the number of cases\0".as_ptr() as *const _);
        }
        return nil;
    }

    let rval = unsafe { Rf_allocVector(SEXPTYPE::LGLSXP, len) };
    let _rval_guard = protect_sexp(rval);
    let rval_int = unsafe { slice::from_raw_parts_mut(INTEGER(rval), len as usize) };
    rval_int.fill(1);

    // Second pass: check for NAs
    s = args_iter;
    while !s.is_null() && s != nil {
        let car = unsafe { CAR(s) };
        if unsafe { Rf_isList(car) } != 0 {
            let mut t = car;
            while !t.is_null() && t != nil {
                let u = unsafe { CAR(t) };
                check_vector_na(u, rval_int, len);
                t = unsafe { CDR(t) };
            }
        } else if isNewList(car) {
            let t = car;
            let nt = length_sexp(t);
            let mut it: c_int = 0;
            while it < nt {
                let u = unsafe { VECTOR_ELT(t, it as i64) };
                check_vector_na(u, rval_int, len);
                it += 1;
            }
        } else {
            check_vector_na(car, rval_int, len);
        }
        s = unsafe { CDR(s) };
    }

    rval
}
