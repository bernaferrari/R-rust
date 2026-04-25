#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/init.c
 *
 *  Registration table for grDevices package.
 *  In the monolithic crate, R_registerRoutines is a no-op since
 *  all symbols are already visible.
 */

use crate::library::grdevices::colors::initPalette;
use crate::main::coerce::asInteger;
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::SEXP;

/// Stub: cairoProps - return whether Cairo features are available.
/// Returns FALSE (0) for both Cairo and PangoCairo.
pub unsafe fn cairoProps(in_: SEXP) -> SEXP {
    let which = asInteger(in_);
    if which == 1 {
        // HAVE_WORKING_CAIRO
        Rf_ScalarLogical(0)
    } else if which == 2 {
        // HAVE_PANGOCAIRO
        Rf_ScalarLogical(0)
    } else {
        use crate::sexp::globals::R_NilValue;
        R_NilValue()
    }
}

/// R_init_grDevices - registration entry point (stub).
/// In the monolithic crate, this is a no-op.
pub unsafe fn R_init_grDevices(_dll: *mut std::ffi::c_void) {
    initPalette();
    // R_registerRoutines, R_useDynamicSymbols, R_forceSymbols
    // are no-ops in the monolithic crate
}
