/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/do_substitute_direct.c
 */

use std::os::raw::c_char;

use crate::sexp::accessors::TYPEOF;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_NilValue};
use crate::sexp::memory_ext::NewEnvironment;
use crate::sexp::protect::protect;

/// Substitute in an already evaluated object with an explicit list-like environment.
pub unsafe fn do_substitute_direct(f: SEXP, mut env: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(env) == SEXPTYPE::VECSXP {
            let pairlist = crate::mainutils::subassign::VectorToPairList(env);
            let _pairlist_guard = protect(pairlist);
            env = NewEnvironment(pairlist, R_BaseEnv(), R_NilValue());
        } else if TYPEOF(env) == SEXPTYPE::LISTSXP {
            let pairlist = crate::mainutils::duplicate::duplicate(env);
            let _pairlist_guard = protect(pairlist);
            env = NewEnvironment(pairlist, R_BaseEnv(), R_NilValue());
        }

        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            crate::mainutils::errors::Rf_error(
                b"invalid list for substitution\0".as_ptr() as *const c_char
            );
        }

        let _env_guard = protect(env);
        let _f_guard = protect(f);
        crate::mainutils::coerce::substitute(f, env)
    }
}
