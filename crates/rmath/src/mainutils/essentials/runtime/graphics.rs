//! `par`, `layout`, `getGraphicsEvent` stubs.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

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
// Complete R runtime — par, getGraphicsEvent
// ---------------------------------------------------------------------------

/// R's `par(...)` — session-owned graphical parameters.
pub unsafe fn do_par(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_par(_call, _op, _args, _rho) }
}

/// R's `layout(...)` — session-owned base graphics layout state.
pub unsafe fn do_layout(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_layout(_call, _op, _args, _rho) }
}

/// R's `getGraphicsEvent(prompt, onMouseDown, ...)` — no Android event loop is attached here.
pub unsafe fn do_getGraphicsEvent(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    std::panic::panic_any(RError {
        message: "graphics events are not available for the headless Android device".to_string(),
    });
}
