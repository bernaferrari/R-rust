/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/mask.c
 *
 *  Mask support (stub - requires GE).
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// setMask - set the mask for the current device.
pub fn setMask(_args: SEXP) -> SEXP {
    unsupported("grDevices::setMask")
}
