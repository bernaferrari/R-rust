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

// ---------------------------------------------------------------------------
// do_sequence: sequence()
// ---------------------------------------------------------------------------

pub unsafe fn do_sequence(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        checkArity(op, args);

        // User-facing signature: sequence(nvec, from = 1L, by = 1L,
        // recycle = FALSE). Stock R wraps
        // .Internal(sequence(nvec, from, by, recycle)) with as.integer()
        // coercion of the numeric arguments; apply the same defaults and
        // coercion here rather than exposing the raw internal signature.
        // Match user args by tag/position so named calls like by= bind correctly.
        let formals = allocFormalsList5(
            Rf_install_stub(b"nvec\0".as_ptr() as *const c_char),
            Rf_install_stub(b"from\0".as_ptr() as *const c_char),
            Rf_install_stub(b"by\0".as_ptr() as *const c_char),
            Rf_install_stub(b"recycle\0".as_ptr() as *const c_char),
            R_DotsSymbol(),
        );
        let matched = matchArgs_NR(formals, args, call);

        let lengths_arg = CAR(matched);
        if lengths_arg.is_null() || lengths_arg == R_NilValue() || lengths_arg == R_MissingArg() {
            error("argument \"nvec\" is missing, with no default");
        }
        let lengths: SEXP = if TYPEOF(lengths_arg) != INTSXP_VAL {
            coerceVector(lengths_arg, INTSXP_VAL)
        } else {
            lengths_arg
        };
        let from_arg = CADR(matched);
        let from: SEXP =
            if from_arg.is_null() || from_arg == R_NilValue() || from_arg == R_MissingArg() {
                ScalarInteger(1)
            } else if TYPEOF(from_arg) != INTSXP_VAL {
                coerceVector(from_arg, INTSXP_VAL)
            } else {
                from_arg
            };
        let by_arg = CADDR(matched);
        let by: SEXP = if by_arg.is_null() || by_arg == R_NilValue() || by_arg == R_MissingArg() {
            ScalarInteger(1)
        } else if TYPEOF(by_arg) != INTSXP_VAL {
            coerceVector(by_arg, INTSXP_VAL)
        } else {
            by_arg
        };
        let recycle_1st_arg = CADDDR(matched);
        let recycle_1st = if recycle_1st_arg.is_null()
            || recycle_1st_arg == R_NilValue()
            || recycle_1st_arg == R_MissingArg()
        {
            false
        } else {
            asBool2(recycle_1st_arg, call) != 0
        };
        let lengths_len = XLENGTH(lengths);
        let from_len = XLENGTH(from);
        let by_len = XLENGTH(by);

        // sequence(integer(0)) is integer(0) regardless of the other args.
        if lengths_len == 0 {
            return Rf_allocVector(INTSXP_VAL, 0);
        }

        if !recycle_1st && lengths_len != 0 {
            if from_len == 0 {
                error("'from' has length 0, but not 'nvec'; 'recycle = TRUE' returns empty here");
            }
            if by_len == 0 {
                error("'by' has length 0, but not 'nvec'; 'recycle = TRUE' returns empty here");
            }
        } else {
            if from_len == 0 || by_len == 0 {
                return Rf_allocVector(INTSXP_VAL, 0);
            }
        }

        let mut max_len = std::cmp::max(std::cmp::max(lengths_len, from_len), by_len);

        // A shorter 'nvec' with recycle = FALSE only uses the first
        // lengths_len inputs; warn (if `recycle` was not supplied) that
        // future R's default 'recycle = TRUE' will recycle 'nvec' -- at most
        // once per R session.
        if !recycle_1st && lengths_len < max_len {
            // C's maybe_warn: the R-level wrapper passes 0L when `recycle`
            // is missing; the port sees R_MissingArg directly.
            let maybe_warn = recycle_1st_arg == R_MissingArg();
            static WARN_1ST: AtomicBool = AtomicBool::new(true);
            if maybe_warn && WARN_1ST.swap(false, Ordering::Relaxed) {
                let msg = format!(
                    "length(nvec) = {} < {} = max(length(from), length(by))",
                    lengths_len, max_len
                );
                let c_msg = std::ffi::CString::new(format!(
                    "{} -- future R's default 'recycle = TRUE' will recycle 'nvec'",
                    msg
                ))
                .unwrap_or_default();
                warningcall(R_NilValue(), c_msg.as_ptr());
            }
            max_len = lengths_len;
        }

        let lengths_elt = INTEGER(lengths);

        // Calculate total length
        let mut ans_len: R_xlen_t = 0;
        let mut i1: R_xlen_t = 0;
        for _i in 0..max_len {
            if recycle_1st && i1 >= lengths_len {
                i1 = 0;
            }
            let len_i = *lengths_elt.add(i1 as usize);
            if len_i == NA_INTEGER || len_i < 0 {
                error("'nvec' must be a vector of non-negative integers");
            }
            ans_len += len_i as R_xlen_t;
            i1 += 1;
        }

        let ans = Rf_allocVector(INTSXP_VAL, ans_len as c_int);
        let ans_elt = INTEGER(ans);
        let pfrom = INTEGER(from);
        let pby = INTEGER(by);

        let mut offset: R_xlen_t = 0;
        i1 = 0;
        let mut i2: R_xlen_t = 0;
        let mut i3: R_xlen_t = 0;
        for _i in 0..max_len {
            if recycle_1st && i1 >= lengths_len {
                i1 = 0;
            }
            if i2 >= from_len {
                i2 = 0;
            }
            if i3 >= by_len {
                i3 = 0;
            }
            let length_i = *lengths_elt.add(i1 as usize) as R_xlen_t;
            let from_val = *pfrom.add(i2 as usize);
            if length_i != 0 && from_val == NA_INTEGER {
                error("'from' contains NAs");
            }
            let by_val = *pby.add(i3 as usize);
            if length_i >= 2 && by_val == NA_INTEGER {
                error("'by' contains NAs");
            }
            let mut j = from_val;
            for _k in 0..length_i {
                *ans_elt.add(offset as usize) = j;
                j += by_val;
                offset += 1;
            }
            i1 += 1;
            i2 += 1;
            i3 += 1;
        }

        ans
    }
}
