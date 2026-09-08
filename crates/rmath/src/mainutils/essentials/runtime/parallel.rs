//! Parallel operations (simplified serial fallbacks).

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — parallel operations (simplified)
// ---------------------------------------------------------------------------

/// R's `parallel::mclapply(X, FUN, ...)` — parallel lapply (simplified serial version).
pub unsafe fn do_mclapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `future.apply::future_lapply(X, FUN, ...)` — future lapply (simplified serial version).
pub unsafe fn do_future_lapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `doParallel::foreach(...)` — parallel foreach (simplified serial version).
pub unsafe fn do_foreach(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x).max(1) as usize;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let dst = (*result).gengc_next_node as *mut SEXP;
        for i in 0..n {
            let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
                let src = (*x).gengc_next_node as *const SEXP;
                *src.add(i)
            } else {
                R_NilValue()
            };
            *dst.add(i) = elt;
        }
        result
    }
}
