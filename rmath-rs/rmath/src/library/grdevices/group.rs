/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/group.c
 *
 *  Group definition and usage (stub - requires GE).
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// defineGroup - define a group on the current device.
pub fn defineGroup(_args: SEXP) -> SEXP {
    unsupported("grDevices::defineGroup")
}

/// useGroup - use a group on the current device.
pub fn useGroup(_args: SEXP) -> SEXP {
    unsupported("grDevices::useGroup")
}

/// devUp - check if the device has y increasing upward.
pub fn devUp(_args: SEXP) -> SEXP {
    unsupported("grDevices::devUp")
}
