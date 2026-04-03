#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/mapply.c — mapply builtin.
//!
//! `mapply` applies a function to multiple lists/vectors in parallel,
//! recycling shorter arguments to match the longest.

use std::ptr;

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, SET_VECTOR_ELT, SETCDR, SETTAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// Helper: set the TAG of a cons cell.
unsafe fn SET_TAG(x: SEXP, y: SEXP) {
    unsafe {
        SETTAG(x, y);
    }
}

/// `mapply(FUN, ..., MoreArgs)` — apply a function to multiple lists/vectors.
///
/// Port of R's `do_mapply` from src/main/mapply.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_mapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // --- Extract FUN, dots, MoreArgs ---
        let fun = CAR(args);
        let dots = CADR(args);
        let moreargs = CADDR(args);

        // --- Count varying arguments and collect them into a vector ---
        let mut varyings: Vec<SEXP> = Vec::new();
        let mut ap = dots;
        while !ap.is_null() && ap != R_NilValue() {
            let elt = CAR(ap);
            varyings.push(elt);
            ap = CDR(ap);
        }

        if varyings.is_empty() {
            return Rf_allocVector3(SEXPTYPE::LISTSXP.0, 0);
        }

        // --- Compute max length ---
        let mut maxlen: R_xlen_t = 0;
        for &v in &varyings {
            let t = TYPEOF(v);
            let vl = if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::LISTSXP.0 {
                let mut len: R_xlen_t = 0;
                let mut p = v;
                while !p.is_null() && p != R_NilValue() {
                    len += 1;
                    p = CDR(p);
                }
                len
            } else {
                XLENGTH(v)
            };
            if vl > maxlen {
                maxlen = vl;
            }
        }

        if maxlen == 0 {
            return Rf_allocVector3(SEXPTYPE::LISTSXP.0, 0);
        }

        // --- Check for zero-length args alongside longer ones ---
        for &v in &varyings {
            let t = TYPEOF(v);
            let vl = if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::LISTSXP.0 {
                let mut len: R_xlen_t = 0;
                let mut p = v;
                while !p.is_null() && p != R_NilValue() {
                    len += 1;
                    p = CDR(p);
                }
                len
            } else {
                XLENGTH(v)
            };
            if vl == 0 {
                return Rf_allocVector3(SEXPTYPE::LISTSXP.0, 0);
            }
        }

        // --- Determine MoreArgs count ---
        let nmoreargs: R_xlen_t = if moreargs.is_null() || moreargs == R_NilValue() {
            0
        } else {
            XLENGTH(moreargs)
        };

        // --- Build and evaluate calls ---
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LISTSXP.0, maxlen));

        for i in 0..maxlen as usize {
            // Build the call: FUN(varying[0][[i]], varying[1][[i]], ..., MoreArgs[[j]])
            let mut call_args = R_NilValue();
            let mut tail: SEXP = ptr::null_mut();

            // Add varying arguments (recycled)
            for &v in &varyings {
                let t = TYPEOF(v);
                let vl = if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::LISTSXP.0 {
                    let mut len: R_xlen_t = 0;
                    let mut p = v;
                    while !p.is_null() && p != R_NilValue() {
                        len += 1;
                        p = CDR(p);
                    }
                    len
                } else {
                    XLENGTH(v)
                };
                let idx = i % (vl as usize);

                let elt = if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::LISTSXP.0 {
                    let mut p = v;
                    for _ in 0..idx {
                        p = CDR(p);
                    }
                    CAR(p)
                } else {
                    VECTOR_ELT(v, idx as R_xlen_t)
                };

                // Create pairlist cell
                let cell = Rf_protect(Rf_cons(elt, R_NilValue()));

                if call_args.is_null() || call_args == R_NilValue() {
                    call_args = cell;
                } else {
                    if tail.is_null() {
                        let mut p = call_args;
                        while !CDR(p).is_null() && CDR(p) != R_NilValue() {
                            p = CDR(p);
                        }
                        SETCDR(p, cell);
                    } else {
                        SETCDR(tail, cell);
                    }
                }
                tail = cell;
                Rf_unprotect(1);
            }

            // Add MoreArgs
            if nmoreargs > 0 {
                for j in 0..nmoreargs as usize {
                    let m_elt = VECTOR_ELT(moreargs, j as R_xlen_t);
                    let cell = Rf_protect(Rf_cons(m_elt, R_NilValue()));
                    if call_args.is_null() || call_args == R_NilValue() {
                        call_args = cell;
                    } else {
                        if tail.is_null() {
                            let mut p = call_args;
                            while !CDR(p).is_null() && CDR(p) != R_NilValue() {
                                p = CDR(p);
                            }
                            SETCDR(p, cell);
                        } else {
                            SETCDR(tail, cell);
                        }
                    }
                    tail = cell;
                    Rf_unprotect(1);
                }
            }

            // Build LANGSXP: FUN(args...)
            let call = Rf_protect(Rf_lang2(fun, call_args));
            // Evaluate the call
            let val = crate::eval::eval::Rf_eval(call, rho);
            SET_VECTOR_ELT(ans, i as R_xlen_t, val);
            Rf_unprotect(1);
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_mapply_null_args_returns_empty() {
        unsafe {
            let empty_list = Rf_allocVector3(SEXPTYPE::LISTSXP.0, 0);
            let args = crate::sexp::memory_ext::allocList(3);
            crate::sexp::accessors::SETCAR(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), empty_list);
            crate::sexp::accessors::SETCAR(CDR(CDR(args)), R_NilValue());

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(XLENGTH(result), 0);
        }
    }
}
