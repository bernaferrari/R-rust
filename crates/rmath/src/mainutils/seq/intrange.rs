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
        // `0:2147483647` has length 2^31. That length does not fit in `c_int`,
        // and both endpoints do. Truncating the length would allocate a
        // negative vector. A range whose endpoint does not fit in `c_int`
        // (seq_len past INT_MAX, or `(-2147483649):1`) is a real sequence.
        let n = if from <= to {
            to.saturating_sub(from).saturating_add(1)
        } else {
            from.saturating_sub(to).saturating_add(1)
        };
        if n > 1 {
            if let (Ok(from_i), Ok(_to_i)) = (c_int::try_from(from), c_int::try_from(to)) {
                let step: c_int = if from <= to { 1 } else { -1 };
                if let Ok(nu) = usize::try_from(n) {
                    return crate::sexp::altseq::compact_int_seq(from_i, step, nu);
                }
            } else {
                let step = if from <= to { 1.0 } else { -1.0 };
                if let Ok(nu) = usize::try_from(n) {
                    return crate::sexp::altseq::compact_real_seq(from as c_double, step, nu);
                }
            }
            return ptr::null_mut();
        }
        // One element. An endpoint outside `c_int` is a real scalar, not a
        // truncated integer.
        if c_int::try_from(from).is_err() {
            return Rf_ScalarReal(from as c_double);
        }
        let ans = Rf_allocVector(INTSXP_VAL, 1);
        if !ans.is_null() {
            let data = INTEGER(ans);
            if !data.is_null() {
                *data = from as c_int;
            }
        }
        ans
    }
}
