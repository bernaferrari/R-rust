/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/stubs.c
 *
 *  Stub wrappers and devAskNewPage.
 */

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::accessors::{CDR, SET_STRING_ELT};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

fn tail(args: SEXP) -> SEXP {
    unsafe { CDR(args) }
}

fn do_contourLines(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::contourLines")
}

fn do_getSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::getSnapshot")
}

fn do_playSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::playSnapshot")
}

fn do_getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::main::essentials::do_getGraphicsEvent(call, op, args, env) }
}

fn do_getGraphicsEventEnv(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::getGraphicsEventEnv")
}

fn do_setGraphicsEventEnv(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::setGraphicsEventEnv")
}

fn do_bmVersion() -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _ans_guard = protect(ans);
        let nms = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _nms_guard = protect(nms);

        SET_STRING_ELT(nms, 0 as R_xlen_t, Rf_mkChar(c"libpng".as_ptr()));
        SET_STRING_ELT(nms, 1 as R_xlen_t, Rf_mkChar(c"jpeg".as_ptr()));
        SET_STRING_ELT(nms, 2 as R_xlen_t, Rf_mkChar(c"libtiff".as_ptr()));

        SET_STRING_ELT(ans, 0 as R_xlen_t, Rf_mkChar(c"".as_ptr()));
        SET_STRING_ELT(ans, 1 as R_xlen_t, Rf_mkChar(c"".as_ptr()));
        SET_STRING_ELT(ans, 2 as R_xlen_t, Rf_mkChar(c"".as_ptr()));
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), nms);

        ans
    }
}

/// contourLines - wrapper for do_contourLines.
pub unsafe fn contourLines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_contourLines(call, op, args, env)
}

/// getSnapshot - wrapper for do_getSnapshot.
pub unsafe fn getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_getSnapshot(call, op, args, env)
}

/// playSnapshot - wrapper for do_playSnapshot.
pub unsafe fn playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_playSnapshot(call, op, args, env)
}

/// getGraphicsEvent - wrapper for do_getGraphicsEvent.
pub unsafe fn getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_getGraphicsEvent(call, op, tail(args), env)
}

/// getGraphicsEventEnv - wrapper for do_getGraphicsEventEnv.
pub unsafe fn getGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_getGraphicsEventEnv(call, op, args, env)
}

/// setGraphicsEventEnv - wrapper for do_setGraphicsEventEnv.
pub unsafe fn setGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_setGraphicsEventEnv(call, op, args, env)
}

/// bmVersion - wrapper for do_bmVersion.
pub(crate) fn bmVersion() -> SEXP {
    do_bmVersion()
}

/// devAskNewPage - get/set the "ask new page" flag for the current device.
pub unsafe fn devAskNewPage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    unsupported("grDevices::devAskNewPage")
}
