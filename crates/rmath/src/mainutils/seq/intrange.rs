#![allow(unused_imports)]
use super::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDDR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME,
    RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH, translateChar,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_allocVector3, Rf_isInteger, Rf_isNull,
    Rf_isReal, Rf_isVector, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, R_xlen_t, SEXP};
use crate::sexp::globals::{R_MissingArg, R_NilValue};

pub unsafe fn R_compact_intrange(from: R_xlen_t, to: R_xlen_t) -> SEXP {
    unsafe {
        let n = (if from <= to { to - from } else { from - to } + 1) as c_int;
        let ans = Rf_allocVector(INTSXP_VAL, n);
        if !ans.is_null() && n > 0 {
            let data = INTEGER(ans);
            let step: c_int = if from <= to { 1 } else { -1 };
            let mut val = from as c_int;
            for i in 0..n as usize {
                *data.add(i) = val;
                val += step;
            }
        }
        ans
    }
}
