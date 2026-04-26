/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/init.c
 *
 *  Registration table for grDevices package.
 *  In the monolithic crate, R_registerRoutines is a no-op since
 *  all symbols are already visible.
 */

use std::ffi::c_void;

use crate::library::grdevices::colors::initPalette;
use crate::main::coerce::asInteger;
use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::SEXP;

/// cairoProps - return whether Cairo features are available.
/// Returns FALSE (0) for both Cairo and PangoCairo and errors for any
/// unsupported selector.
pub unsafe fn cairoProps(in_: SEXP) -> SEXP {
    let which = unsafe { asInteger(in_) };
    match which {
        1 | 2 => unsafe { Rf_ScalarLogical(0) },
        _ => {
            Rf_error_unimplemented("grDevices::cairoProps");
            unreachable!("Rf_error_unimplemented returned");
        }
    }
}

/// R_init_grDevices - registration entry point (stub).
/// In the monolithic crate, this is a no-op.
pub fn R_init_grDevices(_dll: *mut c_void) {
    initPalette();
    // R_registerRoutines, R_useDynamicSymbols, R_forceSymbols
    // are no-ops in the monolithic crate
}
