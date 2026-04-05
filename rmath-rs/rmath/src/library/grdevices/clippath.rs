
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/clippath.c
 *
 *  Clipping path support (stub - requires GE).
 */

use std::ptr;

use crate::sexp::accessors::{CADDR, CADR, CAR, CDR};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

type pGEDevDesc = *mut std::ffi::c_void;

/// Stub: GEcurrentDevice - returns null.
unsafe fn GEcurrentDevice() -> pGEDevDesc {
    ptr::null_mut()
}

/// setClipPath - set the clipping path for the current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setClipPath(args: SEXP) -> SEXP {
    let _dd = GEcurrentDevice();
    // Stub: cannot access dd->appending or dd->dev->setClipPath on void* dd
    R_NilValue()
}
