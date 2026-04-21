
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/stubs.c
 *
 *  Stub wrappers and devAskNewPage.
 */

use std::ptr;

use crate::main::coerce::asLogical;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_LOGICAL, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

type pGEDevDesc = *mut std::ffi::c_void;

/// Stub: GEcurrentDevice - returns null.
unsafe fn GEcurrentDevice() -> pGEDevDesc {
    ptr::null_mut()
}

/// Stub: do_contourLines - returns R_NilValue.
unsafe fn do_contourLines(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

/// Stub: do_getSnapshot - returns R_NilValue.
unsafe fn do_getSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

/// Stub: do_playSnapshot - returns R_NilValue.
unsafe fn do_playSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

/// do_getGraphicsEvent - delegates to main::gevents::do_getGraphicsEvent.
unsafe fn do_getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    crate::main::essentials::do_getGraphicsEvent(call, op, args, env)
}

/// do_getGraphicsEventEnv - delegates to main::gevents::do_getGraphicsEventEnv.
unsafe fn do_getGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    R_NilValue()
}

/// do_setGraphicsEventEnv - delegates to main::gevents::do_setGraphicsEventEnv.
unsafe fn do_setGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    R_NilValue()
}

/// Stub: do_bmVersion - returns integer 0.
unsafe fn do_bmVersion() -> SEXP {
    Rf_ScalarInteger(0)
}

/// contourLines - wrapper for do_contourLines.
#[unsafe(no_mangle)]
pub unsafe fn contourLines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_contourLines(call, op, CDR(args), env)
}

/// getSnapshot - wrapper for do_getSnapshot.
pub unsafe fn getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_getSnapshot(call, op, CDR(args), env)
}

/// playSnapshot - wrapper for do_playSnapshot.
pub unsafe fn playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_playSnapshot(call, op, CDR(args), env)
}

/// getGraphicsEvent - wrapper for do_getGraphicsEvent.
pub unsafe fn getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_getGraphicsEvent(call, op, CDR(args), env)
}

/// getGraphicsEventEnv - wrapper for do_getGraphicsEventEnv.
pub unsafe fn getGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_getGraphicsEventEnv(call, op, CDR(args), env)
}

/// setGraphicsEventEnv - wrapper for do_setGraphicsEventEnv.
pub unsafe fn setGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_setGraphicsEventEnv(call, op, CDR(args), env)
}

/// bmVersion - wrapper for do_bmVersion.
pub(crate) unsafe fn bmVersion() -> SEXP {
    do_bmVersion()
}

/// devAskNewPage - get/set the "ask new page" flag for the current device.
pub unsafe fn devAskNewPage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _dd = GEcurrentDevice();
    // Stub: cannot access gdd->ask on void* gdd; return FALSE
    Rf_ScalarLogical(0)
}

use std::os::raw::c_int;
