//! `match.call`, `sys.nframe`, `sys.function`, `on.exit`.

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
// Complete R runtime — match.call, sys.nframe, sys.function, on.exit
// ---------------------------------------------------------------------------

/// R's `match.call(definition, call, expand.dots)` — match call arguments.
/// Simplified: returns the call as-is.
pub unsafe fn do_match_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Return the call argument if provided, otherwise the current call
        let call_arg = CAR(args);
        if !call_arg.is_null() && call_arg != R_NilValue() {
            return call_arg;
        }
        _call
    }
}

/// R's `sys.nframe()` — returns the number of frames on the call stack.
pub unsafe fn do_sys_nframe(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let top = crate::sexp::context::R_GlobalContext();
        Rf_ScalarInteger(crate::eval::context::framedepth(top))
    }
}

/// R's `sys.function(which)` — returns the function at the given frame level.
pub unsafe fn do_sys_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_sysfunction(which, top)
        }
    }
}

/// R's `on.exit(expr, add, after)` — register an exit handler for the
/// current function context.
pub unsafe fn do_on_exit(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::special::do_on_exit_from_args(args, rho) }
}
