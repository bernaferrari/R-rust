/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/clippath.c
 *
 *  Clipping path support (stub - requires GE).
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// setClipPath - set the clipping path for the current device.
pub fn setClipPath(_args: SEXP) -> SEXP {
    unsupported("grDevices::setClipPath")
}
