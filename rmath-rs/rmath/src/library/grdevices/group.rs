
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/group.c
 *
 *  Group definition and usage (stub - requires GE).
 */

use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::R_NilValue;

type pGEDevDesc = *mut std::ffi::c_void;
type pDevDesc = *mut std::ffi::c_void;

/// Stub: GEcurrentDevice - returns null.
unsafe fn GEcurrentDevice() -> pGEDevDesc {
    ptr::null_mut()
}

/// Stub: GEMode - no-op.
#[unsafe(no_mangle)]
unsafe fn GEMode(_mode: c_int, _dd: pGEDevDesc) {
    // no-op
}

/// defineGroup - define a group on the current device.
pub unsafe fn defineGroup(_args: SEXP) -> SEXP {
    let _dd = GEcurrentDevice();
    // Stub: cannot access dd->dev->defineGroup on void* dd
    R_NilValue()
}

/// useGroup - use a group on the current device.
pub unsafe fn useGroup(_args: SEXP) -> SEXP {
    let _dd = GEcurrentDevice();
    GEMode(1, _dd);
    // Stub: cannot access dd->dev->useGroup on void* dd
    GEMode(0, _dd);
    R_NilValue()
}

/// devUp - check if the device has y increasing upward.
pub unsafe fn devUp(_args: SEXP) -> SEXP {
    // Stub: no device to query; return FALSE
    let ans = Rf_allocVector(SEXPTYPE::LGLSXP.0, 1);
    *LOGICAL(ans).add(0) = 0;
    ans
}

use std::os::raw::c_int;
